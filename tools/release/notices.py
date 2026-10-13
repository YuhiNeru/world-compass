"""Collect declared licenses and upstream notice texts from installed dependencies."""
from pathlib import Path
import json
import subprocess

root = Path(__file__).resolve().parents[2]
rows, texts = [], []
for path in (root / 'node_modules').rglob('package.json'):
    if 'node_modules' not in str(path.parent):
        continue
    try:
        p = json.loads(path.read_text(encoding='utf-8'))
    except (ValueError, OSError):
        continue
    if not p.get('name') or not p.get('version'):
        continue
    rows.append(dict(ecosystem='npm', name=p['name'], version=p['version'], license=p.get('license', 'UNDECLARED')))
    for f in path.parent.iterdir():
        if f.is_file() and f.name.lower().startswith(('license', 'copying', 'notice')):
            texts.append(f"\n===== npm {p['name']} {p['version']} / {f.name} =====\n" + f.read_text(encoding='utf-8', errors='replace'))

result = subprocess.run(['cargo', 'metadata', '--manifest-path', str(root / 'apps/desktop/src-tauri/Cargo.toml'), '--offline', '--filter-platform', 'x86_64-pc-windows-msvc', '--format-version', '1'], check=True, capture_output=True)
for p in json.loads(result.stdout)['packages']:
    if p['name'] == 'world-compass':
        continue
    rows.append(dict(ecosystem='cargo', name=p['name'], version=p['version'], license=p.get('license') or 'UNDECLARED'))
    directory = Path(p['manifest_path']).parent
    for f in directory.iterdir():
        if f.is_file() and (f.name.lower().startswith(('license', 'copying', 'notice')) or f.name == p.get('license_file')):
            texts.append(f"\n===== cargo {p['name']} {p['version']} / {f.name} =====\n" + f.read_text(encoding='utf-8', errors='replace'))

out = root / 'resources/legal'
out.mkdir(parents=True, exist_ok=True)
(out / 'dependencies.json').write_text(json.dumps(rows, ensure_ascii=False, indent=2), encoding='utf-8')
(out / 'THIRD-PARTY-NOTICES.txt').write_text('World Compass development dependency license inventory. Includes development and platform-specific dependencies, not all of which ship in Windows binaries.\n' + '\n'.join(texts), encoding='utf-8')
print(f'Collected {len(rows)} dependency declarations and {len(texts)} upstream texts')
