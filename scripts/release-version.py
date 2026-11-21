#!/usr/bin/env python3
"""Create immutable release tags and advance development versions through GitHub.

Mutations use the job's GH_TOKEN. Rehearsals never invoke this script.
"""
import argparse
import base64
import json
import os
import re
import subprocess
import tomllib
from urllib.parse import quote


class GitHub:
    def __init__(self, repository):
        if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
            raise ValueError("invalid repository")
        self.repository = repository

    def api(self, route, payload=None, method="POST"):
        args = ["gh", "api", f"repos/{self.repository}/{route}"]
        if payload is not None:
            args += ["--method", method, "--input", "-"]
        result = subprocess.run(args, input=json.dumps(payload) if payload is not None else None,
                                capture_output=True, text=True, check=True)
        return json.loads(result.stdout)

    def manifest(self, release):
        assets = [a for a in release["assets"] if a["name"] == "manifest.json"]
        if len(assets) != 1:
            raise ValueError("release must contain exactly one manifest.json")
        result = subprocess.run(["gh", "api", "-H", "Accept: application/octet-stream",
                                 f"repos/{self.repository}/releases/assets/{assets[0]['id']}"],
                                capture_output=True, text=True, check=True)
        return json.loads(result.stdout)


def stable_version(value):
    if not re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", value):
        raise ValueError("automatic releases require a stable major.minor.patch version")
    return tuple(map(int, value.split(".")))


def package_version(cargo):
    value = tomllib.loads(cargo)["package"]["version"]
    stable_version(value)
    return value


def content(api, path, ref):
    data = api.api(f"contents/{path}?ref={quote(ref, safe='')}")
    return base64.b64decode(data["content"], validate=False).decode("utf-8")


def root_lock_version(lock, name):
    entries = [p for p in tomllib.loads(lock)["package"]
               if p["name"] == name and "source" not in p]
    if len(entries) != 1:
        raise ValueError("lockfile must contain exactly one local root package")
    return entries[0]["version"]


def advance_files(cargo, lock, expected, upcoming):
    data = tomllib.loads(cargo)["package"]
    if data["version"] != expected or root_lock_version(lock, data["name"]) != expected:
        raise ValueError("Cargo.toml and root lockfile version must match the release")
    # Limit edits to the package table and the matching local lockfile entry.
    # Parse both documents again afterward, rejecting unfamiliar formatting.
    def replace_version(block):
        updated, count = re.subn(r'(?m)^(version\s*=\s*)"' + re.escape(expected) + r'"',
                                 lambda m: m[1] + '"' + upcoming + '"', block)
        if count != 1:
            raise ValueError("expected exactly one version assignment")
        return updated

    sections = re.split(r"(?m)(?=^\[)", cargo)
    for i, block in enumerate(sections):
        if block.startswith("[package]\n"):
            sections[i] = replace_version(block)
    cargo = "".join(sections)
    sections = re.split(r"(?m)(?=^\[\[package\]\])", lock)
    for i, block in enumerate(sections):
        if block.startswith("[[package]]"):
            entry = tomllib.loads(block)["package"][0]
            if entry["name"] == data["name"] and "source" not in entry:
                sections[i] = replace_version(block)
    lock = "".join(sections)
    if package_version(cargo) != upcoming or root_lock_version(lock, data["name"]) != upcoming:
        raise ValueError("version edit did not produce the expected package/lock versions")
    return cargo, lock


def tag_commit(api, tag):
    obj = api.api(f"git/ref/tags/{quote(tag, safe='')}")["object"]
    for _ in range(5):
        if obj["type"] == "commit":
            return obj["sha"]
        if obj["type"] != "tag":
            break
        obj = api.api(f"git/tags/{obj['sha']}")["object"]
    raise ValueError("tag does not resolve to a commit")


def ensure_tag(api, source):
    # Pin the dispatched, approved main commit; never tag a moving branch name.
    if not re.fullmatch(r"[0-9a-f]{40}", source):
        raise ValueError("source must be a full commit ID")
    main = api.api("git/ref/heads/main")["object"]["sha"]
    if source != main:
        raise ValueError("main advanced; review and dispatch the new main tip")
    cargo = content(api, "Cargo.toml", source)
    version = package_version(cargo)
    lock = content(api, "Cargo.lock", source)
    if root_lock_version(lock, tomllib.loads(cargo)["package"]["name"]) != version:
        raise ValueError("package and lockfile versions differ")
    tag = "v" + version
    try:
        existing = tag_commit(api, tag)
    except subprocess.CalledProcessError as error:
        # A missing ref is the only acceptable failure; don't mask auth/network errors.
        if "(HTTP 404)" not in error.stderr:
            raise
    else:
        if existing != source:
            raise ValueError("release tag already identifies different source; never retag")
        return tag
    obj = api.api("git/tags", {"tag": tag, "message": f"release {version}",
                              "object": source, "type": "commit"})
    try:
        api.api("git/refs", {"ref": "refs/tags/" + tag, "sha": obj["sha"]})
    except subprocess.CalledProcessError:
        # Another job may have created the ref; accept only the identical source.
        if tag_commit(api, tag) != source:
            raise ValueError("concurrent release tag identifies different source")
    return tag


def verify_release(release, manifest, tag, source, version):
    if release["draft"] or release["prerelease"] or not release.get("published_at"):
        raise ValueError("only published stable releases advance main")
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


def advance_main(api, tag):
    if not tag.startswith("v"):
        raise ValueError("expected a version tag")
    released = tag[1:]
    major, minor, patch = stable_version(released)
    upcoming = f"{major}.{minor}.{patch + 1}"
    release = api.api("releases/tags/" + quote(tag, safe=""))
    source = tag_commit(api, tag)
    if package_version(content(api, "Cargo.toml", source)) != released:
        raise ValueError("tag and source package version differ")
    verify_release(release, api.manifest(release), tag, source, released)
    for _ in range(3):
        main = api.api("git/ref/heads/main")["object"]["sha"]
        relation = api.api(f"compare/{source}...{main}")["status"]
        if relation not in ("ahead", "identical"):
            raise ValueError("released source is not an ancestor of main")
        cargo = content(api, "Cargo.toml", main)
        lock = content(api, "Cargo.lock", main)
        current = package_version(cargo)
        if root_lock_version(lock, tomllib.loads(cargo)["package"]["name"]) != current:
            raise ValueError("main package and lockfile versions differ")
        if stable_version(current) > stable_version(released):
            return "main already targets a newer version; no change"
        if current != released:
            raise ValueError("main is older than the released version")
        cargo, lock = advance_files(cargo, lock, released, upcoming)
        parent = api.api("git/commits/" + main)
        tree = api.api("git/trees", {"base_tree": parent["tree"]["sha"], "tree": [
            {"path": path, "mode": "100644", "type": "blob", "content": value}
            for path, value in (("Cargo.toml", cargo), ("Cargo.lock", lock))]})
        commit = api.api("git/commits", {"message": f"start {upcoming} development",
                                        "tree": tree["sha"], "parents": [main]})
        try:
            api.api("git/refs/heads/main", {"sha": commit["sha"], "force": False}, method="PATCH")
            return f"advanced main to {upcoming} at {commit['sha']}"
        except subprocess.CalledProcessError:
            if api.api("git/ref/heads/main")["object"]["sha"] == main:
                raise  # Permission or validation failure, not a race.
    raise RuntimeError("main kept changing; rerun advancement for the same release tag")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("prepare", "advance"))
    parser.add_argument("value", help="approved main SHA or published version tag")
    args = parser.parse_args()
    api = GitHub(os.environ["GITHUB_REPOSITORY"])
    result = ensure_tag(api, args.value) if args.action == "prepare" else advance_main(api, args.value)
    print(result)
    if args.action == "prepare" and os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            output.write(f"tag={result}\n")


if __name__ == "__main__":
    main()
