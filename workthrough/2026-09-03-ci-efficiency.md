# Affected CI verification and local developer loop

## Overview

The monorepo verification path now resolves changed files through the pnpm and
Cargo workspace dependency graphs. A normal app change builds and tests that app,
while a shared package or crate change also selects its transitive consumers. Jobs
with no relevant work are skipped before a hosted runner is allocated; explicit,
weekly, release and verifier-contract audits retain repository-wide coverage.

## Context

- The repository contains 15 Tauri apps, 7 frontend packages and 28 Cargo
  workspace packages. Re-running all of them for an isolated change made the CI
  latency substantially longer than the implementation loop.
- The previous scope shell script selected every frontend for any `packages/*`
  change and every Rust package for any `crates/*` change. It also treated every
  CI helper change as a full compiler change.
- The dependency-policy job installed both ecosystems and repeated every audit on
  every PR, even when no manifest, lockfile, policy or verification contract had
  changed.
- The frontend job ran `tsc` during each app build and then ran a second explicit
  typecheck over the same app. Only `packages/editor` and `packages/diff-view`
  currently need the additional typecheck because they have a `tsconfig.json` but
  no build script.

## Changes made

### Graph-aware scope resolver

Files:

- `.github/scripts/resolve-ci-scope.py`
- `.github/scripts/ci-scope.sh`
- `.github/scripts/test-ci-scope.py`

The resolver reads workspace package edges from `package.json` files and from
offline `cargo metadata --locked --no-deps`. It computes reverse dependency
closures, keeping frontend source and each app's `src-tauri` tree separate.

Important safety behavior:

- an app source change selects only that side of that app;
- a shared package or crate selects itself and every transitive consumer;
- a workspace manifest paired with its lockfile remains scoped;
- a lockfile-only change, unknown/deleted package path, root workspace manifest,
  or verifier implementation change selects the relevant full workspace;
- invalid path encodings, control characters, traversal and unsupported path
  separators fail closed;
- direct `apps/catalog.json` imports are represented as tested virtual graph
  edges because they do not appear in package manifests;
- documentation-only changes select no compiler or dependency work.

The regression suite covers app-only changes, shared packages and crates with
different fan-out, catalog virtual edges, manifest/lock pairing, documentation,
manual audits, unknown paths and unsafe paths.

### Scoped command runners and local verification

Files:

- `.github/scripts/run-frontend-scope.sh`
- `.github/scripts/run-rust-scope.sh`
- `.github/scripts/verify-affected.sh`
- `.github/scripts/test-ci-scope-runners.py`
- `package.json`

`pnpm verify:affected` evaluates commits since the merge-base with `origin/main`
plus staged, unstaged and untracked files. `pnpm verify:all` is the explicit full
audit. Both use the same resolver and runners as CI.

Selected frontend packages are passed to one recursive pnpm invocation, preserving
pnpm's parallel and topological execution. The extra TypeScript pass skips packages
whose build script already contains `tsc`. Selected Cargo actions use repeated
`-p <package>` arguments; Rust dependencies are still compiled by Cargo itself.

The runner regression test uses isolated fake `pnpm` and `cargo` executables to
assert exact argv, working directories, full/none behavior, validation failures,
Clippy flags and the workspace format contract without rebuilding applications.

### CI job allocation and safety net

File: `.github/workflows/ci.yml`

- Scope detection is now a prerequisite for dependency, frontend, Linux Rust and
  Windows Rust jobs.
- A job with scope `none` is skipped at the job condition, before runner
  allocation. If scope detection fails, all dependent gates run their explicit
  failure step so a resolver failure cannot bypass required checks.
- The dependency-policy runner is allocated only for dependency/policy inputs.
- The scope job publishes the selected packages and reason to the Actions step
  summary.
- `workflow_dispatch` and the weekly schedule select a full audit. The weekly
  audit runs at Sunday 18:17 UTC (Monday 03:17 KST).
- The catalog consistency and frontend accessibility contract job remains an
  unconditional, inexpensive repository-wide guard.

GitHub documents that a job skipped by a job-level condition reports success and
does not block a required-check merge, which preserves branch-protection behavior:
<https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-jobs-with-conditions>

### Developer documentation

Files:

- `AGENTS.md`
- `CONVENTIONS.md`
- `docs/development.md`
- `docs/architecture.md`

The completion contract now leads with focused tests plus
`pnpm verify:affected`. Full workspace verification is reserved for a resolver
decision of `all`, a release, the weekly audit, or an explicit manual audit. The
architecture notes also document the catalog's direct build-time consumers.

No runtime dependency, lockfile, application version, release asset or product
source changed.

## Code examples

### Reverse dependency closure

```python
# .github/scripts/resolve-ci-scope.py
def closure(self, seeds: Iterable[str]) -> set[str]:
    affected = set(seeds)
    pending = list(affected)
    while pending:
        dependency = pending.pop()
        for consumer in self.reverse.get(dependency, set()):
            if consumer not in affected:
                affected.add(consumer)
                pending.append(consumer)
    return affected
```

### Required gate without an unnecessary runner

```yaml
# .github/workflows/ci.yml
needs: scope
if: ${{ always() && (needs.scope.result != 'success' || needs.scope.outputs.rust_scope != 'none') }}
```

### One selected pnpm traversal

```bash
# .github/scripts/run-frontend-scope.sh
pnpm -r "${filters[@]}" "$action"
```

## Verification results

### Full safety audit for this verifier change

```text
pnpm verify:all
  scope resolver tests                              PASS
  scope runner argv tests                           PASS
  15-app/frontend-package build                     PASS
  15-app bundle budgets                             PASS
  all frontend tests                                PASS
  uncovered TypeScript packages (2)                 PASS
  Cargo workspace check                             PASS
  Cargo workspace Clippy (-D warnings)               PASS
  Cargo fmt --all --check                           PASS
  Cargo workspace tests and doc tests               PASS
Exit code: 0
```

### Selected-path checks

```text
apps/run-manager frontend                            build PASS; 61 tests PASS
packages/openapi closure                             build PASS
  @devbox/openapi + API Playground + Webhook Lab     363 tests PASS
crates/process closure                               check PASS
  process + port-manager                             42 tests PASS
```

Historical diff simulations also selected:

```text
04f0817  WSL Desktop frontend manifest/lock change   frontend=wsl-desktop; rust=none
ad32713  Devbox Manager app change                   frontend=devbox-manager; rust=devbox-manager
```

### Policy and static checks

```text
pnpm audit --audit-level moderate                    PASS (no known vulnerabilities)
dependency policy/notices and regression fixtures   PASS
cargo deny --locked check                            PASS
catalog consistency and 15-app accessibility        PASS
actionlint                                           PASS
ShellCheck                                           PASS
Ruff                                                 PASS
git diff --check                                     PASS
```

## Next steps

- Open the CI-efficiency PR and require its intentionally full Linux, Windows,
  frontend and dependency checks to pass before merge.
- Confirm the first ordinary frontend-only and Rust-only PRs show the expected
  skipped jobs and selected package lists in the scope summary.
- Continue the previously reviewed product fixes as separate user-visible PRs:
  WSL Desktop terminal stability, API Playground MCP draft preservation and
  shared StrictMode/listener lifecycle cleanup.
