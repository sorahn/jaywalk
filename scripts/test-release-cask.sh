#!/bin/sh
# Credential-free validation of the generated Homebrew cask on a clean CI runner.
set -eu
cd "$(dirname "$0")/.."
workspace=$(mktemp -d)
trap 'rm -rf "$workspace"' EXIT
mkdir "$workspace/Casks"
python3 - "$workspace/Casks/jaywalk.rb" <<'PY'
import importlib.util,sys
from pathlib import Path
spec=importlib.util.spec_from_file_location('metadata',Path('scripts/release-macos.py'))
metadata=importlib.util.module_from_spec(spec);spec.loader.exec_module(metadata)
report={'official':True,'architecture':'arm64','version':'2026.11.1','artifacts':[{'filename':'Jaywalk-2026.11.1-macos-arm64.dmg','sha256':'a'*64}]}
Path(sys.argv[1]).write_text(metadata.cask(report))
PY
brew style "$workspace/Casks/jaywalk.rb"
