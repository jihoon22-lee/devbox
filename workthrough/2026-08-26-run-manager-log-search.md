# Run Manager 로그 검색·source contract (#311)

## Overview

Run Manager의 기존 app-owned 회전 로그와 bounded `tail_log` 경계를 재사용해 선택한
run의 stdout/stderr를 검색할 수 있는 dirty draft를 작성했다. literal 검색을 기본으로
하고 regex는 명시적으로 opt-in하며, level/source/time 필터와 stream·line navigation을
제공한다. 검색 결과는 로그 원문을 복제하지 않고 `log-source/v1` source identity와
line metadata만 반환한다.

## Context

- 로그 본문은 기존 설계대로 SQLite에 저장하지 않고 `logs/runs/<run_id>/` 회전 파일에만
  남아 있었다. 새 기능도 이 경계를 넘지 않아야 했다.
- running writer가 전체 검색 때문에 장시간 대기하지 않도록 256 KiB cursor read와
  chunk 사이 async yield가 필요했다.
- 사용자 query, regex compile 오류, 파일 경로, credential이 UI 오류나 결과 payload로
  반향되지 않아야 했다.
- Log Lens는 아직 별도 P3 앱이므로 이번 작업은 local source contract validation만
  구현하고 producer/receiver handoff·remote ingest·permanent archive는 제외했다.

## Changes Made

### 1. Bounded search core

Files:

- `apps/run-manager/src-tauri/src/core/log_search.rs`
- `apps/run-manager/src-tauri/src/core/mod.rs`
- `apps/run-manager/src-tauri/Cargo.toml`
- `Cargo.lock`

Added `LogSearchRequest`, `LogSearchMatch`, `LogSearchResponse`, `LogLevel`,
`LogSearchMode`, and `LogSourceRef` types. The core validates opaque run IDs, control-free
UTF-8 queries, reversed time ranges, and exact `log-source/v1` identities. It scans sources
in fixed stdout-then-stderr order and records line number, source, level, and timestamp in a
deterministic order.

The bounds are explicit and shared with the command contract:

```rust
pub const MAX_QUERY_BYTES: usize = 512;
pub const MAX_SCAN_BYTES_PER_STREAM: usize = 4 * 1024 * 1024;
pub const MAX_TOTAL_SCAN_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SCAN_RECORDS: usize = 50_000;
pub const MAX_RECORD_BYTES: usize = 16 * 1024;
pub const MAX_RESULTS: usize = 500;
```

Literal mode uses `str::contains`. Regex mode uses the Rust `regex` crate with a compile
size/DFA budget, which avoids a backtracking engine and protects nested-quantifier scans.
The result intentionally contains no matching line text. Existing bounded log display remains
the only UI surface that renders raw log bytes.

### 2. Read-only Tauri command and source contract

Files:

- `apps/run-manager/src-tauri/src/commands.rs`
- `apps/run-manager/src-tauri/src/lib.rs`

Added `search_run_logs`. It verifies the run through SQLite, resolves only the existing
app-owned relative log directory, reads each selected stream through `tail_log`-compatible
256 KiB chunks, and calls the pure search core. It yields between chunks and retries once with
fresh segment metadata when rotation makes a cursor stale; repeated read failures become the
fixed `log-search-read-failed` error.

No search operation writes a history row, telemetry event, remote request, or archive. The
source reference is generated as `run-manager:<opaque-run-id>:<stdout|stderr>` and validated
against `log-source/v1`; it has no path, command, environment, secret, or remote address.

### 3. RunHistory UI, API, and accessibility

Files:

- `apps/run-manager/src/types.ts`
- `apps/run-manager/src/api.ts`
- `apps/run-manager/src/components/RunHistory.tsx`
- `apps/run-manager/src/components/RunHistory.test.tsx`
- `apps/run-manager/src/App.css`
- `apps/run-manager/src/App.test.tsx`

Added explicit search form controls for query, literal/regex mode, stdout/stderr source, and
level. Existing history date boundaries are passed as half-open epoch-millisecond search
bounds. Search starts only on button/submit/non-composing Enter, and clear removes query,
results, and selection metadata.

Results expose status, previous/next navigation, result buttons, stream selection, and active
line highlighting. A result outside the current 1 MiB/DOM display window is announced as
outside the current screen range without attempting an unsafe file read.

The UI also validates the bounded response and exact source IDs before rendering metadata.
Search state uses a generation token, mounted guard, and busy ref to ignore stale/unmounted
responses and reject duplicate submissions. IME Enter is prevented from submitting while
composition is active. Search errors are mapped to fixed Korean messages and never display
the raw Tauri error.

## Code Examples

### Fixed source identity and bounded request

```rust
// apps/run-manager/src-tauri/src/core/log_search.rs
pub const MAX_QUERY_BYTES: usize = 512;
pub const MAX_SCAN_BYTES_PER_STREAM: usize = 4 * 1024 * 1024;
pub const MAX_TOTAL_SCAN_BYTES: usize = 8 * 1024 * 1024;

pub fn source_ref(run_id: &str, stream: LogStream) -> Result<LogSourceRef, LogSearchError> {
    let source_id = format!("run-manager:{run_id}:{}", stream.as_str());
    // Only the opaque run/stream identity is exposed; no path or log text is
    // included in the handoff-shaped reference.
    Ok(LogSourceRef {
        kind: LOG_SOURCE_KIND.to_string(),
        source_id,
        run_id: run_id.to_string(),
        stream,
    })
}
```

### Stale-result guard in the UI

```tsx
// apps/run-manager/src/components/RunHistory.tsx
const generation = ++searchGeneration.current;
const response = await searchRunLogs(run.id, options);
if (!mountedRef.current || generation !== searchGeneration.current) return;
setSearchResponse(response);
```

### 4. Documentation

Files:

- `apps/run-manager/README.md`
- `docs/architecture.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-12-run-manager-design.md`
- `docs/superpowers/specs/2026-08-17-app-interop-design.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
- `THIRD_PARTY_NOTICES.md`
- `workthrough/2026-08-26-run-manager-log-search.md`

Documented the request/response shape, literal-first behavior, regex safety, source/level/time
semantics, all bounds, stale rotation handling, privacy boundary, UI navigation, and explicit
Log Lens non-scope. The native-first plan and roadmap now identify #311 as an implementation
draft while retaining the later Log Lens integration item.
The `regex` package was already present in the locked inventory; adding the Run Manager's direct
dependency edge required no new notice row, but the generated `Cargo.lock` digest was refreshed.

## Root PR-Boundary Review Findings

1. **Unknown-field smuggling**: the initial serde boundary ignored fields not represented by the
   Rust DTO. Both `LogSearchRequest` and `LogSourceRef` now use `deny_unknown_fields`; fixtures
   prove that an injected `absolutePath` is rejected before validation or filesystem access.
2. **WebView integer precision**: epoch milliseconds were represented as `i64`, although a JSON
   number can lose precision in JavaScript. Request bounds outside `±9,007,199,254,740,991` are
   rejected and parsed/fallback response timestamps outside that range are omitted.
3. **Async executor occupancy**: chunk reads yielded to writers, but retained-segment metadata
   reconstruction and the final bounded 8 MiB regex/text scan were still synchronous inside the
   Tauri async command. Both operations now run through `spawn_blocking`; the app-owned cursor
   reader continues to release its per-stream lock and yield between chunks.
4. **Fixed failure surface**: blocking task join, source reopen, regex compile, and path failures
   remain fixed codes. No query, log line, absolute path, OS error, command, environment value, or
   credential is reflected through the new failure paths.
5. **Visible truncation/range status fixture**: a frontend assertion expected the result count to
   be the complete text node, although the accessible status deliberately appends “현재 화면 범위 밖”.
   The fixture now matches the stable count portion and continues to exercise the visible warning.

## Verification Results

### Rust formatting and diff checks

```text
cargo fmt --manifest-path apps/run-manager/src-tauri/Cargo.toml --check  PASS
git diff --check                                                       PASS
dependency policy / regression / build-manifest checks                PASS
```

### Run Manager Rust tests

```text
cargo test -p run-manager --lib
166 passed; 0 failed
```

The focused search suite covers literal-vs-regex behavior, fixed regex errors, nested
quantifier shape, level/source/time filters, fallback run time, source identity validation,
strict unknown-field rejection, JavaScript-safe timestamps, line/result bounds, deterministic
ordering, and non-reflection of unsafe input. A command-level fixture also verifies a bounded
search reader yields to a concurrent writer.

### Frontend checks

The exact draft was synchronized into the existing disposable Linux-native frontend mirror,
preserving its cached dependencies without installing anything in the worktree:

```text
pnpm --filter run-manager test
6 test files passed; 37 tests passed

pnpm --filter run-manager build
tsc && vite build  PASS
```

### PR-wide gates after rebase onto `c61d661`

The direct `getrandom` edge merged by #419 and this feature's direct `regex` edge were both
preserved in `Cargo.lock`; `THIRD_PARTY_NOTICES.md` was regenerated before the final gates.

```text
cargo test --workspace -j4                              PASS
cargo check --workspace -j4                             PASS
cargo clippy --workspace --all-targets -j4 -- -D warnings PASS
cargo fmt --all -- --check                              PASS
pnpm test                                                PASS (17 frontend projects)
pnpm build                                               PASS (17 frontend projects)
dependency policy / regression / build-manifest          PASS
catalog consistency / changed-scope / git diff --check   PASS
```

The first repository-wide frontend run overlapped the full Rust link and exposed a pre-existing
Knowledge Base async timing assertion once. With the Rust load complete, the unchanged Knowledge
Base suite passed three consecutive isolated runs (7 files/30 tests each), then passed again in
the complete frontend suite. No out-of-scope Knowledge Base code was changed.

Windows packaged smoke remains the W2 checkpoint; CI's Windows compile gate is required before merge.

## Remaining Risks / Follow-up

- Windows W2 packaged-runtime evidence remains pending; Linux repository-wide gates are complete.
- Search line numbers are 1-based within the currently retained stream snapshot. Rotation can
  remove earlier lines, so the UI explicitly treats them as retained-snapshot coordinates.
- Level/timestamp parsing is intentionally best-effort for conventional prefix formats; a later
  structured log contract may extend it without changing the bounded/privacy boundary.
- `log-source/v1` is validated locally but is not yet handed to Log Lens. Producer/consumer
  claim/ack and any cross-app log view remain a separate integration task.
