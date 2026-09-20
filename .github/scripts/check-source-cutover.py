#!/usr/bin/env python3
"""Keep public products closed and historical inputs outside executable routing."""
from pathlib import Path
import json
import tomllib
import subprocess
import hashlib
import re
ROOT=Path(__file__).resolve().parents[2]
products={'devbox-workspace','devbox-api-studio','devbox-knowledge','devbox-control-center'}
legacy=json.loads((ROOT/'apps/legacy-v0.7-catalog.json').read_text())
assert {p.name for p in (ROOT/'apps').iterdir() if p.is_dir()} == products
assert {p['id'] for p in json.loads((ROOT/'apps/catalog.json').read_text())['apps']} == products
members=tomllib.loads((ROOT/'Cargo.toml').read_text())['workspace']['members']
assert not any(f"apps/{app['id']}/" in str(members) for app in legacy['apps'])
engines=list((ROOT/'crates').glob('*-engine'))+[ROOT/'crates/webhook-host',ROOT/'crates/installation-tools']
assert len(engines)==14
for engine in engines:
    assert not any((engine/name).exists() for name in ['tauri.conf.json','build.rs','src/main.rs','capabilities','icons'])
    manifest=tomllib.loads((engine/'Cargo.toml').read_text())
    assert manifest['lib']['crate-type']==['rlib']
    assert 'standalone' not in manifest.get('features',{})
# Native product authority must not call the former executable router. Metadata-only
# legacy readers live in migration/installation engines; Suite has its own identity checks.
for product in products:
    for path in (ROOT/'apps'/product/'src-tauri/src').rglob('*.rs'):
        text=path.read_text()
        assert 'devbox_launch::launch' not in text, path
launch=(ROOT/'crates/launch/src/lib.rs').read_text()
assert launch.count('refuse_retired_product(app_id)?;')==3
for app in legacy['apps']: assert '"'+app['id']+'"' in launch
# Embedded reviewed bytes must survive Windows autocrlf checkout unchanged.
lock_path = 'crates/editor-engine/src/lsp/node-lock.json'
attribute = subprocess.check_output(['git', 'check-attr', 'eol', '--', lock_path], cwd=ROOT, text=True).strip()
assert attribute == f'{lock_path}: eol: lf', 'reviewed Node lock requires checkout LF identity'
expected = re.search(r'REVIEWED_NODE_LOCK_SHA256: &str =\s*"([a-f0-9]{64})"', (ROOT/'crates/editor-engine/src/lsp/node_lock.rs').read_text()).group(1)
assert hashlib.sha256((ROOT/lock_path).read_bytes()).hexdigest() == expected
print('Source cutover: four products, 14 library engines, no legacy route fallback')
