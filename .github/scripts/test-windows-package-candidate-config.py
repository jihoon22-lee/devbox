#!/usr/bin/env python3
"""Source/provenance and native gate contracts for the four-product candidate."""
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
def read(name): return (ROOT / name).read_text(encoding='utf-8')
w = read('.github/workflows/windows-package-candidate.yml')
a = read('.github/scripts/assemble-suite-candidate.ps1')
b = read('.github/scripts/build-windows-packages.ps1')
assert w.startswith('name: Windows package candidate\n\non:\n  workflow_dispatch:\n')
assert '\n  push:' not in w and '\n  pull_request:' not in w
for boundary in ['candidate_commit is not the exact current origin/main', 'candidate tag already exists', 'candidate release already exists', 'refs/heads/main', '--allow-prerelease false', 'cancel-in-progress: false']:
    assert boundary in w, boundary
assert '--shards 2' in w and 'max-parallel: 2' in w and 'fail-fast: false' in w
assert 'merge-multiple: false' in w and 'retention-days: 14' in w
assert 'build-manifest.py' not in w and 'flatten-windows-packages.py' not in w
for boundary in ['Shard provenance mismatch', 'Incomplete four-product candidate', 'Duplicate/unexpected shard file', 'Undeclared shard content', 'Incomplete shard files', 'Linked shard content', 'Shard file identity mismatch']:
    assert boundary in a, boundary
for script in ['build-suite-package.py', 'build-suite-installer.py', 'acquire-suite-nsis.ps1', 'build-candidate-metadata.py', 'verify-downloaded-release.py']:
    assert script in a, script
assert '--artifact-kind candidate' in a and 'if ($AllowPrerelease) { return }' in a
assert 'tauri build --no-bundle' in b and 'Four-product catalog required' in b
assert 'fake-lsp-server.exe' not in a and 'resources/suite/devbox-suite-bootstrap.exe' in a
assert 'resources/wsl/devbox-workspace-wsl' in a and 'resources/wsl/manifest.json' in a
for scope in ['product-shells', 'api', 'knowledge', 'cross-product']:
    assert scope in w
for script in ['windows-product-foundation.mjs',  'windows-api-lifecycle.mjs', 'windows-api-workflow.mjs', 'windows-knowledge-lifecycle.mjs', 'windows-suite-workflows.mjs', 'windows-suite-delivery.ps1', 'windows-workspace-owned-wsl2.ps1', 'compare-product-performance.mjs']:
    assert script in w, script
runtime = w.split('\n  packaged-runtime:', 1)[1]
assert 'cargo build' not in runtime and 'tauri build' not in runtime
assert 'prepare-suite-runtime.py' in runtime and 'prepare-suite-fixture.py' in runtime
assert 'DEVBOX_FIXTURE_PROFILE: release' in runtime
assert 'gh release create' not in w and 'gh release upload' not in w
assert 'artifact_name=windows-package-candidate-$CANDIDATE_TAG-$CANDIDATE_COMMIT' in w
print('Four-product candidate source/provenance/native gates: PASS')
