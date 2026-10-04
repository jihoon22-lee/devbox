#!/usr/bin/env python3
"""The product builds and native probe must share a cwd-independent target root."""
import re
import copy
import io
import json
import runpy
from contextlib import redirect_stdout
from unittest.mock import patch
from pathlib import Path, PureWindowsPath

root = Path(__file__).resolve().parents[2]
workflow = (root / ".github/workflows/product-foundation.yml").read_text()
match = re.search(r"^\s+CARGO_TARGET_DIR:\s*(.+)$", workflow, re.MULTILINE)
assert match, "hidden product builds must declare the shared Cargo target"
workspace = PureWindowsPath("D:/fixture/devbox")
target = PureWindowsPath(match.group(1).strip().replace("${{ github.workspace }}", str(workspace)))
assert target.is_absolute(), "pnpm --filter changes cwd; a relative Cargo target diverges per app"
for product in ("workspace", "api-studio", "knowledge", "control-center"):
    app_cwd = workspace / "apps" / f"devbox-{product}" / "src-tauri"
    assert app_cwd / target == workspace / "target"
probe = (root / ".github/scripts/windows-product-foundation.mjs").read_text()
assert 'path.resolve("target/debug"' in probe
assert 'process.env.RUNNER_ENVIRONMENT, "github-hosted"' in probe
assert 'process.env.GITHUB_ACTIONS, "true"' in probe
assert "contents: read" in workflow
installer = (root / ".github/scripts/windows-product-installation.ps1").read_text()
assert "$env:RUNNER_ENVIRONMENT -ne 'github-hosted'" in installer
assert "$env:GITHUB_ACTIONS -ne 'true'" in installer
assert "baseline manifest digest mismatch" in installer
assert "baseline tag commit mismatch" in installer
assert "product uninstall changed anchor installation or shortcut" in installer
assert "window-state-v1.json" in installer
assert "FindMainWindow($process.Id, $Title)" in installer
assert "owner != processId || !IsWindowVisible(window)" in installer
assert '. "$PSScriptRoot/windows-installer-helpers.ps1"' in installer
assert "windows-product-installation.ps1" in workflow
assert "Legacy performance baseline (Windows)" in workflow
assert "ref: 3a23f49c85aa3c3d04b86f227e8aa184ef964085" in workflow
performance = (root / ".github/scripts/windows-product-performance.ps1").read_text()
assert "$env:RUNNER_ENVIRONMENT -ne 'github-hosted'" in performance
assert "verify-downloaded-release.py" in performance
assert "baseline manifest digest mismatch" in performance
assert "--performance" in performance
print("Hidden product build/probe target and host boundary: PASS")

workflow_probe = (root / ".github/scripts/windows-api-workflow.mjs").read_text()
assert 'process.env.RUNNER_ENVIRONMENT, "github-hosted"' in workflow_probe
assert 'process.env.GITHUB_ACTIONS, "true"' in workflow_probe
assert "windows-api-workflow.mjs" in workflow
assert "windows-process-identity.mjs" in workflow
assert "Page.handleJavaScriptDialog" in workflow_probe
assert "captureMasked: true" in workflow_probe
assert "restartPreservesDraft: true" in workflow_probe

# Windows domain/WAL/vault unit tests belong to the required CI Windows job.
# The native workflow must not run a duplicate copy of the same unit suite.
ci = (root / ".github/workflows/ci.yml").read_text()
windows_job = ci.split("  rust-windows:\n", 1)[1]
assert "runs-on: windows-latest" in windows_job
assert 'run-rust-scope.sh test "$RUST_SCOPE" "$RUST_PACKAGES"' in windows_job, "Windows domain regressions must execute when affected, not merely compile"
assert 'RUST_PACKAGES: ${{ needs.scope.outputs.rust_packages }}' in windows_job
assert "product-native-authority-" not in workflow, "do not duplicate CI's Windows unit suite"
assert "windows-knowledge-lifecycle.mjs" in workflow
for source in ("packages/knowledge-features/**", "crates/knowledge-vault-engine/**", "crates/activity-engine/**", "crates/content-index-engine/**"):
    assert source in workflow, "native Knowledge consumers require acceptance on source changes"

import subprocess
subprocess.run(["node", str(root / ".github/scripts/check-api-studio-routes.mjs"), "--self-test"], check=True)
subprocess.run(["node", str(root / ".github/scripts/check-knowledge-routes.mjs"), "--self-test"], check=True)

check = runpy.run_path(str(root / ".github/scripts/check-product-foundation.py"))["check"]
original_read = Path.read_text

for source in ("packages/workspace-features/**", "crates/projects-engine/**",
               "crates/repositories-engine/**", "crates/editor-engine/**",
               "crates/runtime-engine/**", "crates/logs-engine/**",
               "crates/ports-engine/**", "crates/terminal-engine/**",
               ".github/scripts/copy-owned-terminal-profile.ps1"):
    assert source in workflow, "native Workspace consumers require acceptance on source changes"

for filename, changed_fields in [
    ("terminal.json", {"windows": ["*"]}),
    ("terminal.json", {"permissions": ["core:default", "workspace:allow-execute"]}),
    ("terminal.json", {"remote": {"urls": ["https://example.invalid"]}}),
]:
    capability_path = root / "apps/devbox-workspace/src-tauri/capabilities" / filename
    capability = json.loads(original_read(capability_path))
    capability.update(changed_fields)
    def read_capability(path, *args, **kwargs):
        return json.dumps(capability) if path == capability_path else original_read(path, *args, **kwargs)
    with patch.object(Path, "read_text", read_capability), redirect_stdout(io.StringIO()):
        try:
            check(root)
        except AssertionError:
            continue
    raise AssertionError("broadened Terminal capability was accepted")
print("Terminal companion capabilities remain separate and local: PASS")

# A typed product must not regain the former broad execute capability.
for product in ("knowledge", "api-studio", "control-center"):
    capability_path = root / f"apps/devbox-{product}/src-tauri/capabilities/default.json"
    capability = json.loads(original_read(capability_path))
    capability["permissions"].append(f"{product}:allow-execute")
    def read_capability(path, *args, **kwargs):
        return json.dumps(capability) if path == capability_path else original_read(path, *args, **kwargs)
    with patch.object(Path, "read_text", read_capability), redirect_stdout(io.StringIO()):
        try:
            check(root)
        except AssertionError:
            continue
    raise AssertionError(f"retired broad {product} execute capability was accepted")
print("Typed products cannot regain the retired execute capability: PASS")

# A coordinated stable release may advance any semver component, while every
# product and the agent must still agree on the exact version.
def check_versions(versions, agent_version):
    def read_version(path, *args, **kwargs):
        value = original_read(path, *args, **kwargs)
        try:
            relative = path.relative_to(root).as_posix()
        except ValueError:
            return value
        for product, version in {**versions, "agent": agent_version}.items():
            app = f"apps/devbox-{product}"
            native = app if product == "agent" else f"{app}/src-tauri"
            if relative == f"{native}/Cargo.toml":
                return re.sub(r'^version = "[^"]+"', f'version = "{version}"', value, count=1, flags=re.MULTILINE)
            if relative in (f"{native}/tauri.conf.json", f"{app}/package.json"):
                document = json.loads(value)
                document["version"] = version
                return json.dumps(document)
        return value
    with patch.object(Path, "read_text", read_version), redirect_stdout(io.StringIO()):
        check(root)

products = ("workspace", "api-studio", "knowledge", "control-center")
for version in ("0.8.1", "0.9.0", "1.0.0", "12.34.56"):
    check_versions(dict.fromkeys(products, version), version)
for version in ("01.2.3", "1.02.3", "1.2.03", "0.9", "v0.9.0", "0.9.0-beta.1", "0.9.0+build"):
    try:
        check_versions(dict.fromkeys(products, version), version)
    except AssertionError:
        continue
    raise AssertionError(f"non-stable or noncanonical version was accepted: {version}")
for versions, agent_version in (
    ({**dict.fromkeys(products, "0.8.1"), "workspace": "0.8.2"}, "0.8.1"),
    (dict.fromkeys(products, "0.8.1"), "0.8.2"),
):
    try:
        check_versions(versions, agent_version)
    except AssertionError:
        continue
    raise AssertionError("mixed product/agent versions were accepted")
print("Stable semver and coordinated product/agent release versions: PASS")

# Retained candidate API replays remain diagnostics, preserving the runner SHA.
suite_diagnostic = workflow.split("  suite-diagnostic:\n", 1)[1].split("  terminal-diagnostic:\n", 1)[0]
assert "suite_diagnostic_api_workflow:" in workflow
assert "candidate-assembly-$env:SOURCE_RUN" in suite_diagnostic
assert ".github/workflows/windows-package-candidate.yml" in suite_diagnostic
assert 'Assemble and verify unpublished candidate' in suite_diagnostic
assert '.conclusion == "success"' in suite_diagnostic
assert '$jobs -ne $record.head_sha' in suite_diagnostic
assert 'prepare-suite-runtime.py "$env:RUNNER_TEMP/api-candidate-retained/assets" --source "$($record.head_sha)"' in suite_diagnostic
assert 'runnerSourceSha = $env:GITHUB_SHA' in suite_diagnostic
assert 'payloadSourceSha = $record.head_sha' in suite_diagnostic
assert 'promotionEvidence = $false' in suite_diagnostic
assert 'native-api-workflow-diagnostic-' in suite_diagnostic
assert 'GITHUB_SHA=' not in suite_diagnostic and '$env:GITHUB_SHA =' not in suite_diagnostic
assert "suite-workflow-fixture-$env:SOURCE_RUN" in suite_diagnostic
assert "windows-suite-workflows.mjs --remaining" in suite_diagnostic

# Cross-product diagnostics use the same verified assembly, a distinct selection,
# and an explicit unpromotable receipt; ordinary retained fixture mode remains.
assert "suite_diagnostic_cross_product:" in workflow
assert "if (@($modes).Count -gt 1)" in suite_diagnostic
assert "cross-product-diagnostic-source.json" in suite_diagnostic
assert "diagnosticOnly = $true" in suite_diagnostic
assert "native-cross-product-diagnostic-" in suite_diagnostic
assert "'${{ inputs.suite_diagnostic_cross_product }}' -eq 'true' -or '${{ inputs.suite_full_workflow }}' -eq 'true'" in suite_diagnostic

# A targeted Knowledge probe reuses verified candidate bytes, never rebuilds
# products or disguises runner source as the payload source.
assert "suite_diagnostic_knowledge:" in workflow
assert "'${{ inputs.suite_diagnostic_knowledge }}'" in suite_diagnostic
assert "knowledge-components-diagnostic-source.json" in suite_diagnostic
assert "knowledge-components-diagnostic-only" in suite_diagnostic
assert "DEVBOX_PORTABLE_FIXTURES=target/portable-fixture" in suite_diagnostic
assert "DEVBOX_FIXTURE_PROFILE=release" in suite_diagnostic
assert "windows-product-foundation.mjs --knowledge-diagnostic" in suite_diagnostic
assert "native-knowledge-components-diagnostic-" in suite_diagnostic
for job in ("workspace-wsl-helper", "windows", "baseline-performance"):
    section = workflow.split(f"  {job}:\n", 1)[1]
    condition = next(line for line in section.splitlines() if line.strip().startswith("if:"))
    assert "!inputs.suite_diagnostic_knowledge" in condition, "Knowledge diagnosis must not rebuild unrelated products"

# The sequence replay preserves the preceding Workspace/API workloads and their
# exact private LSP inputs; it remains a retained-byte diagnostic, never a build.
sequence = suite_diagnostic.split("      - name: Finish the selected Suite acceptance scope", 1)[0]
assert "windows-product-foundation.mjs --product-sequence-diagnostic" in suite_diagnostic
assert "candidate-private-windows-lsp-${{ inputs.suite_diagnostic_run }}" in sequence
assert "candidate-private-wsl-lsp-${{ inputs.suite_diagnostic_run }}" in sequence
assert "windows-knowledge-wsl.ps1" in sequence
assert "windows-workspace-wsl-git.ps1" in sequence
assert "windows-product-performance.ps1 -Apps" in sequence
assert "sequenceReplay =" in suite_diagnostic
assert "always() && inputs.suite_diagnostic_knowledge && inputs.suite_full_workflow" in suite_diagnostic

installer_diagnostic = workflow.split("  installer-ui-diagnostic:\n", 1)[1].split("  terminal-diagnostic:\n", 1)[0]
assert "suite_diagnostic_installer_ui:" in workflow
assert "'${{ inputs.suite_diagnostic_cross_product }}' -eq 'true' -or $env:SOURCE_RUN" in installer_diagnostic
assert "!inputs.suite_diagnostic_installer_ui" in suite_diagnostic
assert "DEVBOX_API_DIAGNOSTIC_STYLES=1" in suite_diagnostic
assert "candidate-assembly-$env:SOURCE_RUN" in installer_diagnostic
assert "candidate-private-windows-lsp-${{ inputs.suite_diagnostic_run }}" in installer_diagnostic
assert "candidate-private-wsl-lsp-${{ inputs.suite_diagnostic_run }}" in installer_diagnostic
assert "DEVBOX_USER_FLOW_DIAGNOSTIC=true" in installer_diagnostic
assert "diagnosticOnly = $true; promotionEvidence = $false" in installer_diagnostic
assert "runnerSourceSha = $env:GITHUB_SHA" in installer_diagnostic
assert "payloadSourceSha = $record.head_sha" in installer_diagnostic
for script in ("windows-suite-user-flow.mjs", "windows-workspace-user-flows.mjs", "windows-api-user-flows.mjs", "windows-knowledge-user-flows.mjs", "windows-suite-delivery-user-flows.mjs", "windows-suite-legacy-upgrade-ui.mjs", "windows-suite-integration.mjs", "windows-suite-layout.mjs", "windows-suite-agent-user-flows.mjs"):
    assert script in installer_diagnostic
# Cross-product and layout journeys require the original committed namespace;
# delivery intentionally changes its generation and can leave health on failure.
for name in ("product-foundation.yml", "windows-package-candidate.yml"):
    text = (Path(".github/workflows") / name).read_text()
    order = next(line for line in text.splitlines() if "foreach ($script in" in line and "windows-suite-delivery-user-flows.mjs" in line)
    assert order.index("windows-suite-integration.mjs") < order.index("windows-suite-delivery-user-flows.mjs")
    assert order.index("windows-suite-layout.mjs") < order.index("windows-suite-delivery-user-flows.mjs")
    assert order.index("windows-suite-agent-user-flows.mjs") > order.index("windows-suite-delivery-user-flows.mjs")
assert "--withdrawn" in installer_diagnostic
for step_name, evidence_name in (("Install interactively and complete visible activation", "hosted-display-installer"), ("Exercise actual work in the same installed namespace", "hosted-display-work")):
    step = installer_diagnostic.split("      - name: " + step_name + "\n", 1)[1].split("\n      - ", 1)[0]
    assert "shell: pwsh" in step
    assert "run: |" in step
    prepare = '. .github/scripts/prepare-windows-ui-display.ps1 -EvidenceName ' + evidence_name
    assert prepare in step
    journey = "node .github/scripts/windows-suite-user-flow.mjs" if evidence_name == "hosted-display-installer" else "node .github/scripts/windows-suite-legacy-upgrade-ui.mjs"
    assert step.index(prepare) < step.index(journey)
assert "Prepare hosted display for native layout acceptance" not in installer_diagnostic
assert "windows-user-flow-install.ps1 -Cleanup" in installer_diagnostic
assert "windows-knowledge-wsl.ps1 -Cleanup" in installer_diagnostic
assert "windows-suite-delivery.ps1 -Staging candidate/delivery" in installer_diagnostic
assert "installer-ui-diagnostic-${{ github.run_id }}" in installer_diagnostic
assert "collect-user" not in installer_diagnostic and "promote" not in installer_diagnostic
assert "GITHUB_SHA=" not in installer_diagnostic and "$env:GITHUB_SHA =" not in installer_diagnostic

install_step = installer_diagnostic.split("      - name: Install interactively and complete visible activation\n", 1)[1].split("\n      - ", 1)[0]
work_step = installer_diagnostic.split("      - name: Exercise actual work in the same installed namespace\n", 1)[1].split("\n      - ", 1)[0]
assert "id: retained_install" in install_step
assert "continue-on-error" not in install_step
assert "if: ${{ success() || (failure() && steps.retained_install.outcome == 'failure') }}" in work_step
assert work_step.index("node .github/scripts/verify-retained-committed-install.mjs") < work_step.index("prepare-windows-ui-display.ps1")
assert "if ($LASTEXITCODE -ne 0) { throw 'Owned retained installation was not committed; independent journeys are blocked.' }" in work_step

# Apps-only retained diagnosis is an exclusive installer mode, never a rebuild.
assert "suite_diagnostic_apps_only:" in workflow
assert "purpose = 'retained-installer-ui-diagnostic-only'; appsOnly = $appsOnly" in installer_diagnostic
assert "appsOnly = $appsOnly" in installer_diagnostic
assert "Apps-only diagnosis requires only a retained installer source run" in installer_diagnostic
assert "if ($appsOnly -and $script -notin" in work_step
for stage in ("Installed journey start:", "Installed journey end:"):
    assert stage in work_step
assert "$scriptExitCode = $LASTEXITCODE" in work_step
assert "if ($scriptExitCode -ne 0)" in work_step
for title in ("Fetch the pinned withdrawn version for same-version replacement", "Exercise full installed Suite migration and recovery"):
    selected = installer_diagnostic.split("      - name: " + title + "\n", 1)[1].split("\n      - ", 1)[0]
    assert "if: ${{ !inputs.suite_diagnostic_apps_only }}" in selected
candidate_work = Path(".github/workflows/windows-package-candidate.yml").read_text().split("      - name: Exercise actual work in the same installed namespace\n", 1)[1].split("\n      - ", 1)[0]
assert "$scriptExitCode = $LASTEXITCODE" in candidate_work
assert "Installed journey start:" in candidate_work and "Installed journey end:" in candidate_work
assert "if ($scriptExitCode -ne 0)" in candidate_work

selected_apps = re.search(r"if \(\$appsOnly -and \$script -notin @\(([^\n]+)\)\)", work_step).group(1)
assert re.findall(r"'([^']+)'", selected_apps) == [
    "windows-workspace-user-flows.mjs", "windows-api-user-flows.mjs",
    "windows-knowledge-user-flows.mjs", "windows-suite-integration.mjs", "windows-suite-layout.mjs",
]
for job, block in re.findall(r"^  ([a-z][a-z0-9-]+):\n(.*?)(?=^  [a-z][a-z0-9-]+:|\Z)", workflow.split("jobs:\n",1)[1], re.M | re.S):
    condition = next(line for line in block.splitlines() if line.startswith("    if:"))
    if job == "installer-ui-diagnostic":
        assert "|| inputs.suite_diagnostic_apps_only" in condition
    else:
        assert "!inputs.suite_diagnostic_apps_only" in condition
# Exit status is captured before logging; later independent stages still run.
for block in (work_step, candidate_work):
    assert block.index('$scriptExitCode = $LASTEXITCODE') < block.index('Write-Host "Installed journey end: $script')
    assert block.index('$withdrawnExitCode = $LASTEXITCODE') < block.index('Write-Host "Installed journey end: withdrawn')

# The synthetic WindowsApplication helper requires Windows PowerShell 5.1.
# Keep its interpreter explicit while actual installed journeys remain on pwsh.
for name in ("product-foundation.yml", "windows-package-candidate.yml"):
    text = (root / ".github/workflows" / name).read_text()
    step = text.split("      - name: Check owned cleanup helpers\n", 1)[1].split("\n      - ", 1)[0]
    assert "shell: powershell" in step
    assert "windows-user-flow-cleanup-uninstall.test.ps1" in step
    assert "windows-user-flow-cleanup-observation.test.ps1" in step
