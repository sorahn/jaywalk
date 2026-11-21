#!/usr/bin/env python3
"""Build and deliver explicitly tagged releases without editing versions or refs."""
import argparse
import base64
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib
from urllib.parse import quote

spec = importlib.util.spec_from_file_location("release_version", Path(__file__).with_name("release-version.py"))
version = importlib.util.module_from_spec(spec)
spec.loader.exec_module(version)
metadata_spec = importlib.util.spec_from_file_location("release_metadata", Path(__file__).with_name("release-macos.py"))
metadata = importlib.util.module_from_spec(metadata_spec)
metadata_spec.loader.exec_module(metadata)


def optional(call):
    try:
        return call()
    except subprocess.CalledProcessError as error:
        if "(HTTP 404)" not in (error.stderr or ""):
            raise
        return None


def select(api, tag):
    """Select only the explicitly supplied immutable tag; never mutate main/refs."""
    version.stable_version(tag.removeprefix("v"))
    if not tag.startswith("v"):
        raise ValueError("expected a v-prefixed version tag")
    source = version.tag_commit(api, tag)
    verify_source(api, tag, source)
    cargo = version.content(api, "Cargo.toml", source)
    target = version.package_version(cargo)
    lock = version.content(api, "Cargo.lock", source)
    name = tomllib.loads(cargo)["package"]["name"]
    if tag != "v" + target or version.root_lock_version(lock, name) != target:
        raise ValueError("tag, Cargo.toml and root lockfile versions must agree")
    existing = candidate(api, tag)
    return {"tag": tag, "source": source, "build": "true" if existing is None else "false"}


def candidate(api, tag):
    release = optional(lambda: api.api("releases/tags/" + quote(tag, safe="")))
    if release is None:
        return None
    source = version.tag_commit(api, tag)
    target = version.package_version(version.content(api, "Cargo.toml", source))
    if tag != "v" + target:
        raise ValueError("candidate tag and source version disagree")
    required = {"manifest.json", "appcast.xml", "jaywalk.rb",
                f"Jaywalk-{target}-macos-arm64.zip", f"Jaywalk-{target}-macos-arm64.dmg"}
    names = [asset["name"] for asset in release["assets"]]
    if any(names.count(name) != 1 for name in required):
        if release["draft"]:
            return None  # A failed upload can be rebuilt without moving its tag.
        raise ValueError("published release assets are incomplete")
    report = api.manifest(release)
    version.verify_artifacts(release, report, tag, source, target)
    return release, report, source


def verify_source(api, tag, source):
    if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", tag) or not re.fullmatch(r"[0-9a-f]{40}", source):
        raise ValueError("invalid release identity")
    if version.tag_commit(api, tag) != source:
        raise ValueError("release tag does not identify the selected source")
    main = api.api("git/ref/heads/main")["object"]["sha"]
    if main != source and api.api(f"compare/{source}...{main}")["status"] not in ("ahead", "identical"):
        raise ValueError("release source is not on main")


def update_tap(tap, report):
    generated = metadata.cask(report)
    path = "contents/Casks/jaywalk.rb"
    for _ in range(3):
        existing = optional(lambda: tap.api(path))
        if existing:
            current = base64.b64decode(existing["content"]).decode()
            match = re.search(r'^  version "([0-9]+\.[0-9]+\.[0-9]+)"$', current, re.M)
            if not match:
                raise ValueError("tap cask has an unrecognized version; refusing to overwrite it")
            if version.stable_version(match[1]) > version.stable_version(report["version"]):
                return "tap already has a newer release"
            if current == generated:
                return "tap is current"
            if match[1] == report["version"]:
                raise ValueError("tap already records different metadata for this release")
        payload = {"message": "update jaywalk to " + report["version"],
                   "content": base64.b64encode(generated.encode()).decode()}
        if existing:
            payload["sha"] = existing["sha"]
        try:
            tap.api(path, payload, method="PUT")
            return "updated tap to " + report["version"]
        except subprocess.CalledProcessError:
            latest = optional(lambda: tap.api(path))
            if (latest or {}).get("sha") == (existing or {}).get("sha"):
                raise
    raise RuntimeError("tap changed repeatedly; rerun release completion")


def finish(api, tap, tag):
    existing = candidate(api, tag)
    if existing is None:
        raise ValueError("published release is missing")
    release, report, source = existing
    verify_source(api, tag, source)
    version.verify_release(release, report, tag, source, tag[1:])
    print(update_tap(tap, report))


def create_draft(api, tag, directory):
    report = json.loads((directory / "manifest.json").read_text())
    source = version.tag_commit(api, tag)
    verify_source(api, tag, source)
    if report.get("source_commit") != source or report.get("version") != tag[1:] or report.get("official") is not True:
        raise ValueError("draft assets do not identify the official tagged source")
    existing = optional(lambda: api.api("releases/tags/" + quote(tag, safe="")))
    if existing:
        if not existing["draft"]:
            raise ValueError("never replace a public release")
        api.api("releases/" + str(existing["id"]), method="DELETE")
    subprocess.run(["gh", "release", "create", tag, *map(str, sorted(directory.iterdir())),
                    "--repo", api.repository, "--draft", "--verify-tag", "--generate-notes"], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    selection = sub.add_parser("select")
    selection.add_argument("tag")
    draft = sub.add_parser("draft")
    draft.add_argument("tag")
    draft.add_argument("directory", type=Path)
    completion = sub.add_parser("finish")
    completion.add_argument("tag")
    completion.add_argument("--tap", default="sorahn/homebrew-tap")
    args = parser.parse_args()
    api = version.GitHub(os.environ["GITHUB_REPOSITORY"])
    if args.action == "select":
        result = select(api, args.tag)
        print(json.dumps(result))
        if os.environ.get("GITHUB_OUTPUT"):
            with open(os.environ["GITHUB_OUTPUT"], "a") as output:
                output.writelines(f"{key}={value}\n" for key, value in result.items())
    elif args.action == "draft":
        create_draft(api, args.tag, args.directory)
    else:
        finish(api, version.GitHub(args.tap), args.tag)


if __name__ == "__main__":
    main()
