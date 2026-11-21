"""Portable release state-machine tests: no network, credentials or real refs."""
import base64
import copy
import importlib.util
from pathlib import Path
import subprocess
import unittest

spec = importlib.util.spec_from_file_location("release_version", Path(__file__).with_name("release-version.py"))
version = importlib.util.module_from_spec(spec)
spec.loader.exec_module(version)
SOURCE = "a" * 40
MAIN = "b" * 40
CARGO = '[package]\nname = "jaywalk"\nversion = "0.1.1" # keep comment\n\n[dependencies]\nother = "0.1.1"\n'
LOCK = 'version = 4\n\n[[package]]\nname = "jaywalk"\nversion = "0.1.1"\ndependencies = ["other"]\n\n[[package]]\nname = "other"\nversion = "0.1.1"\nsource = "registry+https://example.test"\n'


def missing():
    return subprocess.CalledProcessError(1, ["gh"], stderr="Not Found (HTTP 404)")


class FakeGitHub:
    def __init__(self):
        self.repository = "example/project"
        self.main = MAIN
        self.tag = None
        self.files = {SOURCE: {"Cargo.toml": CARGO, "Cargo.lock": LOCK},
                      MAIN: {"Cargo.toml": CARGO, "Cargo.lock": LOCK}}
        self.created = []
        self.mutations = []
        self.races = 0
        self.denied = False
        self.relation = "ahead"
        self.report = {"official": True, "source_tree_clean": True, "version": "0.1.1",
                       "source_commit": SOURCE, "binary_revision": SOURCE[:12],
                       "platform": "macos", "architecture": "arm64", "artifacts": [
                           {"filename": f"Jaywalk-0.1.1-macos-arm64{suffix}", "sha256": "c" * 64, "bytes": 123}
                           for suffix in (".zip", ".dmg")]}
        self.release = {"draft": False, "prerelease": False, "published_at": "2026-11-20T12:00:00Z",
                        "tag_name": "v0.1.1", "assets": [
                            {"name": f["filename"], "digest": "sha256:" + f["sha256"], "size": f["bytes"]}
                            for f in self.report["artifacts"]]}

    def manifest(self, release):
        return self.report

    def api(self, route, payload=None, method="POST"):
        if payload is not None:
            self.mutations.append((route, copy.deepcopy(payload)))
        if route == "git/ref/heads/main":
            return {"object": {"sha": self.main}}
        if route.startswith("contents/"):
            path, ref = route.removeprefix("contents/").split("?ref=")
            return {"content": base64.b64encode(self.files[ref][path].encode()).decode()}
        if route == "git/ref/tags/v0.1.1":
            if self.tag is None:
                raise missing()
            return {"object": {"type": "tag", "sha": "tag-object"}}
        if route == "git/tags/tag-object":
            return {"object": {"type": "commit", "sha": self.tag}}
        if route == "git/tags":
            self.tag_source = payload["object"]
            return {"sha": "tag-object"}
        if route == "git/refs":
            self.tag = self.tag_source
            return {}
        if route.startswith("releases/tags/"):
            return self.release
        if route.startswith("compare/"):
            return {"status": self.relation}
        if route.startswith("git/commits/"):
            return {"tree": {"sha": "parent-tree"}}
        if route == "git/trees":
            self.pending = {f["path"]: f["content"] for f in payload["tree"]}
            return {"sha": "new-tree"}
        if route == "git/commits":
            sha = str(len(self.created) + 1) * 40
            self.created.append(payload)
            self.files[sha] = self.pending.copy()
            return {"sha": sha}
        if route == "git/refs/heads/main":
            assert method == "PATCH" and payload["force"] is False
            if self.denied:
                raise subprocess.CalledProcessError(1, ["gh"], stderr="forbidden")
            if self.races:
                self.races -= 1
                self.main = "d" * 40
                self.files[self.main] = {"Cargo.toml": CARGO, "Cargo.lock": LOCK}
                raise subprocess.CalledProcessError(1, ["gh"], stderr="not fast forward")
            self.main = payload["sha"]
            return {}
        raise AssertionError(route)


class VersionTests(unittest.TestCase):
    def published(self):
        api = FakeGitHub()
        api.tag = SOURCE
        return api

    def test_tag_created_once_at_exact_source(self):
        api = FakeGitHub()
        self.assertEqual(version.ensure_tag(api, MAIN), "v0.1.1")
        self.assertEqual(api.tag, MAIN)
        count = len(api.mutations)
        version.ensure_tag(api, MAIN)
        self.assertEqual(len(api.mutations), count)

    def test_tag_mismatch_and_moving_main_refused(self):
        api = self.published()
        for source in (MAIN, SOURCE, "main"):
            with self.assertRaises(ValueError):
                version.ensure_tag(api, source)
        self.assertEqual(api.mutations, [])

    def test_only_two_versions_change_and_retry_does_nothing(self):
        api = self.published()
        version.advance_main(api, "v0.1.1")
        cargo, lock = api.files[api.main].values()
        self.assertEqual(cargo, CARGO.replace('version = "0.1.1"', 'version = "0.1.2"'))
        self.assertEqual(lock, LOCK.replace('version = "0.1.1"', 'version = "0.1.2"', 1))
        self.assertEqual(api.created[0]["parents"], [MAIN])
        self.assertEqual(api.created[0]["message"], "start 0.1.2 development")
        before = len(api.mutations)
        version.advance_main(api, "v0.1.1")
        self.assertEqual(len(api.mutations), before)

    def test_race_rebases_onto_latest_main(self):
        api = self.published()
        api.races = 1
        version.advance_main(api, "v0.1.1")
        self.assertEqual(len(api.created), 2)
        self.assertEqual(api.created[-1]["parents"], ["d" * 40])

    def test_permission_failure_not_retried(self):
        api = self.published()
        api.denied = True
        with self.assertRaises(subprocess.CalledProcessError):
            version.advance_main(api, "v0.1.1")
        self.assertEqual(len(api.created), 1)
        self.assertEqual(api.main, MAIN)

    def test_manual_minor_advance_preserved(self):
        api = self.published()
        api.files[MAIN] = {"Cargo.toml": CARGO.replace("0.1.1", "0.2.0"),
                           "Cargo.lock": LOCK.replace("0.1.1", "0.2.0", 1)}
        version.advance_main(api, "v0.1.1")
        self.assertEqual(api.mutations, [])

    def test_unpublished_prerelease_or_rehearsal_cannot_advance(self):
        for key, value in (("draft", True), ("prerelease", True), ("published_at", None)):
            api = self.published()
            api.release[key] = value
            with self.assertRaises(ValueError):
                version.advance_main(api, "v0.1.1")
            self.assertEqual(api.mutations, [])
        api = self.published()
        api.report["official"] = False
        with self.assertRaises(ValueError):
            version.advance_main(api, "v0.1.1")
        self.assertEqual(api.mutations, [])

    def test_unrelated_main_and_tampered_assets_refused(self):
        for mutation in (lambda a: a.report.update(source_commit=MAIN),
                         lambda a: a.release["assets"][0].update(digest="sha256:" + "d" * 64),
                         lambda a: setattr(a, "relation", "diverged"),
                         lambda a: a.files[MAIN].update({"Cargo.lock": LOCK.replace("0.1.1", "0.1.2", 1)})):
            api = self.published()
            mutation(api)
            with self.assertRaises(ValueError):
                version.advance_main(api, "v0.1.1")
            self.assertEqual(api.mutations, [])

    def test_preparation_rejects_lock_mismatch_and_unstable_version(self):
        for path, value in (("Cargo.lock", LOCK.replace("0.1.1", "0.1.2", 1)),
                            ("Cargo.toml", CARGO.replace("0.1.1", "0.1.1-dev", 1))):
            api = FakeGitHub()
            api.files[MAIN][path] = value
            with self.assertRaises(ValueError):
                version.ensure_tag(api, MAIN)
            self.assertEqual(api.mutations, [])

    def test_tag_lookup_permission_failure_does_not_create_tag(self):
        api = FakeGitHub()
        original = api.api
        def forbidden(route, payload=None, method="POST"):
            if route.startswith("git/ref/tags/"):
                raise subprocess.CalledProcessError(1, ["gh"], stderr="Forbidden (HTTP 403)")
            return original(route, payload, method)
        api.api = forbidden
        with self.assertRaises(subprocess.CalledProcessError):
            version.ensure_tag(api, MAIN)
        self.assertEqual(api.mutations, [])

    def test_unfamiliar_version_format_refused(self):
        for value in ("1.2.3-dev", "01.2.3", "1.2", "1.2.3\n"):
            with self.assertRaises(ValueError):
                version.stable_version(value)


if __name__ == "__main__":
    unittest.main()
