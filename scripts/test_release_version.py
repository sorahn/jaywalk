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
CARGO = '[package]\nname = "jaywalk"\nversion = "2026.11.1" # keep comment\n\n[dependencies]\nother = "2026.11.1"\n'
LOCK = 'version = 4\n\n[[package]]\nname = "jaywalk"\nversion = "2026.11.1"\ndependencies = ["other"]\n\n[[package]]\nname = "other"\nversion = "2026.11.1"\nsource = "registry+https://example.test"\n'


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
        self.report = {"official": True, "source_tree_clean": True, "version": "2026.11.1",
                       "source_commit": SOURCE, "binary_revision": SOURCE[:12],
                       "platform": "macos", "architecture": "arm64", "artifacts": [
                           {"filename": f"Jaywalk-2026.11.1-macos-arm64{suffix}", "sha256": "c" * 64, "bytes": 123}
                           for suffix in (".zip", ".dmg")]}
        self.release = {"draft": False, "prerelease": False, "published_at": "2026-11-20T12:00:00Z",
                        "tag_name": "v2026.11.1", "assets": [
                            {"name": f["filename"], "digest": "sha256:" + f["sha256"], "size": f["bytes"]}
                            for f in self.report["artifacts"]] + [{"name": n} for n in ("manifest.json", "appcast.xml", "jaywalk.rb")]}

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
        if route == "git/ref/tags/v2026.11.1":
            if self.tag is None:
                raise missing()
            return {"object": {"type": "tag", "sha": "tag-object"}}
        if route == "git/tags/tag-object":
            return {"object": {"type": "commit", "sha": self.tag}}
        if route.startswith("releases/tags/"):
            return self.release
        if route.startswith("compare/"):
            return {"status": self.relation}
        raise AssertionError(route)


class VersionTests(unittest.TestCase):
    def test_calendar_version_order(self):
        self.assertLess(version.stable_version("2026.11.9"), version.stable_version("2026.11.10"))
        self.assertLess(version.stable_version("2026.12.10"), version.stable_version("2027.1.1"))

    def test_invalid_calendar_versions(self):
        for value in ("0.1.1", "2026.0.1", "2026.13.1", "2026.11.0", "2026.01.1", "2026.11.1-dev", "2026.11.1\n"):
            with self.assertRaises(ValueError):
                version.stable_version(value)

    def test_annotated_tag_resolves_to_exact_commit(self):
        api = FakeGitHub()
        api.tag = SOURCE
        self.assertEqual(version.tag_commit(api, "v2026.11.1"), SOURCE)
        self.assertEqual(api.mutations, [])

    def test_artifact_digest_and_source_are_checked(self):
        for field, value in (("source_commit", MAIN), ("official", False)):
            api = FakeGitHub()
            api.report[field] = value
            with self.assertRaises(ValueError):
                version.verify_artifacts(api.release, api.report, "v2026.11.1", SOURCE, "2026.11.1")
        api = FakeGitHub()
        api.release["assets"][0]["digest"] = "sha256:" + "d" * 64
        with self.assertRaises(ValueError):
            version.verify_artifacts(api.release, api.report, "v2026.11.1", SOURCE, "2026.11.1")


if __name__ == "__main__":
    unittest.main()
