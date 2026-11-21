#!/usr/bin/env python3
"""Stage verified macOS release metadata; never publishes or reads credentials."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import tempfile

ROOT = Path(os.environ.get("JAYWALK_RELEASE_SOURCE", Path(__file__).resolve().parent.parent)).resolve()

def run(*args):
    env = os.environ.copy()
    xcode = Path("/Applications/Xcode.app/Contents/Developer")
    if xcode.is_dir():
        env.setdefault("DEVELOPER_DIR", str(xcode))
    result = subprocess.run(args, text=True, capture_output=True, env=env, cwd=ROOT)
    if result.returncode:
        raise RuntimeError(f"{args[0]} failed: {result.stderr.strip()}")
    return result.stdout.strip()

def version():
    return os.environ.get("JAYWALK_RELEASE_VERSION") or "0.0.0"

def validate_tag(tag):
    if tag != "v" + version():
        raise ValueError("release tag must equal v plus the injected application version")

def checksum(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()

def verify_bundle_architecture(bundle):
    # Check actual Mach-O bytes, including framework/XPC helpers, before packaging
    # or reporting ARM64 metadata. Universal helpers are fine when ARM is present.
    magic = {bytes.fromhex(h) for h in (
        "feedface", "cefaedfe", "feedfacf", "cffaedfe",
        "cafebabe", "bebafeca", "cafebabf", "bfbafeca",
    )}
    for name in ("jaywalk", "jw"):
        path = bundle / "Contents/MacOS" / name
        if not path.is_file() or run("lipo", "-archs", str(path)) != "arm64":
            raise ValueError(f"this release path requires an arm64 {name} executable")
    for path in sorted(bundle.rglob("*")):
        if path.is_symlink() or not path.is_file():
            continue
        with path.open("rb") as stream:
            if stream.read(4) not in magic:
                continue
        architectures = run("lipo", "-archs", str(path)).split()
        if not {"arm64", "arm64e"}.intersection(architectures):
            raise ValueError(f"bundle contains an Intel-only executable: {path.relative_to(bundle)}")

def manifest(bundle, archive, dmg, official):
    verify_bundle_architecture(bundle)
    with (bundle / "Contents/Info.plist").open("rb") as stream:
        info = plistlib.load(stream)
    if info["CFBundleIdentifier"] != "dev.jaywalk.Jaywalk":
        raise ValueError("unexpected application identifier")
    if info["CFBundleShortVersionString"] != version() or info["CFBundleVersion"] != version():
        raise ValueError("bundle and Cargo versions differ")
    revision = run(str(bundle / "Contents/MacOS/jaywalk"), "--build-revision")
    source_commit = run("git", "rev-parse", "HEAD")
    if official:
        if revision != source_commit[:12]:
            raise ValueError("binary was not built from the clean release source commit")
        tag = os.environ["JAYWALK_RELEASE_TAG"]
        validate_tag(tag)
        if run("git", "rev-parse", tag + "^{commit}") != source_commit:
            raise ValueError("release tag does not identify the packaged source")
        if run("git", "status", "--porcelain"):
            raise ValueError("official releases require a clean source tree")
        run("codesign", "--verify", "--deep", "--strict", str(bundle))
        details = subprocess.run(["codesign", "-dv", "--verbose=4", str(bundle)], capture_output=True, text=True, check=True).stderr
        if "Authority=Developer ID Application:" not in details or "runtime" not in details:
            raise ValueError("official bundles require Developer ID and hardened runtime")
        run("xcrun", "stapler", "validate", str(bundle))
        run("xcrun", "stapler", "validate", str(dmg))
        run("codesign", "--verify", "--strict", str(dmg))
        run("spctl", "--assess", "--type", "execute", "--verbose=2", str(bundle))
    files = [{"filename": p.name, "sha256": checksum(p), "bytes": p.stat().st_size} for p in (archive, dmg)]
    return {"version": version(), "source_commit": source_commit, "binary_revision": revision, "source_tree_clean": not bool(run("git", "status", "--porcelain")),
            "platform": "macos", "architecture": "arm64", "minimum_os": info["LSMinimumSystemVersion"],
            "official": official, "artifacts": files,
            "release_tag": os.environ.get("JAYWALK_RELEASE_TAG"),
            "checks_run_id": os.environ.get("JAYWALK_RELEASE_CHECKS_RUN")}

def cask(data):
    if not data["official"]:
        raise ValueError("cannot generate a publishable cask from rehearsal artifacts")
    if data["architecture"] != "arm64":
        raise ValueError("only validated arm64 artifacts are supported")
    v = data["version"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", v):
        raise ValueError("unsupported cask version")
    asset = next(a for a in data["artifacts"] if a["filename"].endswith(".dmg"))
    if not re.fullmatch(r"Jaywalk-[0-9.]+-macos-arm64\.dmg", asset["filename"]) or not re.fullmatch(r"[0-9a-f]{64}", asset["sha256"]):
        raise ValueError("invalid artifact metadata")
    return f'''cask "jaywalk" do
  version "{v}"
  sha256 "{asset['sha256']}"

  url "https://github.com/sorahn/jaywalk/releases/download/v#{{version}}/Jaywalk-#{{version}}-macos-arm64.dmg"
  name "Jaywalk"
  desc "Native Git and Jujutsu diff viewer"
  homepage "https://github.com/sorahn/jaywalk"

  auto_updates true
  depends_on arch: :arm64
  depends_on macos: :sequoia

  app "Jaywalk.app"
  binary "#{{appdir}}/Jaywalk.app/Contents/MacOS/jw"
end
'''

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    check = sub.add_parser("check-tag")
    check.add_argument("tag")
    pack = sub.add_parser("package")
    pack.add_argument("bundle", type=Path)
    pack.add_argument("output", type=Path)
    architecture = sub.add_parser("check-architecture")
    architecture.add_argument("bundle", type=Path)
    report = sub.add_parser("manifest")
    report.add_argument("bundle", type=Path)
    report.add_argument("archive", type=Path)
    report.add_argument("dmg", type=Path)
    report.add_argument("--official", action="store_true")
    brew = sub.add_parser("cask")
    brew.add_argument("manifest", type=Path)
    args = parser.parse_args()
    if args.action == "check-tag":
        validate_tag(args.tag)
    elif args.action == "cask":
        print(cask(json.loads(args.manifest.read_text())), end="")
    elif args.action == "check-architecture":
        verify_bundle_architecture(args.bundle.resolve())
    elif args.action == "manifest":
        print(json.dumps(manifest(args.bundle.resolve(), args.archive.resolve(), args.dmg.resolve(), args.official), indent=2))
    else:
        bundle, output = args.bundle.resolve(), args.output.resolve()
        verify_bundle_architecture(bundle)
        output.mkdir(parents=True, exist_ok=True)
        stem = "Jaywalk-" + version() + "-macos-arm64"
        if any((output / (stem + suffix)).exists() for suffix in (".zip", ".dmg")):
            raise ValueError("output already contains this version; choose a fresh artifact directory")
        # Notarize/staple the app BEFORE packaging; notarize/staple DMG BEFORE manifest.
        run("ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", str(bundle), str(output / (stem + ".zip")))
        with tempfile.TemporaryDirectory(prefix="jaywalk-dmg-") as temporary:
            staging = Path(temporary)
            shutil.copytree(bundle, staging / "Jaywalk.app", symlinks=True)
            (staging / "Applications").symlink_to("/Applications")
            run("hdiutil", "create", "-volname", "Jaywalk", "-srcfolder", str(staging), "-format", "UDZO", str(output / (stem + ".dmg")))

if __name__ == "__main__":
    main()
