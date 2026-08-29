# Log Lens Run Manager stdout/stderr reader (#472)

## Overview

The v0.5.0 completion audit found that the Run Manager handoff could be
published, claimed, previewed, and accepted, but every accepted `run` source
still returned `AdapterUnavailable`. The plan's P3-02 source inventory includes
Run Manager stdout/stderr, and no separate reader issue existed. This change
completes that user flow without rebuilding an RC or replacing the published
v0.5.0 assets.

## Context and decision

- #366/#463 intentionally kept the producer payload identity-only:
  `{ kind, sourceId, runId, stream }`.
- Passing an absolute log path, command, raw log, database record, environment,
  or credential through AppLink would weaken that boundary.
- Log Lens therefore derives only the fixed Tauri data root for
  `com.devbox.runmanager`, validates the opaque run/stream identity again, and
  reads the known `logs/runs/<run-id>/<stream>.gN.oS-E.log` format directly.
- The change is a post-release main-branch completion. It does not create an RC,
  mutate the annotated `v0.5.0` tag, or replace release assets.

## Changes

### Fixed Run source resolution

`apps/log-lens/src-tauri/src/core/model.rs` exposes the already strict
source-ID parser to the app-local source layer. Only bounded alphanumeric,
hyphen, or underscore run IDs and the exact `stdout` or `stderr` stream are
accepted.

`apps/log-lens/src-tauri/src/core/sources.rs` maps that identity to the fixed
Run Manager app-local-data directory. It does not accept a path from the
producer or renderer. The directory and each segment are checked for
symlink/reparse components and canonical containment. The reader opens exact
regular files through the common no-follow filesystem helper and verifies the
directory identity again after the snapshot.

### Rotation and bounds

Segment names are parsed as half-open logical ranges, sorted by offset rather
than lexicographic generation text, and rejected when malformed, overlapping,
non-contiguous, duplicate-generation, oversized, or over the bounded segment
count. A decimal logical cursor resumes append reads. If retention has moved
past the cursor, the source reports `truncated`; if a replacement resets below
the cursor, it reports `rotated`. The existing 64 MiB source/parser/ring limits
remain the outer output boundary.

### Documentation

The Log Lens and Run Manager READMEs, project inventory, interop design, P3-02
plan status, and historical producer workthrough now distinguish the original
producer-only PR from the completed reader. They continue to state that raw
log duplication, arbitrary paths/commands, network ingest, and permanent
archives are out of scope.

## Verification

The focused fixture covers logical ordering across generations 8/9/10, stream
isolation, cursor resume, retention truncation, malformed segment rejection,
and linked-directory rejection. The final PR gate records the complete Log
Lens Rust suite, strict clippy/check/fmt, frontend test/build, diff checks, and
GitHub Actions results.

Local pre-PR results:

```text
cargo fmt --all -- --check                                      passed
cargo test -p log-lens -j2                                     52 passed
cargo check -p log-lens -j2                                    passed
cargo clippy -p log-lens --all-targets -j2 -- -D warnings      passed
pnpm --dir apps/log-lens test -- --run --maxWorkers=2
  --no-file-parallelism                                        4 files / 21 passed
pnpm --dir apps/log-lens build                                 passed
focused core::sources rerun after final limit fixtures         15 passed
check-dependencies.py check / generated notices                passed
catalog/build-manifest/downloaded-release script regressions   passed
release-note extractor regression                              passed
git diff --check                                               passed
```

The physical Windows app-data/NTFS path remains part of issue #176's truthful
post-release packaged matrix. The reader uses the same `dirs::data_local_dir`
and catalog-checked `com.devbox.runmanager` identifier as Run Manager's own
migration boundary, so the Windows-only check does not justify an RC or a tag
replacement.

## Files

- `apps/log-lens/src-tauri/Cargo.toml`
- `apps/log-lens/src-tauri/src/core/model.rs`
- `apps/log-lens/src-tauri/src/core/sources.rs`
- `CHANGELOG.md`
- `Cargo.lock`
- `THIRD_PARTY_NOTICES.md`
- `apps/log-lens/README.md`
- `apps/run-manager/README.md`
- `docs/projects.md`
- `docs/superpowers/specs/2026-08-17-app-interop-design.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
- `workthrough/2026-08-28-log-lens-producer-handoffs.md`
- `workthrough/2026-08-29-log-lens-run-reader.md`
