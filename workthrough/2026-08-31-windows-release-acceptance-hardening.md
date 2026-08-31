# Windows release acceptance hardening for v0.6.0

## Overview

The first exact-main v0.6.0 package candidate exposed two acceptance-harness defects rather than
product packaging failures: the installer matrix assumed that an NSIS-installed executable was
byte-identical to the separately built portable executable, and packaged smoke required foreground
activation that a non-interactive WSL-launched host cannot reliably grant. This change replaces
those invalid assumptions with reproducible installer-payload and visible-window contracts, keeps
cleanup fail-closed, and adds all-15-app packaged runtime acceptance to the unpublished candidate
workflow.

## Context

- Candidate workflow run `33343252629` built commit
  `31e99d1998fdcea90709d917370908877d63d468` and independently verified the expected
  15 apps / 32 files / 31 manifest-declared assets / mismatch 0 contract.
- Installer acceptance stopped on Port Manager because the installed NSIS payload SHA-256 differed
  from the portable asset. A local v0.5.1 reproduction proved that the two build products are
  intentionally different byte streams.
- The original cleanup registered install ownership only after strict validation and passed NSIS
  `_?=` during uninstall. A validation failure could therefore leave an unowned installation, while
  `_?=` prevented the normal self-copy deletion path and left `uninstall.exe` behind.
- Packaged smoke observed the normal Tauri single-instance behavior: the second process could exit
  before CIM exposed its complete identity, while the first window was restored but could not become
  the Windows foreground window from the hidden automation host.
- A later local run found short-lived descendants such as `conhost.exe` whose CIM executable path is
  unavailable. They can be tracked by PID, creation time, and name, but must never be force-terminated
  without the complete path identity.

## Changes made

### Installer lifecycle contract

File: `.github/scripts/windows-installer-acceptance.ps1`

- Resolve and register the exact uninstall entry, install directory, binary, and uninstaller as
  owned immediately after installation, before strict version/shortcut/notices validation.
- Stop comparing an installed executable with the portable asset digest.
- Capture the installed payload digest for each release and require it to repeat exactly on a fresh
  install or rollback of the same installer.
- Require the candidate update payload to differ from the baseline payload.
- Invoke NSIS uninstall with `/S`, allowing its self-copy process to remove `uninstall.exe` and the
  install directory normally.

The corresponding static test in
`.github/scripts/test-windows-installer-acceptance-config.py` asserts ownership ordering, removal of
the invalid portable comparison, reproducible installed digests, candidate replacement, and the
safe uninstall invocation.

### Packaged runtime and cleanup contract

File: `.github/scripts/windows-packaged-smoke.mjs`

- Accept a normal second-instance process that exits successfully before its complete CIM identity
  becomes observable; retain its PID lineage for fail-closed descendant detection.
- Require that the first app window is restored to a visible, non-minimized state. Foreground PID
  and renderer focus remain recorded diagnostics but are not release gates because foreground
  activation depends on the interactive caller's Windows entitlement.
- Split trackable process identities from complete termination identities. Descendants with a
  temporarily unavailable executable path are tracked through cleanup, but forced termination still
  requires PID, creation time, name, and exact executable path.
- Record sanitized process-inventory errors in evidence instead of exposing only a boolean.
- Extend the built-in self-test for early second-instance exit, PID-lineage fallback, restored-window
  semantics, and incomplete-identity termination rejection.

### Candidate workflow runtime gate

Files:

- `.github/workflows/windows-package-candidate.yml`
- `.github/scripts/test-windows-package-candidate-config.py`

The unpublished exact-main candidate now fans out to a fresh `windows-2025` packaged-smoke job in
parallel with installer acceptance. It downloads the exact build artifact, copies the acceptance
config to a run-owned scratch volume, executes all 15 portable app contracts sequentially, writes a
summary, and uploads JSON evidence for 14 days. It retains read-only repository permissions and does
not create, edit, or upload a GitHub Release.

## Key contract examples

```powershell
# Installed payloads from one installer must reproduce each other; they are
# not expected to equal the separately linked portable executable.
$candidateUpdate = Install-App $app $candidateRelease $CandidateAssets 'update'
$candidateBinarySha256 = [string]$candidateUpdate.binarySha256
Install-App $app $candidateRelease $CandidateAssets 'install' '' $candidateBinarySha256
```

```javascript
// Restoring a minimized app is deterministic. Foreground ownership is kept
// as evidence because a hidden automation host cannot force Windows focus.
if (state?.HasHandle && state.Visible && !state.Minimized) {
  return state;
}
```

## Verification results

### Static and parser verification

```text
node --check .github/scripts/windows-packaged-smoke.mjs                  PASS
node .github/scripts/windows-packaged-smoke.mjs --self-test             PASS
python3 test-windows-package-candidate-config.py                        PASS
python3 test-windows-installer-acceptance-config.py                     PASS
python3 test-windows-packaged-smoke-config.py                           PASS
PowerShell parser: windows-installer-acceptance.ps1                     PASS
Ruby YAML parser: windows-package-candidate.yml                         PASS
git diff --check                                                        PASS
```

### Local Windows packaged smoke

The corrected Port Manager run passed renderer identity, native `list_ports` IPC, ten-second health,
single-instance exit, minimized-window restoration, zero runtime/console/log errors, exact process
cleanup, original data restoration, and zero backup/quarantine/staging/journal residue. The active
installed WSL Desktop remained the same PID, creation identity, and executable path throughout.

Evidence:
`E:\devbox-v060-31e99d1.5zyZCz\output\packaged-smoke-port-v4.json`

### Local installer lifecycle

```text
baseline 0.3.0 install       ec3e85632914...7e9
candidate 0.4.0 update       5a09fc140ba0...4ca
candidate 0.4.0 fresh        5a09fc140ba0...4ca
baseline 0.3.0 rollback      ec3e85632914...7e9
final registry entries       0
final install directories    0
final processes              0
```

Evidence:
`E:\devbox-v060-31e99d1.5zyZCz\output\installer-port-lifecycle-local.json`

## Limitations and next steps

- WSL interop later stopped accepting new Windows process launches while several user-owned Codex
  PTYs remained attached to the active WSL Desktop. Restarting WSL or terminating that app would
  destroy those sessions, so no such cleanup was attempted.
- The user explicitly approved deferring only the physical installed WSL Desktop zellij/terminal
  reconnection check. The new hosted candidate job still exercises WSL Desktop's automated packaged
  renderer, IPC, single-instance, and cleanup contracts.
- Source CI and the new exact-main candidate run must pass before an annotated `v0.6.0` tag is
  created. The candidate's packaged-smoke and installer JSON artifacts are the authoritative final
  automated evidence; the deferred physical item remains documented separately in GitHub issue
  tracking.
