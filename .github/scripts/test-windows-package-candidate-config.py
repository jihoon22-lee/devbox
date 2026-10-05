#!/usr/bin/env python3
"""Source/provenance and native gate contracts for the four-product candidate."""
from pathlib import Path
import json
import os
import subprocess
import sys
import tempfile
import yaml
ROOT = Path(__file__).resolve().parents[2]
def read(name): return (ROOT / name).read_text(encoding='utf-8')
w = read('.github/workflows/windows-package-candidate.yml')
a = read('.github/scripts/assemble-suite-candidate.ps1')
b = read('.github/scripts/build-windows-packages.ps1')
assert w.startswith('name: Windows package candidate\n\non:\n  workflow_dispatch:\n')
assert '\n  push:' not in w and '\n  pull_request:' not in w
for boundary in ['candidate_commit is not the exact current origin/main', 'candidate tag already exists', 'candidate release already exists', 'refs/heads/main', '--allow-prerelease false', 'cancel-in-progress: false']:
    assert boundary in w, boundary
assert '--shards 3' in w and 'max-parallel: 3' in w and 'fail-fast: false' in w
assert 'CARGO_BUILD_JOBS: 2' in w, 'Each isolated Windows host keeps two Cargo workers'
assert 'merge-multiple: false' in w and 'retention-days: 14' in w
assert 'build-manifest.py' not in w and 'flatten-windows-packages.py' not in w
for boundary in ['Shard provenance mismatch', 'Incomplete four-product candidate', 'Duplicate/unexpected shard file', 'Undeclared shard content', 'Incomplete shard files', 'Linked shard content', 'Shard file identity mismatch']:
    assert boundary in a, boundary
for script in ['build-suite-package.py', 'build-suite-installer.py', 'acquire-suite-nsis.ps1', 'build-candidate-metadata.py', 'verify-downloaded-release.py']:
    assert script in a, script
assert '--artifact-kind candidate' in a and 'if ($AllowPrerelease) { return }' in a
assert 'tauri build --no-bundle' in b and 'Four-product catalog required' in b
assert "if ($product -eq 'workspace') {\n      # A private test executable" in b, 'Only the Workspace shard consumes the private Windows LSP fixture'
assert 'fake-lsp-server.exe' not in a and 'resources/suite/devbox-suite-bootstrap.exe' in a
assert 'resources/wsl/devbox-workspace-wsl' in a and 'resources/wsl/manifest.json' in a
rebuild = read('.github/scripts/rebuild-suite-control-center.ps1')
assert "if ($product -in @('workspace','knowledge')) { $names +=" in rebuild, 'Retained rebuild must preserve both products WSL resources required by portable assembly'
for scope in ['product-shells', 'api', 'knowledge', 'cross-product']:
    assert scope in w
for script in ['windows-product-foundation.mjs',  'windows-api-lifecycle.mjs', 'windows-api-workflow.mjs', 'windows-knowledge-lifecycle.mjs', 'windows-suite-workflows.mjs', 'windows-suite-delivery.ps1', 'windows-workspace-owned-wsl2.ps1', 'compare-product-performance.mjs']:
    assert script in w, script
runtime = w.split('\n  packaged-runtime:', 1)[1]
assert 'cargo build' not in runtime and 'tauri build' not in runtime
assert 'prepare-suite-runtime.py' in runtime and 'prepare-suite-fixture.py' in runtime
assert 'DEVBOX_FIXTURE_PROFILE: release' in runtime
assert 'gh release create' not in w and 'gh release upload' not in w
assert 'artifact_name: candidate-assembly-' in w
assert 'Verify complete user journeys and seal candidate' in w
assert 'needs: [plan, assemble, packaged-runtime, installer-acceptance, migration-acceptance, windows-wsl2]' in w
assert '--require-user-flows' in w
assert 'collect-user-flow-evidence.mjs' in w
installer = w.split('\n  installer-acceptance:', 1)[1].split('\n  migration-acceptance:', 1)[0]
for step_name, evidence_name in (("Install interactively and complete visible activation", "hosted-display-installer"), ("Exercise actual work in the same installed namespace", "hosted-display-work")):
    step = installer.split("      - name: " + step_name + "\n", 1)[1].split("\n      - ", 1)[0]
    assert "shell: pwsh" in step
    assert "run: |" in step
    prepare = '. .github/scripts/prepare-windows-ui-display.ps1 -EvidenceName ' + evidence_name
    assert prepare in step
    assert step.index(prepare) < step.index("node .github/scripts/")
assert "Prepare hosted display for native layout acceptance" not in installer
assert 'name: ${{ needs.plan.outputs.artifact_name }}' in w
jobs = yaml.safe_load(w)['jobs']
migration = jobs['migration-acceptance']
assert migration['needs'] == 'assemble' and migration['runs-on'] == 'windows-2025'
assert migration['timeout-minutes'] == 40
assert migration['env']['DEVBOX_SUITE_ARTIFACT_SOURCE'] == '${{ inputs.candidate_commit }}'
assert migration['env']['DEVBOX_SUITE_ARTIFACT_RUN'] == '${{ github.run_id }}'
assert next(step for step in migration['steps'] if step.get('uses', '').startswith('actions/checkout@'))['with']['ref'] == '${{ inputs.candidate_commit }}'
download = next(step for step in migration['steps'] if step.get('uses', '').startswith('actions/download-artifact@'))
assert download['with'] == {'name': '${{ needs.assemble.outputs.artifact_name }}', 'path': 'candidate'}
commands = '\n'.join(step.get('run', '') for step in migration['steps'])
assert 'prepare-suite-fixture.py candidate/assets candidate/delivery --source $env:DEVBOX_SUITE_ARTIFACT_SOURCE --run-id $env:DEVBOX_SUITE_ARTIFACT_RUN' in commands
assert commands.index('prepare-suite-fixture.py') < commands.index('windows-suite-delivery.ps1')
assert "if ($LASTEXITCODE -ne 0) { throw 'Candidate delivery inputs failed.' }" in commands
assert 'windows-suite-delivery.ps1 -Staging candidate/delivery -Bootstrap candidate/delivery/devbox-suite-bootstrap.exe' in commands
assert '-RemainingOnly' not in commands and 'windows-suite-delivery.ps1' not in installer
upload = next(step for step in migration['steps'] if step.get('uses', '').startswith('actions/upload-artifact@'))
assert upload['if'] == '${{ always() }}'
assert upload['with'] == {'name': 'candidate-migration-${{ github.run_id }}', 'path': 'product-foundation-evidence', 'if-no-files-found': 'error', 'retention-days': 14}
seal = jobs['user-flow-acceptance']
assert 'migration-acceptance' in seal['needs']
assert any(step.get('with') == {'name': 'candidate-migration-${{ github.run_id }}', 'path': 'candidate/evidence/migration'} for step in seal['steps'])
seal_commands = '\n'.join(step.get('run', '') for step in seal['steps'])
assert "any(value['result'] != 'success'" in seal_commands
verification = next(step['run'] for step in seal['steps'] if step.get('name') == 'Require complete UI observations from the exact candidate bytes')
assert verification.index('suite-delivery.json') < verification.index('collect-user-flow-evidence.mjs')
code = verification.split("python3 - <<'PYTHON'\n", 1)[1].split('\nPYTHON', 1)[0]
source = 'a' * 40
valid = dict(result='passed', sourceSha=source, fixtureSourceSha=source, artifactRun='123', scope='installed-activation-generation-update-reinstall-data-restore-removal', cleanupFailures=[])
with tempfile.TemporaryDirectory() as directory:
    evidence = Path(directory) / 'candidate/evidence/migration/suite-delivery.json'
    evidence.parent.mkdir(parents=True)
    cases = [('valid', valid, True)]
    for key, bad in [('result', 'failed'), ('sourceSha', 'b' * 40), ('fixtureSourceSha', 'b' * 40), ('artifactRun', '124'), ('scope', 'remaining-updated-uninstaller-restore-reinstall'), ('cleanupFailures', ['owned cleanup failed'])]:
        cases.append((key, {**valid, key: bad}, False))
        cases.append(('missing-' + key, {name: value for name, value in valid.items() if name != key}, False))
    cases.append(('missing-file', None, False))
    for name, record, accepted in cases:
        if record is None:
            evidence.unlink()
        else:
            evidence.write_text(json.dumps(record), encoding='utf-8-sig')
        result = subprocess.run([sys.executable, '-c', code], cwd=directory, env={**os.environ, 'SOURCE': source, 'GITHUB_RUN_ID': '123'}, capture_output=True, text=True)
        assert (result.returncode == 0) == accepted, (name, result.stderr)
print('Four-product candidate source/provenance/native gates and 14 migration evidence cases: PASS')
