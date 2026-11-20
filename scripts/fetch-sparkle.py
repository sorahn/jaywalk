#!/usr/bin/env python3
"""Fetch the pinned public SDK. No updater traffic occurs inside the app."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parent.parent

def main():
    pin = json.loads((ROOT / "packaging/macos/sparkle.json").read_text())
    destination = Path(sys.argv[1] if len(sys.argv) > 1 else ROOT / "target/sparkle").resolve()
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=destination) as temporary:
        archive = Path(temporary) / "sdk.tar.xz"
        with urllib.request.urlopen(pin["url"], timeout=120) as response, archive.open("wb") as output:
            while data := response.read(1024 * 1024):
                output.write(data)
        if hashlib.file_digest(archive.open("rb"), "sha256").hexdigest() != pin["sha256"]:
            raise SystemExit("Sparkle SDK checksum mismatch")
        subprocess.run(["tar", "-xf", str(archive), "-C", str(destination)], check=True)
    print(destination / "Sparkle.framework")

if __name__ == "__main__":
    main()
