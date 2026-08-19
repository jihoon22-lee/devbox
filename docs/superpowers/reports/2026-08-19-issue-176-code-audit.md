# Issue #176 code audit — v0.4.1 readiness

Audit date: 2026-08-19. Target revisions: latest `main` with merged fixes #200, #201, #203, and #208, plus the RC-preparation branch `feat/workspace/v0.4.1-rc1`; reproduce the source audit by comparing that branch with `main`. Issue #176 was inspected read-only with:

```text
gh issue view 176 --repo jihoon22-lee/devbox --json number,title,state,body,comments
```

Issue #176 is OPEN. Its historical comment reports 9/60 checked (A=5, B=2, D=2); that is not current v0.4.1 runtime evidence. This audit did not edit the issue, source, specs, roadmap, versions, or existing docs.

## Decision labels

- **code/test-confirmed**: deterministic source/tests or a successful compile support the claim; this is not Windows runtime proof.
- **contradicted/bug**: current code, test expectation, or spec disagrees with the behavior required by the checklist.
- **Windows-runtime-only pending**: code may exist, but the checklist requires Windows, ConPTY/WebView2, packaged artifacts, or manual UI/network/data verification.
- **out-of-scope/deferred**: explicitly moved out of v0.4.1 or intentionally unimplemented.

## Commands and results

- `source ~/.cargo/env && cargo test -p wsl-desktop --all-targets`: 24 Rust tests passed. The parser consumes STATE/VERSION from the right and joins `Ubuntu 24.04` (`apps/wsl-desktop/src-tauri/src/core/parsers.rs:10-35,139-148`).
- `pnpm --filter wsl-desktop test`: 51 frontend tests passed, including the stopped-distro rendering and terminal contract regressions (`apps/wsl-desktop/src/components/DistroPanel.test.tsx`, `TermPane.test.tsx`).
- `source ~/.cargo/env && cargo test -p workbench`: 15 tests passed, including exact matching for `Ubuntu 24.04` (`apps/workbench/src-tauri/src/core/health.rs:31-101`).
- `source ~/.cargo/env && cargo check --workspace --all-targets`: exit 0.
- `pnpm build`: exit 0; confirms frontend compilation only, not Tauri/Windows behavior.
- `.github/scripts/check-catalog.sh`: exit 0. A read-only `rg` check finds the same baseline CSP in all 13 `apps/*/src-tauri/tauri.conf.json`; this does not prove zero DevTools violations.
- Focused pending-open tests pass for Code Pad, WSL Desktop, and Workbench; they verify listener-before-take and re-take delivery (`apps/code-pad/src/App.test.tsx`, `apps/wsl-desktop/src/App.applink.test.tsx`, `apps/workbench/src/App.applink.test.tsx`). Actual Windows single-instance forwarding remains pending.
- `session_command_keeps_cwd_as_a_separate_argument` passes and asserts the exact `wsl.exe -d Ubuntu --cd <cwd> --` argv with quoted-path content and no `bash`/`-lc` (`apps/wsl-desktop/src-tauri/src/commands/terminal.rs:133-144,406-430`). The local Windows-target cross-check was attempted but stopped before a target result because `llvm-rc` is unavailable. Subsequent PR #203 GitHub Actions [`Rust (Windows compile check)`](https://github.com/jihoon22-lee/devbox/actions/runs/32232332821/job/96004696235) completed its check, Clippy, and test stages successfully; this is CI evidence, not Windows runtime verification.

## Checklist mapping

| #176 area/checks | Category | Current code/test evidence and remaining proof |
|---|---|---|
| **A1-A5 release dry-run** | Windows-runtime-only pending | `.github/workflows/release.yml` defines the workflow, but no current `workflow_dispatch`, 13-app staging artifact/installer set, duplicate-tag failure, verify failure, or manifest/tag independence was run from this worktree. |
| **B1-B2 manager list/versions** | code/test-confirmed | `apps/devbox-manager/src/api.ts:6-18` has 12 `managerVisible` apps plus hidden self; `apps/devbox-manager/src/App.test.tsx:14-29` asserts 12 rows and distinct versions. UI execution remains Windows pending. |
| **B3-B9 install, retry, sha, rollback, force-close, new-install, doctor** | Windows-runtime-only pending | Pure guards/cleanup exist in `apps/devbox-manager/src-tauri/src/core/download.rs:3-109` and `commands/manager.rs:58-89`; doctor is implemented in `commands/doctor.rs:21-160`. Network interruption, real installers, registry/rollback, and PATH/environment behavior need Windows/package runs. |
| **C1-C4 identifier/session/data migration** | Windows-runtime-only pending | No Windows data fixture or two-run migration was executed. `apps/life-log/src-tauri/src/core/db.rs:148-` retains legacy absorption logic; data-preservation/idempotence must be checked against real `%LOCALAPPDATA%` data. |
| **D1 distro list/non-ASCII** | code/test-confirmed (Windows runtime pending) | Backend UTF-16 decoding and the spaces fixture pass (`apps/wsl-desktop/src-tauri/src/core/parsers.rs:102-148`). `DistroInfo.state` is wired through `apps/wsl-desktop/src/types.ts:28-33`, and `DistroPanel.test.tsx:23-48` confirms `Stopped` uses the non-running class. Actual Windows distro output remains pending. |
| **D2 Open Terminal in-app tab** | code/test-confirmed (Windows runtime pending) | `apps/wsl-desktop/src/App.tsx:219-235` creates a tab/pane and `src/lib/applink.ts:17-31` routes `Path`; `src/App.applink.test.tsx:46-65` covers listener-before-take and relaunch re-take. No Windows terminal run was made. |
| **D3 Docker list/start/stop/restart** | code/test-confirmed (Windows runtime pending) | `DistroPanel.tsx:71-128`, `src-tauri/src/commands/dashboard.rs`, and parser tests cover the action paths; Docker Desktop/WSL behavior and UI refresh need Windows. |
| **D4 tab/pane save/restore and ☰** | out-of-scope/deferred | `apps/wsl-desktop/src/lib/storage.ts:1-40` stores only pinned cwd/recent paths; it has no persisted tabs/panes/layout. The terminal spec explicitly assigns `PersistedLayout` to v0.5.0 (`docs/superpowers/specs/2026-08-17-wsl-desktop-terminal-design.md:333-356,472-485`). The ☰ toggle exists in `App.tsx:386-395`, but visual behavior remains Windows pending. |
| **D5 cwd spaces/non-ASCII and `Ubuntu 24.04`** | code/test-confirmed (Windows runtime pending) | `crates/wsl/src/argv.rs:13-52,65-114` preserves argv boundaries; `commands/terminal.rs:406-430` now proves the final `CommandBuilder` argv is `wsl.exe -d ... --cd ... --` and contains no `bash -lc`. The WSL parser and Workbench health tests cover spaced names; real Windows cwd launch remains pending. |
| **E1-E4 Everything+ watcher/search/open** | Windows-runtime-only pending | Debounce/event/index logic is present and tested (`apps/everything-plus/src-tauri/src/core/watcher.rs:7-103`; `crates/search` 6 tests), but filesystem mutation timing, overflow/restart recovery, keyboard/UI actions, and Windows paths were not run. |
| **F1-F4 Life Log** | Windows-runtime-only pending | Sessionizer/privacy/DB logic has Rust coverage (life-log suite 32 in the workspace run), but idle, lock/suspend, startup registry, and day-view behavior require Windows and real process/session fixtures. |
| **G1-G3 Knowledge Base editor/watch/search** | Windows-runtime-only pending | Markdown/DB/search code and tests pass (markdown suite 12; knowledge-base suite 16), but WebView editor shortcuts, external edit watcher, mermaid/images, and root-boundary UI were not manually exercised. |
| **H1-H4 Code Pad recovery/problems/navigation** | Windows-runtime-only pending | Rust code-pad suite 147 plus integration suites pass; `apps/code-pad/src/App.test.tsx` contains app-shell coverage. Recovery after process kill, LSP UI, keyboard navigation, and WebView rendering remain Windows/manual checks. |
| **I1-I6 Run Manager** | Windows-runtime-only pending | Workspace run-manager suite 152 and `crates/secrets` tests pass; snapshot/redaction code exists. Service/process lifecycle, WSL jobs with spaced paths, export/import UI, and sealed-environment upgrade need Windows fixtures. |
| **J1-J3 API Playground** | Windows-runtime-only pending | `crates/secrets/src/lib.rs:64-134` covers seal/unseal/masking and API request code has pure tests; secret-in-history/curl/error, collections, and environment substitution were not exercised in the desktop UI. |
| **K1 Knowledge Base snapshot** | code/test-confirmed (runtime pending) | Producer is `apps/knowledge-base/src-tauri/src/integration.rs:12-65`; integration snapshot tests pass. A real app run still must confirm file placement and that note bodies are absent. |
| **L1 profile CRUD/canonical key** | code/test-confirmed | `apps/workbench/src-tauri/src/core/profile.rs:62-104` and its dedup tests, plus `crates/wsl` canonical-path tests, cover Windows/WSL identity normalization. CRUD UI/runtime remains pending. |
| **L2 project_health** | code/test-confirmed (Windows runtime pending) | `apps/workbench/src-tauri/src/core/health.rs:31-52` parses the two rightmost columns and exact-matches the reconstructed distro name; `:76-100` covers spaced-name and UTF-16 decode behavior. Real distro/port/run-manager status still needs Windows. |
| **L3 Start Workspace** | code/test-confirmed (Windows runtime pending) | `commands/workspace.rs:308-435` and tests `526-573` cover WSL-path preference, Windows fallback, Code Pad workspace validation, health/port/open step construction. Actual app launch/ports are unverified. |
| **L4 Stop What I Started** | Windows-runtime-only pending | The run registry/stop path exists, but process ownership and Windows termination were not exercised. |
| **L5 life-log read-only absorption** | out-of-scope/deferred | Current code still directly opens `com.devbox.lifelog/data.db` (`workspace.rs:475-508`), while the app-interop spec explicitly defers snapshot/SQLite cleanup to v0.5.0 (`docs/superpowers/specs/2026-08-17-app-interop-design.md:345-371,401-405`). |
| **M1-M2 Webhook Lab** | Windows-runtime-only pending | History masking/rules are covered by 4 Rust tests; server port, LAN `0.0.0.0` warning, request capture, delay, and 404 behavior were not run on Windows. |
| **N1 scan/status** | code/test-confirmed (Windows runtime pending) | `apps/repo-manager/src-tauri/src/commands.rs:29-100` uses canonical dedup, ignored-dir pruning, depth/visit limits; Rust tests pass. A real large/junctioned Windows root remains pending. |
| **N2 worktree create/clean/open; remove** | mixed: Windows-runtime-only pending; out-of-scope/deferred for remove | Create/clean/open target code is in `commands.rs:125-169`, with target test at `177-195`; actual Git/launcher execution is pending. Remove is intentionally unimplemented and surfaced as such in `apps/repo-manager/src/App.tsx:64-72` and its test, matching the #176 comment. |
| **O1-O3 CSP/HMR/build/UI tokens** | Windows-runtime-only pending | CSP declarations and catalog script pass; `pnpm build` proves compilation. DevTools console zero, Tauri packaged executable, HMR, and visual regression require Windows/WebView2. |
| **P1-P6 prior code-review fixes** | code/test-confirmed (manual/runtime portions pending) | Current source/tests cover UTF-16 decode, scan pruning/depth, numeric launch version ordering (`crates/launch/src/lib.rs:115-163`), exact terminal argv/no `bash -lc`, distro state, and pending-open consumers. Focused pending-open tests pass; actual Windows single-instance/manual checks remain pending. |

## Bottom line

The v0.4.1 app-link/argv contract, target mapping, launch ordering, terminal carry/attach/session fixes, spaced-distro parsing/health, Running/Stopped UI, exact `CommandBuilder` argv, and deterministic Rust/frontend/build gates are supported by current code/tests. The earlier distro-state, spaced-name health, and `bash -lc` findings are resolved in the current tree. Tab/pane layout save/restore is not a missing v0.4.1 fix: the specs explicitly defer it to v0.5.0. Local Windows-target compilation was blocked by missing `llvm-rc`, but PR #203's GitHub Actions `Rust (Windows compile check)` passed its check, Clippy, and test stages. That CI evidence does not constitute Windows runtime verification; all remaining checklist observations requiring Windows, packaged installers, WebView2/ConPTY, real data, network failure, or visual inspection are Windows-runtime verification items.
