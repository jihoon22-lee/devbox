# Bounded Windows candidate build sharding

## Overview

The unpublished stable candidate workflow now builds the 15 Windows applications in three parallel,
bounded shards instead of one serial Windows job. A separate Linux assembly job merges the staged
packages, generates the manifest and candidate evidence, and preserves the existing exact
15-application/32-file contract before either Windows acceptance job starts.

This is the second release-efficiency boundary. The preceding candidate-promotion change removed the
duplicate stable release rebuild; this change reduces the remaining candidate package-build critical
path without weakening the pre-publication acceptance gate.

## Context

- Historical candidate workflow run `33384213398` spent about 59 minutes building all 15 applications
  serially on one Windows runner.
- The catalog-order round-robin plan produces three five-application groups. Using that run's observed
  per-application durations, their package-build totals are approximately 22, 18, and 19 minutes.
- Three shards bound Windows runner fan-out while avoiding one slow shard dominating the critical path.
  Actual end-to-end improvement will be measured by the next real candidate run rather than asserted by
  the static estimate.

## Changes made

### Deterministic bounded planning

Files:

- `.github/scripts/plan-windows-package-shards.py`
- `.github/scripts/test-plan-windows-package-shards.py`

The planner reads the release catalog as the source of truth, requires exactly 15 unique safe release
application IDs, and emits a single-line GitHub Actions matrix. The workflow fixes the plan at three
shards, while the planner rejects fan-out outside the bounded range of two through four.

Catalog-order round-robin assignment is deterministic:

```text
01: port-manager, api-playground, life-log, run-manager, repo-manager
02: developer-toolbox, everything-plus, devbox-manager, workbench, devbox-launcher
03: wsl-desktop, knowledge-base, code-pad, webhook-lab, log-lens
```

Tests pin exact-once coverage, equal five-app sizes for the real catalog, stable ordering, bounded shard
counts, safe identifiers, and GitHub output encoding.

### Subset Windows packaging

File: `.github/scripts/build-windows-packages.ps1`

The existing packager accepts an optional validated application-ID subset. A shard builds and stages
only its assigned applications; unknown, duplicate, unsafe, or over-wide selections fail before a build.
Calling the script without a subset still builds all 15 applications and includes notices, preserving
the separately gated prerelease workflow.

All shards restore the same dependency cache. Only shard 01 may save it, preventing parallel jobs from
racing to publish one cache key. Intermediate package artifacts use no redundant compression for the
already-compressed executables and expire after one day.

### Fail-closed assembly

Files:

- `.github/workflows/windows-package-candidate.yml`
- `.github/scripts/flatten-windows-packages.py`
- `.github/scripts/test-flatten-windows-packages.py`

The workflow now has an explicit `plan -> build[3] -> assemble` dependency chain. Assembly downloads all
three uniquely named artifacts with merge semantics, adds the repository notices, creates the release
manifest, and flattens the complete package set on Linux.

The cross-platform flattener requires:

- exactly the 15 catalog applications in catalog order;
- one correctly named, non-empty regular portable and installer per application;
- only the declared two-level staging topology, with no symlinks or extra entries;
- manifest application and package names matching the catalog and staging tree;
- exactly 32 unique, non-empty regular files in the final flat asset directory.

Candidate metadata generation, independent digest verification, 14-day final artifact retention,
packaged runtime acceptance, and installer upgrade/rollback acceptance are unchanged. Both acceptance
jobs consume only the final assembled artifact.

### Contract and operator documentation

Files:

- `.github/scripts/check-catalog.sh`
- `.github/scripts/test-windows-package-candidate-config.py`
- `AGENTS.md`
- `CONVENTIONS.md`
- `docs/architecture.md`
- `docs/development.md`
- `docs/windows-guide.md`

The required catalog check now executes the planner and flattener unit tests and statically pins the
matrix, cache-writer, artifact merge, retention, assembly, and downstream dependency contracts. Release
documentation describes the three bounded shards and distinguishes one-day intermediate artifacts from
the 14-day acceptance candidate.

No application source, dependency, lockfile, version, public asset name, or acceptance threshold changed.

## Verification results

```text
python3 .github/scripts/test-plan-windows-package-shards.py       4 tests PASS
python3 .github/scripts/test-flatten-windows-packages.py          4 tests PASS
python3 .github/scripts/test-windows-package-candidate-config.py  PASS
bash .github/scripts/check-catalog.sh                             PASS
actionlint candidate/release/CI workflows                        PASS
Ruby workflow YAML parse                                         PASS
ruff check / format --check                                      PASS
git diff --check                                                  PASS
pnpm verify:affected                                              PASS
  frontend scope                                                  none
  Rust scope                                                      none
  dependency scope                                                all (authoritative CI audit selected)
```

GitHub Actions CI remains the required merge gate.
