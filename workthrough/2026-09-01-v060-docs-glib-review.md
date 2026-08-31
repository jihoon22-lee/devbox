# v0.6.0 documentation synchronization and glib advisory review

## Overview

The repository documentation still described v0.5.1 as the current stable release after v0.6.0
had been published. This maintenance change synchronizes the current release identity and successor
validation checklist while preserving older release evidence as history. It also re-evaluates the
open `glib 0.18.5` Dependabot alert against the current Cargo graph, official Tauri release and
upstream manifest instead of extending or dismissing the exception by assumption.

## Context

- v0.6.0 is the published Latest release at source commit
  `d2fa25a0a1f087459838449daded00c0b09764b4`.
- Exact-main candidate run `33384213398` and release run `33390009009` passed the 15-app,
  32-public-asset, 31-manifest-declared, mismatch-zero contract.
- Issue #176 was the v0.5.1 historical checklist and is closed. Issue #518 is the current
  post-release checklist.
- Dependabot alert `GHSA-wrw7-89jp-8q8g` remains open for `glib 0.18.5`. The GitHub advisory marks
  `>=0.15.0,<0.20.0` affected and `0.20.0` as the first patched version.

## Changes made

### Current release and validation status

Files:

- `AGENTS.md`
- `README.md`
- `CHANGELOG.md`
- `docs/architecture.md`
- `docs/development.md`
- `docs/projects.md`
- `docs/roadmap.md`
- `docs/windows-guide.md`
- `docs/superpowers/plans/2026-08-28-v0.5.0-release.md`
- `docs/superpowers/plans/2026-08-31-v0.6.0-release.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

The current-state sections now identify v0.6.0 as Latest, record the exact candidate/release
evidence, link #518, and identify #176 as closed history. Historical v0.5.0/v0.5.1 descriptions and
deleted RC evidence remain intact. The v0.6.0 release plan has an explicit completion banner rather
than rewriting its original gate sequence as though it had never been a plan.

The roadmap no longer has an active `#349–#350 Windows packaged 확인` checkbox. It is closed as a
historical checkpoint without claiming that the old manual scenario was retroactively executed.
There is no active v0.7+ milestone; optional WSL LSP, MCP WSL stdio, legacy MCP SSE/GET and
OCR/semantic-search ideas remain candidates rather than commitments.

### Dependency evidence and advisory decision

Files:

- `.github/dependency-policy.json`
- `docs/dependency-policy.md`
- `CHANGELOG.md`

The current v0.6.0 `Cargo.lock` and `THIRD_PARTY_NOTICES.md` evidence is recorded as:

```text
Cargo.lock             ebe22c7df176d95685cc9ff9c0eb3760ac08b95829498d7c5884f89dd10977c7
THIRD_PARTY_NOTICES.md 6cbc242562e62ac8892bc88e3ca8fcad5e1dd7911db2b46a200cb8d1786a26d9
notices size           145317 bytes
```

The advisory exception review date is updated to 2026-09-01. Its expiry remains 2026-11-30 and the
Dependabot alert remains open. No dependency, lockfile, notice inventory or shipped binary changes.

## Decision evidence

### Resolved dependency path

```text
tauri 2.11.5
  -> gtk 0.18.2 / webkit2gtk 2.0.2
  -> glib 0.18.5
```

`cargo tree -i glib@0.18.5 --workspace -e normal` confirms that this is Tauri's Linux GTK runtime
path. Windows releases do not link it, but that platform boundary is not a vulnerability-free claim.

### Compatible upgrade attempt

```text
$ cargo update -p glib@0.18.5 --precise 0.20.0 --dry-run
error: failed to select a version for the requirement `glib = "^0.18"`
required by package `gtk v0.18.2`
```

The official Latest Tauri release is still `2.11.5`, and the upstream `dev` manifest still declares
`gtk = "0.18"`. A forced `glib 0.20.0` update is therefore not compatible. Vendoring or pointing at
an unreviewed fork would widen source and maintenance risk while leaving devbox responsible for a
Linux GTK fork; that is not justified for the current Windows distribution boundary.

## Verification results

```text
pnpm install --frozen-lockfile                         PASS
pnpm audit --audit-level moderate                      PASS (0 known vulnerabilities)
python3 .github/scripts/check-dependencies.py check    PASS
python3 .github/scripts/test-check-dependencies.py     PASS
python3 .github/scripts/test-build-manifest.py         PASS
python3 .github/scripts/test-extract-release-notes.py  PASS (4 tests)
bash .github/scripts/check-catalog.sh                  PASS
cargo deny --locked check                              PASS
python3 -m json.tool .github/dependency-policy.json    PASS
git diff --check                                       PASS
```

`cargo deny` reported only the repository's existing allowed duplicate-crate warnings. No new
advisory, license or source failure was introduced.

## Next steps

- Re-evaluate the alert on every Tauri update and no later than 2026-11-30. Remove the exception
  when Tauri exposes a compatible maintained GTK/glib line; do not dismiss it merely to clear the UI.
- Complete the installed WSL Desktop zellij/terminal reconnect observation in #518 without stopping
  existing WSL/Codex sessions or service Docker containers.
- Decide the legacy identifier and Life Log absorption migration retirement after another minor
  adoption/data-read-back checkpoint.
