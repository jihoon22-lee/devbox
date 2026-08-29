# Fail-Closed Prerelease Release Guard

## Overview

The Release workflow previously treated every `v*` tag as a releasable input and
derived the prerelease state from the presence of a hyphen. That allowed an
annotated RC tag push to proceed into the Windows build and release jobs without
an explicit operator decision. This change makes stable releases keep their
existing push/manual paths while requiring an explicit manual prerelease gate
for every prerelease version.

## Context

- Stable tags remain exact `vMAJOR.MINOR.PATCH` tags and still publish as Latest.
- A prerelease tag push must fail in preflight, before the Windows build or any
  GitHub Release creation step.
- A future prerelease run must use `workflow_dispatch`, enter the exact full
  prerelease tag, and set `allow_prerelease=true`; the boolean defaults to
  `false`.
- No GitHub tag, release, or remote branch was created or changed during this
  work.

## Changes Made

### 1. Central release-input policy

File: `.github/scripts/validate-release-input.py`

- Added dependency-free strict release-tag validation.
- Rejects prerelease tags from `push` events.
- Rejects manually entered prerelease tags unless the exact
  `allow_prerelease=true` gate is present.
- Emits `tag`, `prerelease`, and `make_latest` from the same validated result so
  downstream jobs do not infer state independently.

File: `.github/scripts/test-validate-release-input.py`

- Added unit and CLI coverage for stable push/manual inputs, explicitly gated RC
  and beta inputs, implicit RC push/manual rejection, malformed tags, unknown
  events, invalid gate values, and GitHub output writing.

### 2. Release workflow integration

File: `.github/workflows/release.yml`

- Added the deliberately named `allow_prerelease` boolean workflow-dispatch
  input with default `false`.
- Runs the central validator as the first preflight policy check. A failed RC
  push therefore prevents `build-windows`, `publish`, and `verify` through the
  existing job dependencies.
- Uses the preflight outputs for manifest and downloaded-release prerelease
  state while preserving stable `make_latest=true` behavior.

File: `.github/workflows/ci.yml`

- Runs the new policy unit test with the existing dependency/script regression
  tests.

### 3. Documentation and acceptance example

Files: `CONVENTIONS.md`, `docs/windows-guide.md`

- Documented stable trigger paths and the manual-only, explicitly gated
  prerelease policy.
- Documented that rejected prerelease pushes do not build or create releases.

File: `.github/workflows/windows-installer-acceptance.yml`

- Changed the candidate example from the historical RC tag to the stable
  `v0.5.0` tag.

## Code Examples

```bash
# .github/workflows/release.yml preflight
python3 .github/scripts/validate-release-input.py \
  --event "$EVENT_NAME" \
  --tag "$TAG" \
  --allow-prerelease "$ALLOW_PRERELEASE" \
  --github-output "$GITHUB_OUTPUT"
```

```text
push + v0.5.0       -> accepted, prerelease=false, make_latest=true
push + v0.5.0-rc1   -> rejected before build/release jobs
manual + RC + false -> rejected
manual + RC + true  -> accepted, prerelease=true, make_latest=false
```

## Verification Results

The following checks passed in the dedicated worktree:

```text
python3 .github/scripts/test-validate-release-input.py       10 tests, OK
python3 .github/scripts/test-build-manifest.py               passed
python3 .github/scripts/test-check-dependencies.py           passed
python3 .github/scripts/test-extract-release-notes.py        4 tests, OK
python3 .github/scripts/test-verify-downloaded-release.py    passed
python3 .github/scripts/test-windows-packaged-smoke-config.py passed
python3 .github/scripts/test-windows-installer-acceptance-config.py PASS
PyYAML safe_load for release/CI/acceptance workflows         OK
actionlint release/CI/acceptance workflows                   passed
git diff --check                                             passed
```

The direct policy smoke check also accepted stable `v0.5.0` and rejected
`push` + `v0.5.0-rc1` with the expected fail-closed message. Full Cargo and
frontend builds were not run because this change only touches workflow,
documentation, and Python validation paths.

## Risks and Follow-up

- The validator accepts any strict prerelease identifier (`rc`, `beta.1`, and
  similar), but never enables it implicitly; repository owners still decide
  when to use the manual gate.
- The remote annotated-tag, source-commit, duplicate-release, asset, and
  changelog checks remain in the workflow and were not weakened.
- GitHub Actions runtime behavior should be observed on the next ordinary
  stable release; no live release was run as part of this change.
