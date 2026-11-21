"""Release orchestration invariants; synthetic API fixtures, no real credentials."""
import base64
from datetime import datetime, timezone
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


class DraftAPI(SelectionAPI):
    """Match GitHub: a draft is listed but the published-by-tag API returns 404."""
    def __init__(self):
        super().__init__()
        self.release.update(draft=True, id=123, published_at=None)
        self.pages = [[{"id": 123, "tag_name": "v2026.11.1"}]]
        self.reads = []

    def api(self, route, payload=None, method="POST"):
        self.reads.append(route)
        if route.startswith("releases/tags/"):
            raise missing()
        if route.startswith("releases?per_page=100&page="):
            return self.pages[int(route.rsplit("=", 1)[1]) - 1]
        if route == "releases/123":
            return self.release
        return super().api(route, payload, method)


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
        self.assertEqual(result, {"source": SOURCE, "tag": "v2026.11.1", "build": "false", "version": "2026.11.1", "checks": "77"})
        self.assertEqual(api.main, MAIN)
        self.assertEqual(api.mutations, [])

    def test_complete_draft_is_discovered_and_reused_without_rebuilding(self):
        api = DraftAPI()
        result = pipeline.select(api, "v2026.11.1")
        self.assertEqual(result["build"], "false")
        self.assertIn("releases/123", api.reads)
        self.assertEqual(api.mutations, [])

    def test_draft_discovery_paginates_and_matches_exact_tag(self):
        api = DraftAPI()
        api.pages.insert(0, [{"id": i, "tag_name": "v2026.11.10"} for i in range(100)])
        self.assertEqual(pipeline.find_release(api, "v2026.11.1"), api.release)
        self.assertIn("releases?per_page=100&page=2", api.reads)

    def test_ambiguous_or_changed_draft_identity_is_rejected(self):
        api = DraftAPI()
        api.pages[0].append({"id": 456, "tag_name": "v2026.11.1"})
        with self.assertRaisesRegex(ValueError, "multiple releases"):
            pipeline.find_release(api, "v2026.11.1")
        api = DraftAPI()
        api.release["tag_name"] = "v2026.11.2"
        with self.assertRaisesRegex(ValueError, "tag changed"):
            pipeline.find_release(api, "v2026.11.1")

    def test_partial_draft_can_be_rebuilt_but_auth_errors_are_not_missing(self):
        api = DraftAPI()
        api.release["assets"].pop()
        self.assertEqual(pipeline.select(api, "v2026.11.1")["build"], "true")
        with patch.object(api, "api", side_effect=subprocess.CalledProcessError(1, ["gh"], stderr="Forbidden (HTTP 403)")):
            with self.assertRaises(subprocess.CalledProcessError):
                pipeline.find_release(api, "v2026.11.1")

    def test_existing_draft_is_found_before_guarded_replacement(self):
        api = DraftAPI()
        original = api.api
        deleted = []
        def request(route, payload=None, method="POST"):
            if method == "DELETE":
                deleted.append(route)
                return None
            return original(route, payload, method)
        api.api = request
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "manifest.json").write_text(json.dumps(api.report))
            with patch.object(pipeline.subprocess, "run"):
                pipeline.create_draft(api, "v2026.11.1", root)
        self.assertEqual(deleted, ["releases/123"])

    def test_missing_release_builds_only_the_selected_source(self):
        api = SelectionAPI()
        with patch.object(pipeline, "candidate", return_value=None):
            result = pipeline.select(api, "v2026.11.1")
        self.assertEqual(result["source"], SOURCE)
        self.assertEqual(result["build"], "true")
        self.assertEqual(api.mutations, [])

    def test_allocation_source_and_checks_must_match(self):
        api = SelectionAPI()
        api.run["head_sha"] = MAIN
        with self.assertRaisesRegex(ValueError, "different source"):
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

    def test_late_historical_build_cannot_update_the_homebrew_channel(self):
        api = SelectionAPI()
        original = api.api
        def request(route, payload=None, method=None):
            if route == "releases/latest":
                return {"tag_name": "v2026.11.2"}
            return original(route, payload, method)
        api.api = request
        tap = TapAPI()
        with redirect_stdout(io.StringIO()):
            pipeline.finish(api, tap, "v2026.11.1")
        self.assertEqual(tap.writes, [])

    def test_draft_cannot_update_public_tap_or_development_version(self):
        api = SelectionAPI()
        api.tag = SOURCE
        api.release["draft"] = True
        tap = TapAPI()
        with self.assertRaises(ValueError):
            pipeline.finish(api, tap, "v2026.11.1")
        self.assertEqual(tap.writes, [])
        self.assertEqual(api.mutations, [])


class AllocationTests(unittest.TestCase):
    now = datetime(2026, 11, 21, tzinfo=timezone.utc)

    def fresh(self):
        api = FakeGitHub()
        api.has_release = False
        return api

    def reserve(self, api, now=None):
        return pipeline.reserve(api, "77", now=now or self.now)

    def test_first_build_reserves_annotated_identity_without_editing_main(self):
        api = self.fresh()
        result = self.reserve(api)
        self.assertEqual(result, {"tag": "v2026.11.1", "source": SOURCE, "version": "2026.11.1", "checks": "77", "build": "true"})
        self.assertEqual(api.main, MAIN)
        self.assertTrue(all(route in ("git/tags", "git/refs") for route, _ in api.mutations))
        self.assertEqual(pipeline.allocation(api, result["tag"])["source_commit"], SOURCE)

    def test_retry_in_another_month_reuses_version_and_original_checks(self):
        api = self.fresh()
        first = self.reserve(api)
        writes = len(api.mutations)
        retry = self.reserve(api, datetime(2027, 1, 2, tzinfo=timezone.utc))
        self.assertEqual(retry, first)
        self.assertEqual(len(api.mutations), writes)

    def test_failed_reserved_numbers_count_and_new_month_resets_counter(self):
        api = self.fresh()
        self.reserve(api)
        api.run.update(head_sha=MAIN, id=78)
        next_build = pipeline.reserve(api, "78", now=self.now)
        self.assertEqual(next_build["version"], "2026.11.2")
        api.main = "c" * 40
        api.run.update(head_sha=api.main, id=79)
        next_month = pipeline.reserve(api, "79", now=datetime(2026, 12, 1, tzinfo=timezone.utc))
        self.assertEqual(next_month["version"], "2026.12.1")

    def test_queued_older_checks_do_not_regress_calendar(self):
        api = self.fresh()
        self.reserve(api, datetime(2026, 12, 1, tzinfo=timezone.utc))
        api.run.update(head_sha=MAIN, id=78)
        result = pipeline.reserve(api, "78", now=self.now)
        self.assertEqual(result["version"], "2026.12.2")

    def test_only_successful_same_repository_main_checks_can_allocate(self):
        mutations = [("event", "pull_request"), ("head_branch", "feature"),
                     ("path", ".github/workflows/other.yml"), ("conclusion", "failure"),
                     ("status", "in_progress"), ("head_repository", {"full_name": "other/repo"})]
        for key, value in mutations:
            api = self.fresh()
            api.run[key] = value
            with self.assertRaises(ValueError):
                self.reserve(api)
            self.assertEqual(api.mutations, [])
        api = self.fresh()
        api.relation = "diverged"
        with self.assertRaises(ValueError):
            self.reserve(api)
        self.assertEqual(api.mutations, [])

    def test_permission_failure_is_not_treated_as_tag_collision(self):
        api = self.fresh()
        api.denied = True
        with self.assertRaises(subprocess.CalledProcessError):
            self.reserve(api)
        self.assertEqual(api.refs, {})
        self.assertEqual(len(api.objects), 1)

    def test_all_tag_pages_are_considered_before_allocating(self):
        api = self.fresh()
        original = api.api
        def paginated(route, payload=None, method=None):
            if route == "tags?per_page=100&page=1":
                return [{"name": "unrelated-" + str(i), "commit": {"sha": MAIN}} for i in range(100)]
            if route == "tags?per_page=100&page=2":
                return [{"name": "v2026.11.99", "commit": {"sha": MAIN}}]
            return original(route, payload, method)
        api.api = paginated
        self.assertEqual(self.reserve(api)["version"], "2026.11.100")

    def test_same_source_collision_reuses_winning_allocation(self):
        api = self.fresh()
        original = api.api
        raced = []
        def race(route, payload=None, method=None):
            if route == "git/refs" and not raced:
                raced.append(True)
                api.refs["v2026.11.1"] = payload["sha"]
            return original(route, payload, method)
        api.api = race
        result = self.reserve(api)
        self.assertEqual(result["version"], "2026.11.1")
        self.assertEqual(len(api.refs), 1)
        self.assertEqual(len(api.objects), 1)

    def test_competing_allocation_retries_without_moving_existing_tag(self):
        api = self.fresh()
        original = api.api
        raced = []
        def race(route, payload=None, method=None):
            if route == "git/refs" and not raced:
                raced.append(True)
                oid = payload["sha"]
                rival = copy.deepcopy(api.objects[oid])
                rival["object"]["sha"] = MAIN
                api.objects["rival"] = rival
                api.refs["v2026.11.1"] = "rival"
            return original(route, payload, method)
        api.api = race
        result = self.reserve(api)
        self.assertEqual(result["version"], "2026.11.2")
        self.assertEqual(api.refs["v2026.11.1"], "rival")


class PromotionTests(unittest.TestCase):
    def test_publishing_historical_versions_never_moves_latest_backwards(self):
        api = SelectionAPI()
        original = api.api
        def request(route, payload=None, method=None):
            if route == "releases/latest":
                return {"tag_name": "v2026.11.10"}
            return original(route, payload, method)
        api.api = request
        self.assertFalse(promotion.should_be_latest(api, "2026.11.9", MAIN))
        with patch.object(promotion.pipeline.version, "tag_commit", return_value=SOURCE):
            self.assertTrue(promotion.should_be_latest(api, "2026.12.1", MAIN))
            api.relation = "behind"
            self.assertFalse(promotion.should_be_latest(api, "2026.12.1", MAIN))
        with patch.object(api, "api", side_effect=missing()):
            self.assertTrue(promotion.should_be_latest(api, "2026.11.1", SOURCE))

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
