#!/usr/bin/env python3
"""Shared GitHub release identity and artifact validation."""
import json
import re
import subprocess
from urllib.parse import quote


class GitHub:
    def __init__(self, repository):
        if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
            raise ValueError("invalid repository")
        self.repository = repository

    def api(self, route, payload=None, method=None):
        args = ["gh", "api", f"repos/{self.repository}/{route}"]
        if method is not None or payload is not None:
            args += ["--method", method or "POST"]
        if payload is not None:
            args += ["--input", "-"]
        result = subprocess.run(args, input=json.dumps(payload) if payload is not None else None,
                                capture_output=True, text=True, check=True)
        return json.loads(result.stdout) if result.stdout.strip() else None

    def manifest(self, release):
        assets = [a for a in release["assets"] if a["name"] == "manifest.json"]
        if len(assets) != 1:
            raise ValueError("release must contain exactly one manifest.json")
        result = subprocess.run(["gh", "api", "-H", "Accept: application/octet-stream",
                                 f"repos/{self.repository}/releases/assets/{assets[0]['id']}"],
                                capture_output=True, text=True, check=True)
        return json.loads(result.stdout) if result.stdout.strip() else None


def stable_version(value):
    if not re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", value):
        raise ValueError("release versions must use year.month.release-number")
    year, month, release = map(int, value.split("."))
    if not 2000 <= year <= 9999 or not 1 <= month <= 12 or release < 1:
        raise ValueError("release versions must use year.month.release-number (for example 2026.11.1)")
    return year, month, release


def tag_commit(api, tag):
    obj = api.api(f"git/ref/tags/{quote(tag, safe='')}")["object"]
    for _ in range(5):
        if obj["type"] == "commit":
            return obj["sha"]
        if obj["type"] != "tag":
            break
        obj = api.api(f"git/tags/{obj['sha']}")["object"]
    raise ValueError("tag does not resolve to a commit")


def verify_release(release, manifest, tag, source, version):
    if release["draft"] or release["prerelease"] or not release.get("published_at"):
        raise ValueError("expected a published stable release")
    verify_artifacts(release, manifest, tag, source, version)


def verify_artifacts(release, manifest, tag, source, version):
    if release["prerelease"]:
        raise ValueError("only stable release candidates are supported")
    if release["tag_name"] != tag:
        raise ValueError("release tag mismatch")
    required = {"official": True, "source_tree_clean": True, "version": version,
                "source_commit": source, "binary_revision": source[:12],
                "platform": "macos", "architecture": "arm64"}
    if any(manifest.get(k) != v for k, v in required.items()):
        raise ValueError("release manifest does not identify the clean official tagged build")
    expected = {f"Jaywalk-{version}-macos-arm64{suffix}" for suffix in (".zip", ".dmg")}
    files = manifest.get("artifacts", [])
    if len(files) != 2 or {f["filename"] for f in files} != expected:
        raise ValueError("unexpected release payloads")
    for file in files:
        matching = [a for a in release["assets"] if a["name"] == file["filename"]]
        if len(matching) != 1 or not re.fullmatch(r"[0-9a-f]{64}", file["sha256"]):
            raise ValueError("missing or invalid release asset")
        asset = matching[0]
        if asset.get("digest") != "sha256:" + file["sha256"] or asset["size"] != file["bytes"]:
            raise ValueError("published asset does not match approved manifest")


