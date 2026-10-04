#!/usr/bin/env python3
"""Check planned user-flow ownership without pretending missing runners passed."""
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
matrix = json.loads((ROOT / '.github/scripts/suite-user-flow-matrix.json').read_text())
plan = (ROOT / 'docs/superpowers/plans/2026-10-03-product-readiness/06-acceptance-release.md').read_text()
ids = re.findall(r'^\| ([A-Z]+-\d+) \| R\d+', plan, re.M)
assert set(ids) == {row['id'] for row in matrix}
assert len(ids) == len(matrix) == len(set(ids))
assert all(re.fullmatch(r'windows-[a-z0-9-]+\.mjs', row['module']) for row in matrix)
for product in ('workspace', 'api-studio', 'knowledge', 'control-center'):
    config = json.loads((ROOT / f'apps/devbox-{product}/src-tauri/tauri.conf.json').read_text())
    main = [window for window in config['app']['windows'] if window['label'] == 'main']
    assert len(main) == 1, f'{product}: one configured main window required'
    assert main[0].get('zoomHotkeysEnabled') is True, f'{product}: actual UI-02 browser zoom must be enabled'
print('User-flow plan and matrix ownership agree; execution is not implied.')
