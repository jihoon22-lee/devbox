# Devbox Manager Related Tools (#365)

## Overview

Audited and completed the Devbox Manager Related Tools feature from issue
#365/P3-17 in its dedicated feature worktree. Manager now exposes a small,
reviewed list of optional developer tools with official and license links, local
installed detection, an explicitly confirmed exact WinGet install, and direct
launch of detected tools. The remediation pass kept the feature native-first:
it removed a PATH lookup→spawn redirect window, bounded filesystem probes,
added process-tree ownership for WinGet, isolated concurrent actions, and
prevented untrusted native responses/errors from reaching the UI.

The implementation stays separate from the devbox app catalog and applink
contracts. It does not add package search, automatic updates or removal, native
action replacements, arbitrary paths, arbitrary commands, or external state
changes without confirmation. The feature commits and follow-up review changes
were integrated with the existing Data Inspector/support-bundle UI during the
final rebase and preserved as independent Manager capabilities.

## Context

- Base: `main` / `4a1a61a746ef03b7f6f9c548e75e81d2df32064c`
- Worktree: `/mnt/e/projects/devbox-worktrees/devbox-manager-related-tools`
- Branch: `feat/devbox-manager/related-tools`
- Issue: `https://github.com/jihoon22-lee/devbox/issues/365`
- Curated tools: PowerToys, Windows Terminal, Visual Studio Code, Bruno,
  DBeaver Community, DB Browser for SQLite, GitHub Desktop, Podman Desktop,
  and Docker Desktop.

The issue acceptance was reviewed from GitHub before remediation: implement the
P3-17 curated official URL/license, installed detection, confirmed WinGet
install, and launch; prove that secrets/raw credentials/unsafe paths and
unintended external mutations do not cross the feature boundary; keep
reproducible failure fixtures without duplicating separately tracked work; and
retain offline/no-WinGet/confirmation plus Windows packaged/W3 evidence as
release gates. The explicit non-scope remains package-manager search,
automatic update/uninstall, and replacing Manager-native actions.

## Changes Made

### 1. Curated metadata and safe native commands

Files:

- `apps/devbox-manager/src-tauri/src/core/related_tools.rs`
- `apps/devbox-manager/src-tauri/src/commands/related_tools.rs`
- `apps/devbox-manager/src-tauri/src/core/mod.rs`
- `apps/devbox-manager/src-tauri/src/commands/mod.rs`
- `apps/devbox-manager/src-tauri/src/lib.rs`

The Manager-local catalog contains only static display metadata, official HTTPS
URLs, license information, exact WinGet IDs, and fixed executable names. IDs are
lowercase opaque identifiers with a byte bound and are revalidated at every
command boundary.

`related_tools` performs bounded read-only detection. Windows probes directly
walk at most 128 PATH entries (each at most 4 KiB and the complete PATH at most
32 KiB), then check fixed local-drive installation layouts. Each candidate must
be a regular non-reparse file and every existing parent must also be a plain
directory. The OS-owned `%LOCALAPPDATA%\\Microsoft\\WindowsApps` `wt.exe` and
`winget.exe` application aliases are the only fixed-name reparse exception.
A fixed executable name is resolved to an explicit path and checked again
immediately before launch; Manager never checks a name and then lets a changed
PATH choose a different process. Only `path`, `known-location`,
`not-found`, or `unavailable` reaches the frontend; resolved paths, versions,
PATH contents, process output, and OS errors do not.

`install_related_tool` requires `confirmed: true`, serializes installs with a
single-flight lock, and invokes only:

```text
winget install --id <reviewed-id> --exact --source winget
  --accept-source-agreements --accept-package-agreements
```

The command is a direct `CreateProcessW` spawn with a bounded allowlisted
environment, no inherited handles, no shell, no user-provided argv, and a
120-second timeout. WinGet is
resolved from the OS-owned per-user WindowsApps alias, System32, or a bounded
PATH candidate under one of those same roots rather than trusting an arbitrary
earlier PATH entry or re-searching the name at spawn time. The roots come from
the Windows Known Folder/System Directory APIs, not spoofable `LOCALAPPDATA` or
`SystemRoot` environment strings. Its child
is created suspended, assigned to a Windows Job Object configured with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, and only then resumed. Assignment or
resume failure terminates and reaps the still-suspended process, closing the
earlier spawn-to-assignment escape window. Timeout, wait/query errors, and
Manager shutdown cannot silently leave an unowned installer tree. The command
queries `JOBOBJECT_BASIC_ACCOUNTING_INFORMATION.ActiveProcesses` until the
complete job becomes empty before publishing success; it does not incorrectly
treat a Job Object handle as a normal zero-process wait signal. Launch also accepts
only a curated ID, uses fixed executable names/layouts, rejects symlink/reparse
components, passes no frontend path or arguments, and discards child stdio.

Detection, install, and launch share one native try-lock. This makes an install
and an immediate refresh/launch observe a consistent executable set and returns
a fixed busy message instead of allowing overlapping external state changes.
The lock is held only around the bounded read/probe or process lifecycle, so it
does not serialize unrelated Manager app operations.

Non-Windows builds report `unavailable`/unsupported outcomes without spawning
external processes.

### 2. Related Tools frontend flow

Files:

- `apps/devbox-manager/src/types.ts`
- `apps/devbox-manager/src/api.ts`
- `apps/devbox-manager/src/App.tsx`
- `apps/devbox-manager/src/App.css`

Added a lazy-loaded Related Tools tab. Each card shows the curated name,
summary, coarse detection reason, WinGet ID, license, and safe official/license
links. Installation requires a per-tool browser confirmation and refreshes the
local detection result after success. Launch is shown only for a detected tool.
The API boundary validates the complete fixed catalog metadata, detection/installed
consistency, and unique IDs before React renders it. Action results must match
the requested tool and expected status; native-provided message text is replaced
with a fixed local message.

Official links are checked for HTTPS, no credentials/port, and an explicit
official-host allowlist. `openRelatedToolUrl` repeats the allowlist before using
the existing Tauri opener plugin (browser preview uses a safe new tab). Related
install/launch errors are mapped to a finite safe-message allowlist, so raw
WinGet diagnostics, local paths, account names, and credentials cannot become
global notice/error text. Mount and action generations guard late responses;
unmounted or superseded operations cannot resurrect busy/notice/error state.
The section uses native keyboard buttons, `aria-current`, a named region,
`aria-busy`, and polite status/alert announcements. Existing Manager app
actions remain the primary workflow.

### 3. Tests and documentation

Files:

- `apps/devbox-manager/src-tauri/src/core/related_tools.rs` (catalog, ID, and
  detection classification tests)
- `apps/devbox-manager/src-tauri/src/commands/related_tools.rs` (DTO shape,
  suspended-create/job-object process-tree policy, Windows UTF-16 argv quoting,
  no-path view, exact argv, action single-flight, bounded PATH resolution,
  outcome mapping, and non-Windows tests)
- `apps/devbox-manager/src/App.test.tsx` (links, URL rejection, confirmation,
  launch gating, and opener routing)
- `apps/devbox-manager/README.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

The README and P3-17 plan record the Manager-local ownership, native-first
boundary, exact WinGet contract, timeout/output handling, process-tree cleanup,
response validation, and explicit non-scope. `apps/catalog.json` and
`crates/applink` were intentionally left unchanged. `Cargo.toml` only enables
the already-pinned `windows` crate's Job Object/Threading API features; no new
runtime package or sidecar was introduced.

### 4. Remediation audit matrix

| Boundary | Finding in the dirty draft | Final boundary |
|---|---|---|
| Catalog/capability | Related Tools must not become installable devbox apps or applink targets | Static Manager-local catalog remains separate from `apps/catalog.json`; every action accepts only a reviewed opaque id |
| Install state | A `where.exe` result was discarded and the executable name was searched again at spawn time; environment strings alone cannot establish an OS-owned WinGet root | Bounded direct PATH resolution returns an explicit candidate, then repeats regular-file/non-reparse checks immediately before spawn; WinGet roots come from Windows Known Folder/System Directory APIs, with only the OS-owned `wt.exe`/`winget.exe` aliases trusted |
| Process safety | A root-only `kill` could leave a WinGet installer/helper alive; assigning only after an ordinary spawn left a helper-escape window; waiting on the Job Object handle does not mean the job is empty | WinGet is created suspended, assigned to a kill-on-close Job Object, then resumed; timeout, wait/query/assignment error, and app drop terminate the owned tree; success polls the authoritative active-process count to zero |
| Concurrency | Install-only locking did not cover launch or detection | Detection, install, and launch share a native try-lock and never overlap external tool state transitions |
| Resource bounds | One `where.exe` probe per executable could accumulate an unbounded refresh duration | PATH entries, entry length, total length, fixed catalog size, path depth, known path count, wait timeout, and post-termination reap are bounded |
| Privacy | Backend DTO was coarse, but future native error/message text could be rendered by React | stdio is null and DTOs omit paths/output; API validates exact metadata and action identity, while UI renders only fixed success/error messages |
| Stale UI | Late install/launch responses updated state without a generation guard | Mount and per-action generations guard all related action state; stale/unmounted responses are ignored; install refresh runs only for the current action |
| External links | URL checks existed only at the view/open boundary | HTTPS, no credentials/port, exact official host allowlist, and Tauri/browser opener routing remain duplicated at both API and render/open boundaries |
| Accessibility | Related Tools was keyboard reachable as native buttons but had little announced state | Native `type=button`, `aria-current`, named region, `aria-busy`, polite empty status, alert errors, and per-action `aria-busy` are explicit |

The audit intentionally did not add automatic update/uninstall, package search,
arbitrary installer URLs, arbitrary executable paths/arguments, shell execution,
or external tool configuration editing. Those remain outside #365's user-visible
feature boundary.

## Code Examples

### Opaque install request

```rust
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RelatedToolInstallRequest {
    pub tool_id: String,
    pub confirmed: bool,
}

let spec = validated_tool(&request.tool_id)?;
if !request.confirmed {
    return Err("관련 도구 설치는 사용자 확인이 필요합니다.".to_string());
}
```

### Frontend confirmation boundary

```tsx
if (!window.confirm(`'${tool.displayName}'을 WinGet으로 설치할까요?`)) return;
const result = await installRelatedTool(tool.id, true);
if (result.toolId !== tool.id || result.status !== "installed") {
  throw new Error("관련 도구 작업 결과가 올바르지 않습니다.");
}
```

The backend still validates the ID and confirmation flag, so the UI does not
control the package name, executable path, or process arguments. The API
normalizes `result.message` and the component maps unknown failures to a fixed
safe error before rendering them.

## Verification Results

Checks completed in the feature worktree (with one frontend worker to keep
resource use bounded):

```text
source ~/.cargo/env && cargo fmt --all -- --check PASS
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-manager-354 \
  cargo test -p devbox-manager -j1 PASS (96 tests)
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-manager-354 \
  cargo check -p devbox-manager --all-targets -j1 PASS
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-manager-354 \
  cargo clippy -p devbox-manager --all-targets -j1 -- -D warnings PASS
pnpm --dir apps/devbox-manager test -- --maxWorkers=1 --no-file-parallelism PASS (28 tests)
pnpm --dir apps/devbox-manager build PASS (tsc + Vite)
python3 .github/scripts/check-dependencies.py check PASS
bash .github/scripts/check-catalog.sh PASS
git diff --check PASS
```

The parent remediation review used the existing serialized Devbox Manager
target cache, never the concurrently shared workspace target. The PR's GitHub
Actions Windows job remains the compile/test gate for the `CreateProcessW`,
Job Object accounting, Known Folder, and System Directory bindings.

No Windows packaged/W3 smoke run was performed. Windows WinGet availability,
PATH/app-execution-alias behavior, standard installation layouts, Job Object
assignment, opener capabilities, and packaged process timeout behavior remain
CI/release-checkpoint risks. At that review checkpoint, the follow-up edits
were intentionally left for the parent agent to validate and commit.

## Follow-up audit (2026-08-28)

The final #365 review re-read the complete branch diff and repository
conventions, then checked the Windows-only process path against the stated
CreateProcessW suspended→Job assignment→resume, active-process polling, trusted
WinGet root, bounded environment, timeout, and tree-cleanup contract. The core
boundary was already present and was kept intact. Three small hardening and UX
gaps were addressed:

- `spawn_guarded_winget` now rechecks the resolved WinGet executable both at
  entry and immediately before `CreateProcessW`. This narrows the resolver →
  process-creation replacement window while preserving the fixed application
  name and argv contract; a failed check creates no unmanaged process.
- `isRelatedToolId` and the official-link validator now fail closed for
  JavaScript callers that pass non-string values. Related URL parsing is capped
  at 2 KiB in both API and render guards, so malformed or unbounded native/UI
  data cannot reach URL parsing or the opener.
- Related Tools UI, README, and the native-first plan now explicitly state the
  offline boundary: local detection and launching an already detected tool do
  not require network access; WinGet installation requires Windows App
  Installer and network; official/license links follow browser/network state;
  none of these optional failures disable Manager-native app operations. The
  frontend test asserts that this boundary remains visible and rejects an
  overlong official URL.

Focused revalidation after the follow-up changes:

```text
source ~/.cargo/env && cargo fmt --all -- --check PASS
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-manager-354 \
  RUSTFLAGS='-C metadata=devbox_manager_365_review' \
  cargo test --manifest-path apps/devbox-manager/src-tauri/Cargo.toml \
  related_tools --lib -j1 PASS (13 related-tools tests)
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-manager-354 \
  RUSTFLAGS='-C metadata=devbox_manager_365_review' \
  cargo check -p devbox-manager --lib -j1 PASS
pnpm --dir apps/devbox-manager test PASS (29 tests)
pnpm --dir apps/devbox-manager build PASS (tsc + Vite)
git diff --check PASS
```

An attempted local `x86_64-pc-windows-gnu` check used the required dedicated
target and `-j1`, but the environment has no `x86_64-w64-mingw32-gcc`; Cargo
stopped in the unrelated `aws-lc-sys` build before compiling the application.
No Windows cfg result is claimed from that attempt. CI/Windows remains the
authoritative check for Win32 bindings and packaged WinGet behavior.

## Final platform and API boundary review

The final parent review made platform support explicit in the native Related
Tools DTO. Windows returns `platformSupported: true`; non-Windows builds return
false. The frontend validates this boolean together with the fixed catalog and
rejects the impossible state of an installed tool on an unsupported platform.
Unsupported platforms now show a disabled `WinGet 설치: Windows 전용` control,
while official and license links remain usable on every platform when network
access is available. The screen copy no longer incorrectly describes those
links as Windows-only.

A separate frontend API-boundary suite now exercises the real validators rather
than mocking the complete API module: tampered catalog URLs, impossible platform
state, mismatched action IDs/status, arbitrary IDs, and unapproved URLs all
fail closed. Native action message text is replaced with the fixed UI message,
so an injected path or credential-like value cannot be rendered.

```text
pnpm --dir apps/devbox-manager test -- --maxWorkers=2 pass (2 files / 33 tests)
pnpm --dir apps/devbox-manager build                  pass
cargo fmt --all -- --check                           pass
git diff --check                                     pass
cargo test/check/clippy -p devbox-manager             pending after rebase:
                                                        stale branch launch/applink
                                                        API mismatch blocks compile
```

The Rust failure is a base-age mismatch (`launch::open_argv` still expects the
pre-main `applink::build_argv` return type), not a Related Tools compile result.
The parent must rebase onto the current main before claiming native gates.

## Post-rebase integrated validation

The parent rebased the candidate onto `main` at `4a1a61a` and resolved the
overlap with Devbox Manager Data Inspector/support bundle by retaining both
feature sets: command registrations, types, state/request guards, tests,
documentation, and CSS remain present. The rebase removed the stale
`launch`/`applink` API mismatch, and the complete affected package now compiles
and tests together.

The Windows suspended-process boundary was tightened once more: after
`CreateProcessW(CREATE_SUSPENDED)` and Job assignment, `ResumeThread` must
return an exact previous suspend count of one. Any other value terminates and
reaps the process instead of returning a potentially still-suspended or
unexpectedly resumed installer.

```text
CARGO_TARGET_DIR=.../devbox-manager-354 cargo test -p devbox-manager --lib -j2   PASS (123 tests)
CARGO_TARGET_DIR=.../devbox-manager-354 cargo check -p devbox-manager -j2        PASS
CARGO_TARGET_DIR=.../devbox-manager-354 cargo clippy -p devbox-manager \
  --all-targets -j2 -- -D warnings                                               PASS
cargo fmt --all -- --check                                                       PASS
git diff --check                                                                  PASS
pnpm --filter devbox-manager test                                                 PASS (2 files, 37 tests)
pnpm --filter devbox-manager build                                                PASS (tsc + Vite, 42 modules)
```

The worktree is committed and clean. GitHub Actions' Windows job remains the
authoritative Win32 compile/test gate, while actual packaged WinGet, launch,
offline, timeout, and opener behavior remains the explicitly unclaimed W3
release checkpoint.

## Windows CI lifetime remediation

The first pull-request CI run exposed a Windows-only borrow-check failure in
the trusted executable path comparison. The code borrowed a trimmed `str`
directly from a temporary `Cow<str>` returned by `Path::to_string_lossy()`.
Linux did not compile this `cfg(windows)` helper, so its otherwise complete
local gate set could not detect the lifetime error.

The helper now retains the `Cow<str>` in a local binding for the full duration
of the case-insensitive comparison. This is a lifetime-only correction: it
does not broaden accepted paths or change the path-boundary check. The Windows
compile job is rerun as the authoritative MSVC verification after the focused
local checks below.

```text
cargo fmt --all -- --check                                      PASS
git diff --check                                                PASS
cargo test -p devbox-manager --lib -j2                          PASS (123 tests)
cargo clippy -p devbox-manager --all-targets -j2 -- -D warnings PASS
cargo check -p devbox-manager --target x86_64-pc-windows-gnu    BLOCKED before
                                                                 app compile:
                                                                 MinGW GCC absent
```

The cross-target attempt reached the existing `aws-lc-sys` build script and
stopped because `x86_64-w64-mingw32-gcc` is not installed. It therefore does
not count as a Windows application compile result; the rerun MSVC CI job is the
required evidence for this correction.
