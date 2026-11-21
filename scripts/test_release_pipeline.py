"""Release orchestration invariants; synthetic API fixtures, no real credentials."""
import base64
from contextlib import redirect_stdout
import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from test_release_version import FakeGitHub, MAIN, SOURCE, missing

spec = importlib.util.spec_from_file_location("pipeline", Path(__file__).with_name("release-pipeline.py"))
pipeline = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pipeline)
promote_spec = importlib.util.spec_from_file_location("promotion", Path(__file__).with_name("promote-release.py"))
promotion = importlib.util.module_from_spec(promote_spec)
promote_spec.loader.exec_module(promotion)


class SelectionAPI(FakeGitHub):
    def __init__(self):
        super().__init__()
        self.tag = SOURCE


class TapAPI:
    def __init__(self, text=None):
        self.text = text
        self.sha = "old" if text else None
        self.writes = []
        self.race = False
        self.forbidden = False

    def api(self, route, payload=None, method="POST"):
        assert route == "contents/Casks/jaywalk.rb"
        if payload is None:
            if self.text is None:
                raise missing()
            return {"sha": self.sha, "content": base64.b64encode(self.text.encode()).decode()}
        assert method == "PUT"
        self.writes.append(copy.deepcopy(payload))
        if self.forbidden:
            raise subprocess.CalledProcessError(1, ["gh"], stderr="Forbidden (HTTP 403)")
        if self.race:
            self.race = False
            self.sha = "changed"
            raise subprocess.CalledProcessError(1, ["gh"], stderr="Conflict (HTTP 409)")
        if self.sha:
            assert payload["sha"] == self.sha
        self.text = base64.b64decode(payload["content"]).decode()
        self.sha = "new"
        return {}


class PipelineTests(unittest.TestCase):
    def test_selects_tagged_ancestor_without_main_or_ref_writes(self):
        api = SelectionAPI()
        result = pipeline.select(api, "v2026.11.1")
        self.assertEqual(result, {"source": SOURCE, "tag": "v2026.11.1", "build": "false"})
        self.assertEqual(api.main, MAIN)
        self.assertEqual(api.mutations, [])

    def test_missing_release_builds_only_the_selected_source(self):
        api = SelectionAPI()
        with patch.object(pipeline, "candidate", return_value=None):
            result = pipeline.select(api, "v2026.11.1")
        self.assertEqual(result["source"], SOURCE)
        self.assertEqual(result["build"], "true")
        self.assertEqual(api.mutations, [])

    def test_tag_and_lock_versions_must_match(self):
        for path in ("Cargo.toml", "Cargo.lock"):
            api = SelectionAPI()
            api.files[SOURCE][path] = api.files[SOURCE][path].replace("2026.11.1", "2026.11.2", 1)
            with self.assertRaises(ValueError):
                pipeline.select(api, "v2026.11.1")
            self.assertEqual(api.mutations, [])

    def test_invalid_or_unrelated_source_rejected(self):
        for tag in ("main", "v2026.13.1", "v2026.11.01", "v2026.11.1-dev"):
            with self.assertRaises(ValueError):
                pipeline.select(SelectionAPI(), tag)
        api = SelectionAPI()
        api.relation = "diverged"
        with self.assertRaises(ValueError):
            pipeline.select(api, "v2026.11.1")

    def test_partial_draft_can_rebuild_but_public_release_cannot(self):
        api = SelectionAPI()
        api.release["assets"].pop()
        with self.assertRaises(ValueError):
            pipeline.candidate(api, "v2026.11.1")
        api.release["draft"] = True
        self.assertIsNone(pipeline.candidate(api, "v2026.11.1"))

    def test_complete_but_unofficial_draft_is_rejected(self):
        api = SelectionAPI()
        api.release["draft"] = True
        api.report["official"] = False
        with self.assertRaises(ValueError):
            pipeline.select(api, "v2026.11.1")

    def test_retry_can_replace_only_an_unpublished_draft(self):
        api = SelectionAPI()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "manifest.json").write_text(json.dumps(api.report))
            with patch.object(pipeline.subprocess, "run") as run:
                with self.assertRaisesRegex(ValueError, "never replace"):
                    pipeline.create_draft(api, "v2026.11.1", root)
                run.assert_not_called()
            api.release.update(draft=True, id=123)
            original = api.api
            deleted = []
            def request(route, payload=None, method=None):
                if method == "DELETE":
                    deleted.append(route)
                    return None
                return original(route, payload, method)
            api.api = request
            with patch.object(pipeline.subprocess, "run") as run:
                pipeline.create_draft(api, "v2026.11.1", root)
                self.assertEqual(deleted, ["releases/123"])
                self.assertIn("--verify-tag", run.call_args.args[0])

    def test_bodyless_api_delete_is_explicit_and_accepts_204(self):
        response = subprocess.CompletedProcess([], 0, stdout="", stderr="")
        with patch.object(pipeline.version.subprocess, "run", return_value=response) as run:
            self.assertIsNone(pipeline.version.GitHub("example/project").api("releases/123", method="DELETE"))
        self.assertEqual(run.call_args.args[0][-2:], ["--method", "DELETE"])

    def test_tap_delivery_is_idempotent_and_never_downgrades(self):
        report = SelectionAPI().report
        tap = TapAPI()
        pipeline.update_tap(tap, report)
        self.assertEqual(tap.text, pipeline.metadata.cask(report))
        pipeline.update_tap(tap, report)
        self.assertEqual(len(tap.writes), 1)
        newer = {**report, "version": "2026.12.1", "artifacts": [{"filename": "Jaywalk-2026.12.1-macos-arm64.dmg", "sha256": "d" * 64}]}
        tap.text = pipeline.metadata.cask(newer)
        pipeline.update_tap(tap, report)
        self.assertEqual(len(tap.writes), 1)

    def test_tap_conflicts_retry_and_permission_errors_stop(self):
        report = SelectionAPI().report
        older = {**report, "version": "2026.10.1", "artifacts": [{"filename": "Jaywalk-2026.10.1-macos-arm64.dmg", "sha256": "d" * 64}]}
        tap = TapAPI(pipeline.metadata.cask(older))
        tap.race = True
        pipeline.update_tap(tap, report)
        self.assertEqual(len(tap.writes), 2)
        tap = TapAPI()
        tap.forbidden = True
        with self.assertRaises(subprocess.CalledProcessError):
            pipeline.update_tap(tap, report)
        self.assertEqual(len(tap.writes), 1)

    def test_conflicting_same_version_cask_is_preserved(self):
        report = SelectionAPI().report
        tap = TapAPI(pipeline.metadata.cask(report).replace('"c' + 'c' * 63 + '"', '"' + 'd' * 64 + '"'))
        with self.assertRaises(ValueError):
            pipeline.update_tap(tap, report)
        self.assertEqual(tap.writes, [])

    def test_completion_only_updates_tap_and_preserves_main(self):
        api = SelectionAPI()
        with redirect_stdout(io.StringIO()):
            pipeline.finish(api, TapAPI(), "v2026.11.1")
        self.assertEqual(api.main, MAIN)
        self.assertEqual(api.mutations, [])

    def test_draft_cannot_update_public_tap_or_development_version(self):
        api = SelectionAPI()
        api.tag = SOURCE
        api.release["draft"] = True
        tap = TapAPI()
        with self.assertRaises(ValueError):
            pipeline.finish(api, tap, "v2026.11.1")
        self.assertEqual(tap.writes, [])
        self.assertEqual(api.mutations, [])


class PromotionTests(unittest.TestCase):
    def test_publishing_historical_versions_never_moves_latest_backwards(self):
        api = SelectionAPI()
        with patch.object(api, "api", return_value={"tag_name": "v2026.11.10"}):
            self.assertFalse(promotion.should_be_latest(api, "2026.11.9"))
            self.assertTrue(promotion.should_be_latest(api, "2026.12.1"))
        with patch.object(api, "api", side_effect=missing()):
            self.assertTrue(promotion.should_be_latest(api, "2026.11.1"))

    def test_native_children_never_inherit_secrets(self):
        with patch.dict("os.environ", {"SPARKLE_PRIVATE_KEY": "secret", "GH_TOKEN": "token", "GITHUB_TOKEN": "token"}):
            self.assertNotIn("SPARKLE_PRIVATE_KEY", promotion.child_environment("gh"))
            for key in ("GH_TOKEN", "GITHUB_TOKEN", "SPARKLE_PRIVATE_KEY"):
                self.assertNotIn(key, promotion.child_environment("codesign"))

    def test_downloaded_checksum_tampering_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "payload.zip").write_bytes(b"original")
            report = {"artifacts": [{"filename": "payload.zip", "bytes": 8, "sha256": hashlib.sha256(b"original").hexdigest()}]}
            promotion.verify_bytes(root, report)
            (root / "payload.zip").write_bytes(b"modified")
            with self.assertRaises(ValueError):
                promotion.verify_bytes(root, report)

    def test_signed_feed_must_target_the_exact_archive(self):
        report = SelectionAPI().report
        signature = base64.b64encode(bytes(64)).decode()
        feed = f'''<rss xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle"><channel><item>
< sparkleversion />
<enclosure url="https://github.com/example/project/releases/download/v2026.11.1/Jaywalk-2026.11.1-macos-arm64.zip" length="123" sparkle:edSignature="{signature}" />
</item></channel></rss>'''.replace('< sparkleversion />', '<sparkle:version>2026.11.1</sparkle:version>')
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "appcast.xml"
            path.write_text(feed)
            self.assertEqual(promotion.enclosure(path, "v2026.11.1", report, "example/project")[1], signature)
            for changed in (feed.replace("example/project/releases", "other/project/releases"), feed.replace('length="123"', 'length="124"'), feed.replace("2026.11.1</sparkle", "2026.11.2</sparkle")):
                path.write_text(changed)
                with self.assertRaises(ValueError):
                    promotion.enclosure(path, "v2026.11.1", report, "example/project")


if __name__ == "__main__":
    unittest.main()
