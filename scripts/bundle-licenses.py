#!/usr/bin/env python3
"""Collect available dependency notices without recording local source paths."""
import json
from pathlib import Path
import shutil
import subprocess
import sys

root = Path(__file__).resolve().parent.parent
output = Path(sys.argv[1]).resolve()
output.mkdir(parents=True, exist_ok=True)
metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--format-version', '1', '--filter-platform', 'aarch64-apple-darwin'], cwd=root))
used = {node['id'] for node in metadata['resolve']['nodes']}
report = []
for package in sorted(metadata['packages'], key=lambda p: (p['name'], p['version'])):
    if package['id'] not in used or package['name'] == 'jaywalk':
        continue
    source = Path(package['manifest_path']).parent
    candidates = [p for p in source.iterdir() if p.name.upper().startswith(('LICENSE', 'LICENCE', 'NOTICE', 'COPYRIGHT'))]
    if package.get('license_file'):
        candidates.append(source / package['license_file'])
    notices = []
    for notice in candidates:
        if not notice.is_file():
            continue
        destination = output / (package['name'] + '-' + package['version']) / notice.name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(notice, destination)
        notices.append(str(destination.relative_to(output)))
    report.append({'name': package['name'], 'version': package['version'], 'license': package['license'], 'repository': package['repository'], 'notices': sorted(set(notices))})
(output / 'dependencies.json').write_text(json.dumps(report, indent=2) + '\n')
print(f'Collected license metadata for {len(report)} resolved dependencies; review notices before publication.', file=sys.stderr)
