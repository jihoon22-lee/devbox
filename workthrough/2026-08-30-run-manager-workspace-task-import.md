# Run Manager workspace task import (PR1)

**Date:** 2026-08-30
**Branch:** `feat/run-manager/task-import`
**Issue:** [#486](https://github.com/jihoon22-lee/devbox/issues/486)

## Overview

Run Manager can now preview one bounded project `.vscode/tasks.json` source and import selected
process tasks as disabled, untrusted drafts. Trust is tied to the exact project filesystem identity,
source revision, and execution target; source changes invalidate trust, availability, and enablement
atomically before a later run can start.

The implementation keeps the preview offline and preserves executable/argv boundaries. Windows and
WSL use the existing owned process-tree lifecycle, while shell tasks, matcher execution, dependency
orchestration, and Workbench task orchestration remain PR2/pending.

## Problem and decision

The existing local import contract covered only bounded `package.json` scripts and Cargo targets.
Developers also need to review project-local VS Code tasks, but importing an arbitrary task definition
must not turn extension commands, dynamic variables, shell text, or a stale file into execution
authority.

PR1 therefore uses a small native projection:

```text
bounded JSONC read → offline preview → atomic disabled/untrusted import
                      → explicit exact-revision trust → pre-spawn revalidation → direct argv spawn
```

The source is authoritative for the task name, executable, argv, cwd, and target. User-controlled
schedule policy and declared environment values remain editable through the existing Run Manager
flows.

## Security boundaries

### Source and parser

- Only the project root's `.vscode/tasks.json` is read; root, directory, file identity, and metadata
  are checked around the read. Symlink/reparse-point sources and traversal are rejected.
- JSONC support is limited to comments and trailing commas, with strict UTF-8/JSON parsing afterward.
  The root must declare version `2.0.0` and an array-valued `tasks` field.
- Bounds include a 4 KiB root display, 512 KiB source, 128 tasks, 128 argv entries per task,
  16 KiB strings, and 64 KiB total argv. Preview does not start a task, shell, package manager,
  Cargo, network request, VS Code process, or extension host.

### Projection and trust

- Only ready `process` tasks are selectable in PR1. Executable and argv remain separate; `cmd.exe`,
  shell parsing, quote-object arguments, extension types, shell tasks, dependencies, background tasks,
  `runOptions`, and dynamic variables are blocked.
- The only substitutions are `${workspaceFolder}`, `${workspaceFolderBasename}`, `${pathSeparator}`,
  and `${/}`. Resolved cwd must remain inside the canonical project root.
- Environment values are never read or imported. Only declared environment key names cross the
  preview/persistence boundary; values are entered later through the existing DPAPI editor.
- Import writes source/task side tables and jobs in one transaction. Imported jobs start as
  `enabled=false`; source trust starts false. Trust uses the exact opaque project identity and
  lower-case SHA-256 source revision, and does not execute anything.
- A changed or unavailable source clears trust, marks the source tasks unavailable, and disables their
  jobs in one transaction. Run/enable and the adapter's final pre-spawn path revalidate the same
  source and persisted projection. A failed check has no shell fallback and exposes only fixed error
  categories.

### Platform execution and editing

- Windows process tasks use direct Win32 argv quoting and the existing suspended CreateProcessW → Job
  Object ownership path; they do not use `cmd.exe`.
- WSL process tasks pass fixed argv through the WSL supervisor and clean up the owned process group;
  user command text is not interpolated into a shell script.
- Source-managed name, command, argv, cwd, and target/distro are locked in the normal Job Editor.
  Schedule, overlap policy, catch-up, and values for declared environment keys remain configurable,
  subject to current trust and revalidation before enable/run.

## Files and surfaces

- `apps/run-manager/src-tauri/src/core/workspace_tasks.rs` — bounded JSONC parsing, Windows/Linux
  override projection, safe variables, root containment, opaque identity/revision, and revalidation.
- `apps/run-manager/src-tauri/src/storage.rs` — schema-v3 source/task side tables, atomic import and
  invalidation, exact-revision trust, managed-field locks, and declared-environment-key validation.
- `apps/run-manager/src-tauri/src/commands.rs` — preview/cancel/apply/list/trust commands plus
  run/enable revalidation and fixed public error mapping.
- `apps/run-manager/src-tauri/src/platform/{execution,windows,wsl}.rs` — final pre-spawn checks,
  direct process launch, Windows Job Object ownership, WSL argv forwarding, and process-group cleanup.
- `crates/wsl/src/argv.rs` — an explicit `wsl.exe --exec` argv boundary used by process-mode
  execution so arguments are not reparsed by the distribution's default shell.
- `apps/run-manager/src-tauri/src/{core/imports.rs,core/models.rs,core/shell.rs,lib.rs,notifications.rs,scheduler.rs}`
  — shared validation, model/command registration, shell/argv boundary, scheduler and notification
  integration.
- `apps/run-manager/src/{App.css,App.tsx,api.ts,components/ImportDialog.tsx,types.ts}` — VS Code
  task import mode, target selection, offline preview, ready/blocked selection, fixed error text,
  and persisted workspace-task state display.
- `docs/superpowers/specs/2026-08-30-run-manager-workspace-task.md` and
  `apps/run-manager/README.md` — implementation contract and the PR1 user/security contract; the
  older design documents remain historical references.

No package or runtime dependency was added for this PR1 projection.

## Tests and evidence

Core and storage coverage includes JSONC comments/trailing commas and escaped strings; exact version
and shape checks; Windows/Linux overrides; allowed and blocked variables; duplicate labels and bounds;
root/file identity changes including same-size rewrites; stale preview/trust; direct argv round-trip;
atomic disabled/untrusted import; source invalidation; managed fields; and environment-key allowlists.

Current local evidence at handoff:

```text
Run Manager Rust library tests: 242 passed, 1 portable-CI fixture ignored
Explicit local WSL interop fixture: PASS
shared WSL crate tests: 34 passed
cargo check: PASS
cargo clippy --all-targets -- -D warnings: PASS
cargo fmt --all --check: PASS
frontend tests: 7 files, 47 passed
frontend production build: PASS
full workspace cargo test: PASS
full workspace cargo check: PASS
full frontend workspace build (19 projects): PASS
GitHub Actions CI: pending
git diff --check: PASS
```

The ignored fixture was run explicitly from the local WSL environment through the installed
`wsl.exe`/Ubuntu distribution. It proved that `--exec` preserves a semicolon-bearing argument as one
argv value, `setsid --wait` keeps the supervisor attached until output and exit are collected, and no
shell side-effect file is created. This is local interop evidence; it does not imply packaged Windows
execution or installation acceptance.

## Pending physical Windows validation

No packaged native Windows execution was tested for this workthrough. Physical acceptance remains
required for local Windows projects and WSL UNC projects, including spaces/Unicode, case-sensitive path behavior,
stopped or missing distros, portable and installer packages, direct process argv behavior, Job Object
and WSL process-group cleanup, source replacement between preview/trust/spawn, and absence of owned
process residue after stop, timeout, or cancellation.

PR2 acceptance is also pending for shell-risk confirmation and execution, matcher/diagnostic and Code
Pad integration, dependency DAG orchestration, and Workbench start/stop receipt provenance.
