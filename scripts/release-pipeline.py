#!/usr/bin/env python3
"""Allocate calendar builds from successful main checks, then deliver verified releases."""
import argparse
import base64
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
from datetime import datetime, timezone
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


def successful_checks(api, identifier):
    if not re.fullmatch(r"[0-9]+", str(identifier)):
        raise ValueError("invalid checks run identifier")
    run = api.api("actions/runs/" + str(identifier))
    if (str(run["id"]) != str(identifier) or run["event"] != "push"
            or run["head_branch"] != "main" or run["path"] != ".github/workflows/checks.yml"
            or run["status"] != "completed" or run["conclusion"] != "success"
            or run["head_repository"]["full_name"] != api.repository
            or not re.fullmatch(r"[0-9a-f]{40}", run["head_sha"])):
        raise ValueError("release requires successful main-push checks for this repository")
    verify_main_source(api, run["head_sha"])
    return run


def allocation(api, tag):
    version.stable_version(tag.removeprefix("v"))
    if not tag.startswith("v"):
        raise ValueError("expected a v-prefixed calendar version tag")
    obj = api.api("git/ref/tags/" + quote(tag, safe=""))["object"]
    if obj["type"] != "tag":
        raise ValueError("automatic release requires its annotated allocation tag")
    annotation = api.api("git/tags/" + obj["sha"])
    record = json.loads(annotation["message"])
    if (record.get("schema") != 1 or record.get("version") != tag[1:]
            or annotation["tag"] != tag or annotation["object"]["type"] != "commit"
            or annotation["object"]["sha"] != record.get("source_commit")):
        raise ValueError("release allocation does not match its immutable tag")
    if successful_checks(api, record.get("checks_run_id"))["head_sha"] != record["source_commit"]:
        raise ValueError("release allocation checks identify a different source")
    return record


def tags(api):
    result = []
    page = 1
    while True:
        batch = api.api(f"tags?per_page=100&page={page}")
        result.extend(batch)
        if len(batch) < 100:
            return result
        page += 1


def reserve(api, check_id=None, now=None):
    if check_id is None:
        source = api.api("git/ref/heads/main")["object"]["sha"]
        runs = api.api(f"actions/workflows/checks.yml/runs?head_sha={source}&event=push&branch=main&status=success&per_page=1")["workflow_runs"]
        if not runs:
            raise ValueError("current main has no successful checks; wait or rerun its checks")
        check_id = runs[0]["id"]
    run = successful_checks(api, check_id)
    source = run["head_sha"]
    clock = now or datetime.now(timezone.utc)
    for _ in range(10):
        existing = tags(api)
        calendar = []
        for entry in existing:
            if not entry["name"].startswith("v"):
                continue
            try:
                numeric = version.stable_version(entry["name"][1:])
            except ValueError:
                continue  # Legacy semver and unrelated tags are not allocations.
            calendar.append((numeric, entry))
        matching = [entry for _, entry in calendar if entry["commit"]["sha"] == source]
        if len(matching) > 1:
            raise ValueError("source already has multiple calendar tags; inspect release history")
        if matching:
            return select(api, matching[0]["name"])
        # Reserved/failed numbers count too. Avoid channel regression if an older
        # checks run finishes later or the month changes while runs are queued.
        month = max([(clock.year, clock.month), *(n[:2] for n, _ in calendar)])
        number = max([n[2] for n, _ in calendar if n[:2] == month], default=0) + 1
        target = f"{month[0]}.{month[1]}.{number}"
        tag = "v" + target
        record = {"schema": 1, "version": target, "source_commit": source,
                  "checks_run_id": str(run["id"])}
        obj = api.api("git/tags", {"tag": tag, "message": json.dumps(record, sort_keys=True),
                                  "object": source, "type": "commit"})
        try:
            api.api("git/refs", {"ref": "refs/tags/" + tag, "sha": obj["sha"]})
        except subprocess.CalledProcessError as error:
            # Only a verified ref collision is retryable; auth/network failures
            # must not be hidden or manufacture another allocation.
            if not any(code in (error.stderr or "") for code in ("(HTTP 409)", "(HTTP 422)")):
                raise
            competing_source = optional(lambda: version.tag_commit(api, tag))
            if competing_source is None:
                raise
            if competing_source == source:
                return select(api, tag)
            continue
        return select(api, tag)
    raise RuntimeError("release tags changed repeatedly; rerun the same checks run")


def select(api, tag):
    record = allocation(api, tag)
    existing = candidate(api, tag)
    return {"tag": tag, "source": record["source_commit"], "version": record["version"],
            "checks": record["checks_run_id"], "build": "true" if existing is None else "false"}


def find_release(api, tag):
    # GitHub's by-tag endpoint only returns published releases. Drafts must be
    # discovered through an authenticated listing, then fetched by release ID.
    release = optional(lambda: api.api("releases/tags/" + quote(tag, safe="")))
    if release is not None:
        return release
    matches = []
    page = 1
    while True:
        batch = api.api(f"releases?per_page=100&page={page}")
        matches.extend(entry for entry in batch if entry["tag_name"] == tag)
        if len(batch) < 100:
            break
        page += 1
    if len(matches) > 1:
        raise ValueError("multiple releases use the selected tag; inspect draft history")
    if not matches:
        return None
    release = api.api(f"releases/{matches[0]['id']}")
    if release["tag_name"] != tag:
        raise ValueError("selected release tag changed during discovery")
    return release


def candidate(api, tag):
    release = find_release(api, tag)
    if release is None:
        return None
    record = allocation(api, tag)
    source, target = record["source_commit"], record["version"]
    required = {"manifest.json", "appcast.xml", "jaywalk.rb",
                f"Jaywalk-{target}-macos-arm64.zip", f"Jaywalk-{target}-macos-arm64.dmg"}
    names = [asset["name"] for asset in release["assets"]]
    if any(names.count(name) != 1 for name in required):
        if release["draft"]:
            return None  # A failed upload can be rebuilt without moving its tag.
        raise ValueError("published release assets are incomplete")
    report = api.manifest(release)
    version.verify_artifacts(release, report, tag, source, target)
    if report.get("checks_run_id") != record["checks_run_id"] or report.get("release_tag") != tag:
        raise ValueError("candidate manifest does not match the recorded release allocation")
    return release, report, source


def verify_main_source(api, source):
    if not re.fullmatch(r"[0-9a-f]{40}", source):
        raise ValueError("invalid source commit")
    main = api.api("git/ref/heads/main")["object"]["sha"]
    if main != source and api.api(f"compare/{source}...{main}")["status"] not in ("ahead", "identical"):
        raise ValueError("release source is not on main")


def verify_source(api, tag, source):
    record = allocation(api, tag)
    if record["source_commit"] != source:
        raise ValueError("release tag does not identify the selected source")
    return record


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
    latest = api.api("releases/latest")
    if latest["tag_name"] != tag:
        print("Historical release verified; leaving the current Homebrew channel unchanged")
        return
    print(update_tap(tap, report))


def create_draft(api, tag, directory):
    report = json.loads((directory / "manifest.json").read_text())
    record = allocation(api, tag)
    source = record["source_commit"]
    if (report.get("source_commit") != source or report.get("version") != record["version"]
            or report.get("checks_run_id") != record["checks_run_id"]
            or report.get("release_tag") != tag or report.get("official") is not True):
        raise ValueError("draft assets do not identify the official tagged source")
    existing = find_release(api, tag)
    if existing:
        if not existing["draft"]:
            raise ValueError("never replace a public release")
        api.api("releases/" + str(existing["id"]), method="DELETE")
    subprocess.run(["gh", "release", "create", tag, *map(str, sorted(directory.iterdir())),
                    "--repo", api.repository, "--draft", "--verify-tag", "--generate-notes"], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    reservation = sub.add_parser("reserve")
    reservation.add_argument("--checks")
    validation = sub.add_parser("check-source")
    validation.add_argument("tag")
    validation.add_argument("source")
    validation.add_argument("checks")
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
    if args.action in ("select", "reserve"):
        result = select(api, args.tag) if args.action == "select" else reserve(api, args.checks)
        print(json.dumps(result))
        if os.environ.get("GITHUB_OUTPUT"):
            with open(os.environ["GITHUB_OUTPUT"], "a") as output:
                output.writelines(f"{key}={value}\n" for key, value in result.items())
    elif args.action == "check-source":
        record = verify_source(api, args.tag, args.source)
        if record["checks_run_id"] != args.checks:
            raise ValueError("checks do not match the immutable release allocation")
    elif args.action == "draft":
        create_draft(api, args.tag, args.directory)
    else:
        finish(api, version.GitHub(args.tap), args.tag)


if __name__ == "__main__":
    main()
