#!/usr/bin/env python3
"""Verify downloaded official candidate bytes before making a release public."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import subprocess
import tempfile
import time
import xml.etree.ElementTree as ET
import importlib.util

spec = importlib.util.spec_from_file_location("pipeline", Path(__file__).with_name("release-pipeline.py"))
pipeline = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pipeline)
NS = "{http://www.andymatuschak.org/xml-namespaces/sparkle}"


def child_environment(program):
    env = os.environ.copy()
    env.pop("SPARKLE_PRIVATE_KEY", None)
    if program != "gh":
        env.pop("GH_TOKEN", None)
        env.pop("GITHUB_TOKEN", None)
    return env


def run(args, data=None):
    result = subprocess.run(args, input=data, capture_output=True, env=child_environment(args[0]))
    if result.returncode:
        # Native diagnostics can be useful; secret-bearing stdin is never echoed.
        message = result.stderr.decode(errors="replace").strip()
        raise RuntimeError(f"{args[0]} verification failed: {message}")
    return result.stdout


def enclosure(feed, tag, report, repository):
    matches = []
    for item in ET.parse(feed).findall("./channel/item"):
        entry = item.find("enclosure")
        if entry is not None and (item.findtext(NS + "version") or entry.get(NS + "version")) == tag[1:]:
            matches.append(entry)
    if len(matches) != 1:
        raise ValueError("signed appcast must contain exactly one entry for this version")
    entry = matches[0]
    archive = next(a for a in report["artifacts"] if a["filename"].endswith(".zip"))
    expected = f"https://github.com/{repository}/releases/download/{tag}/{archive['filename']}"
    if entry.get("url") != expected or entry.get("length") != str(archive["bytes"]):
        raise ValueError("appcast enclosure does not match the approved archive")
    signature = entry.get(NS + "edSignature", "")
    if not re.fullmatch(r"[A-Za-z0-9+/]{86}==", signature):
        raise ValueError("invalid archive signature in signed appcast")
    return archive["filename"], signature


def verify_bytes(directory, report):
    for asset in report["artifacts"]:
        path = directory / asset["filename"]
        if path.stat().st_size != asset["bytes"]:
            raise ValueError("downloaded asset size mismatch")
        with path.open("rb") as stream:
            if hashlib.file_digest(stream, "sha256").hexdigest() != asset["sha256"]:
                raise ValueError("downloaded asset checksum mismatch")


def verify_native(directory, report, public, identity):
    if not re.fullmatch(r"[0-9A-Fa-f]{40}", identity):
        raise ValueError("MACOS_SIGNING_IDENTITY must be the intended certificate fingerprint")
    archive = next(a["filename"] for a in report["artifacts"] if a["filename"].endswith(".zip"))
    dmg = next(a["filename"] for a in report["artifacts"] if a["filename"].endswith(".dmg"))
    run(["ditto", "-x", "-k", str(directory / archive), str(directory / "unpacked")])
    bundle = directory / "unpacked/Jaywalk.app"
    run(["codesign", "--verify", "--deep", "--strict", str(bundle)])
    run(["xcrun", "stapler", "validate", str(bundle)])
    run(["spctl", "--assess", "--type", "execute", "--verbose=2", str(bundle)])
    run(["codesign", "--verify", "--strict", str(directory / dmg)])
    run(["xcrun", "stapler", "validate", str(directory / dmg)])
    prefix = str(directory / "signing-cert-")
    run(["codesign", "-d", "--extract-certificates", prefix, str(bundle)])
    actual = hashlib.sha1(Path(prefix + "0").read_bytes()).hexdigest()
    if actual.lower() != identity.lower():
        raise ValueError("candidate was signed by a different Developer ID certificate")
    # The DMG must contain the same sealed app as the update ZIP.
    def cdhash(path):
        details = subprocess.run(["codesign", "-dv", "--verbose=4", str(path)], capture_output=True, check=True, env=child_environment("codesign")).stderr
        match = re.search(rb"(?m)^CDHash=([0-9a-f]+)$", details)
        if not match:
            raise ValueError("candidate is missing its sealed-code identity")
        return match[1]
    mount = directory / "mounted"
    mount.mkdir()
    run(["hdiutil", "attach", "-readonly", "-nobrowse", "-noautoopen", "-mountpoint", str(mount), str(directory / dmg)])
    try:
        installed = mount / "Jaywalk.app"
        if not (mount / "Applications").is_symlink() or os.readlink(mount / "Applications") != "/Applications":
            raise ValueError("DMG is missing its Applications link")
        run(["codesign", "--verify", "--deep", "--strict", str(installed)])
        if cdhash(installed) != cdhash(bundle):
            raise ValueError("DMG and update ZIP contain different sealed applications")
    finally:
        for attempt in range(3):
            detached = subprocess.run(["hdiutil", "detach", str(mount)], capture_output=True, env=child_environment("hdiutil"))
            if detached.returncode == 0:
                break
            time.sleep(1)
        else:
            raise RuntimeError("could not detach the read-only verification image")
    info = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())
    if (info.get("CFBundleIdentifier") != "dev.jaywalk.Jaywalk"
            or info.get("CFBundleVersion") != report["version"]
            or info.get("CFBundleShortVersionString") != report["version"]
            or info.get("SUPublicEDKey") != public
            or info.get("SUFeedURL") != f"https://github.com/{os.environ['GITHUB_REPOSITORY']}/releases/latest/download/appcast.xml"):
        raise ValueError("bundle identity, version or update configuration mismatch")
    for name in ("jaywalk", "jw"):
        executable = str(bundle / "Contents/MacOS" / name)
        if run([executable, "--build-revision"]).decode().strip() != report["source_commit"][:12]:
            raise ValueError("candidate executable provenance mismatch")


def should_be_latest(api, released):
    latest = pipeline.optional(lambda: api.api("releases/latest"))
    return latest is None or pipeline.version.stable_version(released) > pipeline.version.stable_version(latest["tag_name"].removeprefix("v"))


def promote(api, tag, source, sdk, publish=False):
    # This one-shot CLI retains the signing seed only in memory/stdin, rather
    # than propagating it into unrelated native validators or GitHub commands.
    private = os.environ.pop("SPARKLE_PRIVATE_KEY").encode()
    pipeline.verify_source(api, tag, source)
    existing = pipeline.candidate(api, tag)
    if existing is None or existing[2] != source:
        raise ValueError("selected official candidate is missing")
    release, report, _ = existing
    with tempfile.TemporaryDirectory(prefix="jaywalk-promotion-") as temporary:
        directory = Path(temporary)
        names = [a["filename"] for a in report["artifacts"]] + ["appcast.xml", "jaywalk.rb", "manifest.json"]
        command = ["gh", "release", "download", tag, "--repo", api.repository, "--dir", str(directory)]
        for name in names:
            command += ["--pattern", name]
        run(command)
        verify_bytes(directory, report)
        if json.loads((directory / "manifest.json").read_text()) != report:
            raise ValueError("downloaded manifest differs from the validated candidate")
        public = os.environ["SPARKLE_PUBLIC_KEY"]
        run(["xcrun", "swift", str(Path(__file__).with_name("verify-sparkle-key.swift")), public], private)
        verify_native(directory, report, public, os.environ["MACOS_SIGNING_IDENTITY"])
        signer = str(sdk / "bin/sign_update")
        run([signer, "--verify", "--ed-key-file", "-", str(directory / "appcast.xml")], private)
        archive, signature = enclosure(directory / "appcast.xml", tag, report, api.repository)
        run([signer, "--verify", "--ed-key-file", "-", str(directory / archive), signature], private)
        if (directory / "jaywalk.rb").read_text() != pipeline.metadata.cask(report):
            raise ValueError("candidate cask differs from the safe generated cask")
        casks = directory / "Casks"
        casks.mkdir()
        generated = casks / "jaywalk.rb"
        generated.write_text(pipeline.metadata.cask(report))
        run(["brew", "style", str(generated)])
        print("Candidate bytes, notarization, certificate, provenance, update key and feed/archive signatures verified")
    if not publish:
        print("Verification only; no publication was performed")
        return
    if release["draft"]:
        # GITHUB_TOKEN publication does not recursively dispatch release workflows;
        # the parent pipeline explicitly awaits tap delivery.
        newest = should_be_latest(api, report["version"])
        api.api("releases/" + str(release["id"]), {"draft": False, "make_latest": "true" if newest else "false"}, method="PATCH")
        print("Published " + tag)
    else:
        print("Release is already public; continuing idempotent completion")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag")
    parser.add_argument("source")
    parser.add_argument("--sdk", type=Path, required=True)
    parser.add_argument("--publish", action="store_true", help="Publish only after all verification succeeds")
    args = parser.parse_args()
    promote(pipeline.version.GitHub(os.environ["GITHUB_REPOSITORY"]), args.tag, args.source, args.sdk.resolve(), args.publish)


if __name__ == "__main__":
    main()
