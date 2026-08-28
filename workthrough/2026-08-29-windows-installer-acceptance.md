# v0.5.0-rc3 Windows Installer Acceptance

## Overview

This checkpoint adds the first disposable-Windows installer acceptance gate after the immutable
`v0.5.0-rc3` asset checkpoint. The preceding asset result is treated as a package PASS: 15 apps,
31 manifest-declared assets, 32 downloaded and verified assets, missing 0, undeclared 0, and
exact manifest/GitHub size and SHA-256 matches. This document does not transfer that package
result to installer acceptance.

The workflow and PowerShell harness are intentionally workflow-dispatch-only. The workflow has
not yet been dispatched, so no run ID, Windows runtime result, installer result, or acceptance
PASS is claimed here. It cannot be called PASS until a real run produces explicit `status: PASS`
evidence and the cleanup read-back is clean.

## Acceptance identity and execution boundary

| Item | Contract |
|---|---|
| Intended candidate | `v0.5.0-rc3`; the dispatch also requires its expected peeled 40-character commit |
| Immutable baseline | Annotated `v0.4.2`, expected peeled commit `c9a320ef52ac2d6abe30d9f6e5364a09780b54c4` |
| Workflow trigger | `workflow_dispatch` only; no `push` or `pull_request` trigger |
| Permissions | workflow and job `contents: read`; checkout disables persisted credentials |
| Runner | GitHub-hosted `windows-2025`, one acceptance job, no overlapping run cancellation |
| Evidence | Run-owned `RUNNER_TEMP` tree, sanitized metadata, and uploaded `evidence.json` |

The workflow checks both the configured baseline and the candidate against the remote repository.
Each tag must have strict semver shape, exist on `origin`, be fetched as an annotated tag object,
peel to a commit, and match the supplied expected commit. The baseline and candidate tags must
differ. This prevents a stale, lightweight, retargeted, or accidentally reused tag from entering
the installer matrix. The runner checks out `main` with full history for this validation; it does
not build an unreviewed local tree.

The hosted runner is suitable for the installer lifecycle because it is a disposable Windows
environment where the actual NSIS installer and uninstaller can be run against real Windows
registry, install directories, shortcuts, executable metadata, and file removal. A read-only
preflight refuses to touch an existing Devbox state, and all generated paths are scoped to the
run. This is evidence for the lifecycle represented by this harness, not evidence for every W4
boundary: a hosted runner does not provide a physical multi-monitor/DPI/IME/accessibility/offline
matrix or a trustworthy real disk-full condition.

## Changes made

### Workflow and release inputs

`.github/workflows/windows-installer-acceptance.yml` now:

- accepts `candidate_tag` and `candidate_commit` only through manual dispatch;
- allocates a unique runner-temporary root with separate baseline assets, candidate assets,
  metadata, output, and scratch paths;
- validates the exact annotated `v0.4.2` baseline and supplied candidate tag before mutation;
- downloads each public release into a fresh directory and writes only safe release metadata;
- runs the PowerShell acceptance script with explicit baseline/candidate paths and identity; and
- uploads the JSON evidence even on failure, then requires the evidence file to exist with exact
  `status: PASS`.

The workflow uses `always()` for evidence upload and the final status gate, so a missing result or
non-PASS result cannot be hidden by an earlier step failure. The artifact retention is 14 days.

### Pinned app matrix

`.github/scripts/windows-installer-acceptance-config.json` and its Python contract test pin the
catalog order, product names, executable names, identifiers, legacy identifiers, and baseline
classification for all 15 release apps:

- Existing baseline (13): `port-manager`, `developer-toolbox`, `wsl-desktop`, `api-playground`,
  `everything-plus`, `knowledge-base`, `life-log`, `devbox-manager`, `code-pad`, `run-manager`,
  `workbench`, `webhook-lab`, and `repo-manager`.
- New candidate-only apps (2): `devbox-launcher` and `log-lens`.

The config test requires the matrix to equal the release catalog, requires exactly 13 baseline
entries and the two expected new entries, and cross-checks each identifier/product name against
Tauri metadata and the packaged-smoke contract.

### Fresh asset, manifest, and hash verification

`windows-installer-acceptance.ps1` verifies both independently downloaded release directories
before installing anything:

- Baseline `v0.4.2` must describe 13 apps, 26 binaries, and its manifest. Candidate `v0.5.0-rc3`
  must describe 15 apps, 30 binaries, `THIRD_PARTY_NOTICES.md`, and its manifest.
- Candidate release assets therefore have 31 manifest-declared assets and 32 total downloaded
  assets; the manifest is checked as the additional self-described asset.
- Release metadata must match tag, expected commit, non-draft state, and the expected prerelease
  state. A hyphenated candidate tag such as `v0.5.0-rc3` is required to be a prerelease.
- Manifest app IDs must match the configured matrix. Portable and installer names must be safe,
  unique, and follow the exact app/version naming contract.
- Every downloaded file is checked for exact manifest size and SHA-256, and for exact GitHub
  release API size and `sha256:<digest>`. Duplicate, missing, undeclared, or stale assets fail
  closed.

The two release directories are never mixed. Metadata is reduced to tag, publication state,
expected target commit, asset names, sizes, and digests; unnecessary API response fields are not
preserved in evidence.

### Fail-closed preflight and ownership

Before registry or app-data mutation, the harness refuses to continue if it sees any of the
following:

- a protected Devbox executable process in the Windows process inventory;
- `%LOCALAPPDATA%\devbox` integration data;
- an existing Devbox uninstall entry in the inspected HKCU/HKLM uninstall roots;
- an existing Devbox install directory, named Start Menu/Desktop shortcut, current identifier
  directory, or legacy identifier directory;
- an existing acceptance output, an output/custom path outside the run-owned scratch tree, or an unsafe
  custom install path; or
- a marker collision when a run-owned marker is about to be created.

The harness never broad-kills by image name, closes a user process, overwrites evidence, or
assumes an unexpected path error means absence. Installer roots are accepted only below
`LOCALAPPDATA` or `RUNNER_TEMP`; custom roots are descendants of the run scratch root. Installer
and uninstaller root subprocesses are owned by the harness and bounded by a timeout. The harness
does not use an image-name kill as a timeout fallback.

The core safety contract is represented by these guards:

```powershell
if ($preexistingProcesses.Count -ne 0) { Fail 'pre-existing Devbox process detected' }
if (Test-Path -LiteralPath $Output) { Fail 'refusing to overwrite acceptance evidence' }
Assert-Descendant $Output $ScratchRoot 'acceptance output'
```

## Installer lifecycle

### Existing 13-app baseline: install, RC update, preservation, rollback

For each baseline app, the harness performs this exact sequence from fresh release directories:

```text
v0.4.2 install
  → create run-owned marker(s) in current and legacy app-data identifiers
  → v0.5.0-rc3 installer update
  → assert marker bytes/SHA-256 are preserved
  → uninstall RC3
  → assert marker preservation and uninstall cleanup
  → reinstall RC3
  → direct update/rollback with the v0.4.2 installer
  → assert marker preservation
  → uninstall baseline
  → remove only run-owned markers and empty directories
```

The 13-app set is `port-manager`, `developer-toolbox`, `wsl-desktop`, `api-playground`,
`everything-plus`, `knowledge-base`, `life-log`, `devbox-manager`, `code-pad`, `run-manager`,
`workbench`, `webhook-lab`, and `repo-manager`.

After every install or update, the harness reads back the uninstall registry entry, uninstaller,
install location, executable, DisplayIcon, DisplayVersion, file version, shortcut, and applicable
notices resource. It checks the installed executable and notices against the release manifest.
The uninstaller must be the exact plain-path `uninstall.exe` below the declared install root; a
reparse point in that path is rejected. Uninstall must remove the exact observed registry key,
executable, shortcut, and install directory while leaving
the synthetic app-data marker available for the preservation assertion. The final marker removal
is delayed until the rollback and final uninstall have been checked.

This is a direct installer rollback, not Devbox Manager's catalog rollback. It verifies that the
older baseline package can replace the candidate package and that data-preservation markers remain
intact across the transition.

### New apps: custom/default clean lifecycle

For `devbox-launcher` and `log-lens`, which have no predecessor in the baseline package, the
harness runs a clean candidate lifecycle twice:

1. Install with `/S /D=<run scratch>/custom-install/<app-id>`, verify that the custom root was
   honored, create and hash the run-owned marker, uninstall, and verify removal/preservation.
2. Install with the default `/S` mode, verify the default current-user root and metadata, uninstall,
   and verify removal/preservation.

The preflight ensures the two new apps begin without prior registry, shortcut, install-directory,
or app-data state. No predecessor migration or upgrade result is inferred for either app.

### Cleanup and evidence

The `finally` path attempts to uninstall any install still owned by the run, removes remaining
run-owned markers, and performs an independent read-back. Evidence records uninstall residue,
exact-registry-key residue, install-directory residue, marker residue, app-data residue,
integration-directory residue, and cleanup failures. Any nonzero residue or cleanup failure forces
the final status to `FAIL`.

The JSON report also records candidate identity, sanitized host information, baseline/candidate
release identities and manifest/metadata hashes, per-app phases, marker hashes, public failure
messages, and the known low-disk limitation. Windows paths in public errors are redacted and
messages are bounded. No user data, credential, or raw secret is written by this harness.

## Current limitations and non-claims

The initial script deliberately does not close the full W4 matrix:

- The workflow has not been dispatched. Static validation is PASS, but installer acceptance is
  **PENDING**, with no runtime result or run ID to report.
- It does not launch the installed application. Cold/hot start, WebView startup, window restore,
  second-instance focus, AppLink, and UI behavior remain outside this script.
- It does not seed or migrate real v0.4.1 user data. The configured baseline is the immutable
  `v0.4.2` package, and the marker is a synthetic preservation fixture rather than a real History,
  Collection, index, Knowledge/Life Log, Run Manager, or Workbench data migration.
- Locked-file, ACL-denial, and real disk-full injection are not implemented in this initial script.
  The hosted runner can exercise ordinary install/update/uninstall outcomes, but cannot be treated
  as evidence for those failure-injection branches.
- Physical W4-C boundaries remain open: monitor movement and physical DPI/resolution behavior,
  Korean IME composition, keyboard/screen-reader accessibility, scaling, true offline/no-tool
  behavior, and a real low-disk volume require dedicated directly observable environments.

The hosted workflow is therefore an installer lifecycle gate with strong cleanup and provenance
checks, not a substitute for physical W4-C or real user-data migration acceptance.

## Safety and evidence rules

- Existing processes, registry entries, app-data, integration paths, install roots, and shortcuts
  are inspected before mutation. A collision is a refusal, not authorization to terminate or move
  user state.
- Release bytes are downloaded into fresh baseline/candidate directories, and exact metadata,
  manifest, size, and SHA-256 identities are retained in the report without raw API noise.
- All generated state is scoped to a unique runner-temporary root or the per-app marker paths that
  passed the preflight. The output path must not already exist and must be a descendant of scratch.
- Cleanup is checked after ownership-based uninstall/removal; unresolved state blocks PASS.
- The workflow's final step requires the exact release identities, 15 unique app results, the
  13/2 baseline/new partition, complete ordered lifecycles, valid marker/install evidence, zero
  failures, zero residue, and `status: PASS`. A static check, source review, or package asset PASS
  cannot replace that runtime evidence.

## Local verification

The following read-only local checks were performed in this worktree:

```text
python3 .github/scripts/test-windows-installer-acceptance-config.py  PASS
bash .github/scripts/check-catalog.sh                            PASS
Windows PowerShell Parser::ParseFile on windows-installer-acceptance.ps1  PASS (exit 0)
Ruby Psych YAML parse on windows-installer-acceptance.yml         PASS
Windows PowerShell Parser::ParseFile on all workflow pwsh blocks  PASS
actionlint .github/workflows/windows-installer-acceptance.yml     PASS
git diff --check                                                   PASS
```

`check-catalog.sh` also passed its packaged-smoke configuration and downloaded-release verifier
fixtures before returning success. These are source/configuration checks only. The GitHub-hosted
workflow itself remains undispatched and must be run with the exact candidate tag and peeled
commit before this gate can be reported as PASS.

## Next steps

1. Dispatch the workflow with the immutable `v0.5.0-rc3` tag and its exact peeled commit after the
   candidate release identity is available.
2. Preserve the run's sanitized metadata and JSON evidence, including any fail-closed collision
   or cleanup result; do not invent a PASS from a skipped or unavailable run.
3. Complete the separately required app-launch, real v0.4.1 migration, locked/ACL/disk-full, and
   physical W4-C/offline acceptance in suitable disposable or directly observable environments.
