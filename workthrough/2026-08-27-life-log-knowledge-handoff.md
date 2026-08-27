# Life Log to Knowledge Draft Handoff

## Overview

Issue #307의 `knowledge-draft/v1` 네이티브 handoff를 #315 Webhook Lab → API Playground와
하나의 cohesive app-handoff PR로 구현했다. 각 payload schema와 acceptance는 분리하되,
공용 claim/preview/ack/restore·TTL·privacy 경계를 같은 checkpoint에서 검증한다. Life Log는 검증된
digest를 aggregate-only payload로 축소해 opaque applink descriptor와 함께 Knowledge를
실행하고, Knowledge는 claim 후 미리보기만 먼저 보여 준다. 사용자가 명시적으로 저장할
때에만 Journal 파일·검색 인덱스를 만들고 applink를 acknowledge/delete한다.

## Context

- #284의 protocol v2 one-time store와 #306의 local digest를 stacked base로 사용했다.
- 설계 §2.3/§4의 preview-before-save, claim lease, TTL, restore, consume/delete 흐름을
  앱 간 직접 DB 접근 없이 연결해야 했다.
- handoff 경계에는 세션, 창 제목, raw credential, Git project path를 포함할 수 없으며,
  source failure는 전체 digest나 다른 source를 숨기지 않아야 한다.
- persistent pending/sent/consumed/expired 상태는 후속 #353 범위로 남겼다.

## Changes Made

### 1. Strict, bounded draft contract

- `apps/life-log/src-tauri/src/core/handoff.rs`와
  `apps/knowledge-base/src-tauri/src/core/handoff.rs`에 동일한 app-local wire schema를
  두고 schema/version, 날짜·기간, source 순서·provenance, deterministic Markdown body를
  fail-closed로 검증했다.
- payload는 최대 768 KiB, body 512 KiB, title 256 bytes, source 4개로 제한하고
  aggregate summary와 고정 source metadata만 전달한다.
- `apps/knowledge-base/src-tauri/tests/fixtures/knowledge-draft-v1.json`을 checked-in
  wire fixture로 추가했다.

### 2. Native producer and receiver lifecycle

- `apps/life-log/src-tauri/src/commands/handoff.rs`는 Windows에서 설치 여부를 먼저
  확인한 뒤 digest를 재생성·검증하고, pending store에는 payload를, argv에는 opaque id와
  kind만 남긴다. browser/non-Windows에서는 publish와 launch를 하지 않는다.
- Knowledge의 `preview_knowledge_draft`, `save_knowledge_draft`,
  `discard_knowledge_draft`, `renew_knowledge_draft` 명령은 process-local claim token을
  frontend에 노출하지 않는다.
- 저장은 만료 시 파일을 만들지 않고, `Journal/YYYY-MM-DD-life-log-<period>.md`에
  exclusive temp-write/hard-link와 최대 100개 suffix를 사용한다. index 실패나 파일 실패는
  claim을 restore하며, 성공 시 index 후 ack/delete한다. ack 자체가 실패한 경우에는
  저장 결과를 성공으로 유지하고 `handoffDeleted=false`로 보고한다.
- `apps/knowledge-base/src/App.tsx`, `apps/life-log/src/App.tsx`와 API/routing/CSS에
  explicit preview/save/cancel UI, 30초 lease renewal, fixed safe errors, browser
  disable을 연결했다.

### 3. Catalog, docs, and fixtures

- `apps/catalog.json`, devbox-manager catalog projection, catalog tests와 Cargo dependency에
  handoff capability를 등록했다.
- Life Log/Knowledge README, `docs/architecture.md`, `docs/roadmap.md`, 설계 §1.5/§4와
  v0.5 plan P2-01/P2-10를 현재 구현과 후속 #353 경계에 맞춰 갱신했다.
- `apps/knowledge-base/src/App.applink.test.tsx`,
  `apps/life-log/src/App.contextMenu.test.tsx`에 cold preview/save/cancel과 browser
  disable fixture를 추가했다.

## Follow-up hardening (2026-08-27)

The review pass tightened the boundaries that are easy to miss in a happy-path
handoff:

- Added `core::vault::VaultIdentity` and identity-bound entry operations. A
  Knowledge preview captures the configured canonical root and filesystem
  marker; save revalidates it before resolving `Journal`, before publication,
  and after publication. Handoff preview/save now read only an explicitly
  configured existing root, so receiving an applink cannot initialize the
  default root or create `Journal` as a side effect.
- Reworked draft note publication to flush a private temporary sibling and
  use no-replace linking/move semantics. SQLite indexing runs in one explicit
  transaction before applink acknowledgement. On index failure, cleanup is
  allowed only when the captured file identity still matches; root replacement
  and reparse/symlink components fail closed with fixed messages.
- Made the shared handoff claim immutable from the consumer's point of view:
  acknowledgement rejects a managed claim record whose envelope metadata or
  payload differs from the envelope captured at claim time. Directory
  publication failures are not reported as durable success.
- Bounded the Knowledge watcher with a 4,096-event sync queue, 128 paths/4 KiB
  per event, a 4,096 unique path debounce map, identity/path checks, regular
  UTF-8 document filtering, and a 10 MiB bounded reader. Dropped burst events are retained in a second
  bounded path set for worker delivery, and watcher root resolution no longer
  creates a default root.
- Completed credential/path checks for both producer and receiver metadata,
  including Windows traversal, URL user-info, assignment-shaped secrets,
  private-key markers, and common client/session key names.
- Hardened the draft modal with UTF-8 title/body byte counters, explicit
  `aria-describedby`, initial focus, Escape cancel, Tab trapping, focus
  restoration, request tokens, stale/expiry handling, and mounted guards for
  preview/save/renew/cancel and metadata refresh. Save still sends only the
  opaque handoff id; the preview bytes are never accepted back from the UI.
- Restores an active preview during orderly renderer teardown and explicitly
  restores a claim whose preview IPC response arrives after unmount, rather
  than waiting for lease recovery. Producer preflight now requires the exact
  installed `handoff:knowledge-draft/v1` capability, not only an executable.

### Follow-up focused verification

This pass intentionally did not rerun the high-load workspace/build gates while
the parent agent was running the DOCX final gate. Commands used the dedicated
Linux-native target cache, `CARGO_INCREMENTAL=0`, and `-j2`:

```text
    cargo check -p knowledge-base --lib                      passed
    cargo test -p knowledge-base vault --lib                  6 passed
    cargo test -p knowledge-base handoff --lib                 9 passed
    cargo test -p knowledge-base watcher --lib                 5 passed
cargo test -p knowledge-base debouncer --lib               1 passed
cargo test -p knowledge-base safe_relative_path_rejects_directories_links_and_outside_paths --lib
                                                             1 passed
cargo test -p knowledge-base overflow_path_buffer --lib      1 passed
cargo test -p knowledge-base bounded_reader --lib            1 passed
cargo test -p applink handoff --lib                        16 passed
cargo test -p life-log handoff --lib                        3 passed
pnpm --filter knowledge-base exec vitest run \
  src/App.applink.test.tsx --maxWorkers=2                  10 passed
```

Full workspace checks, Windows W2 packaged launch, and parent-branch rebase
remain release gates.

## Code Examples

### Preview before save

```rust
// apps/knowledge-base/src-tauri/src/commands/handoff.rs
let claim = store.claim(&id, EXPECTED_KIND, CONSUMER_APP, now_ms)?;
let payload = handoff::parse_claim(&claim)?;
let configured_root = resolve_configured_root(&connection)?;
let vault = VaultIdentity::inspect(&configured_root)?;
pending_slot.put_if_empty(ClaimedKnowledgeDraft { claim, payload, vault });
// No file or index mutation occurs until save_knowledge_draft is called.
```

### Aggregate-only producer boundary

```rust
// apps/life-log/src-tauri/src/core/handoff.rs
let payload = KnowledgeDraftPayload {
    schema_version: 1,
    title,
    body: render_body(&summary, &sources),
    tags,
    summary,
    sources,
};
validate_knowledge_draft(&payload)?;
```

## Verification Results

All Rust commands use `CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-issue307`,
`CARGO_INCREMENTAL=0`, and `-j2`; frontend tests use `--maxWorkers=2`.

```text
cargo fmt --all                                      passed
cargo test -p life-log -p knowledge-base -j2          132 passed; 0 failed
pnpm --filter life-log test -- --maxWorkers=2         42 passed; 0 failed
pnpm --filter knowledge-base exec vitest run \
  src/App.applink.test.tsx --maxWorkers=2             6 passed; 0 failed
pnpm build                                            passed
cargo test -j2                                       workspace passed
cargo check -j2                                      workspace passed
git diff --check                                     passed
```

The Linux-native target cache measured approximately 26 GiB after the full workspace test
and check, and approximately 28 GiB after the attempted Windows-target dependency build
(`du -sh /home/jihoon/.cache/targets/devbox-issue307`). Windows W2 packaged-app
evidence remains a release/PR follow-up because it cannot be produced in the WSL environment.
An optional Windows GNU target check reached the native dependency build but could not continue
because the WSL image has no `x86_64-w64-mingw32-gcc`; this is an environment limitation, not a
test failure in the Linux workspace.

### Final grouped #307 + #315 checkpoint

The user-approved app-handoff PR groups this Life Log → Knowledge flow with
Webhook Lab → API Playground because both use the same bounded one-time
handoff lifecycle. After the final review, the complete affected set passed:

```text
Rust affected packages                             477 tests passed
API Playground frontend                            183 tests passed
Webhook Lab frontend                                51 tests passed
Life Log frontend                                   47 tests passed
Knowledge Base frontend                             74 tests passed
Four affected production builds                    passed
strict clippy, cargo check, rustfmt                 passed
catalog, dependency policy/notices, manifest tests passed
git diff --check                                   passed
```

The final frontend regression set covers native-only publication, current
digest reconstruction, duplicate-click single-flight behavior, preview-before-
save messaging, post-transaction reread failure without stale editor content,
and modal Escape timing. Windows packaged launch remains W2 evidence.

The first Windows CI compile exposed a target-gated call-site mismatch after
the digest builder gained a generation cancellation token. The handoff now
enters the same digest single-flight guard, forwards its generation token, and
checks cancellation both after aggregation and immediately before publication.
The remediation passed the focused Life Log check, strict clippy, rustfmt, and
all 89 Life Log Rust tests before the Windows CI rerun.

The following Linux CI run also exposed a filesystem-dependent test assumption:
an unleased, deleted empty directory can receive the same inode immediately
when recreated. The implementation was strengthened instead of only relaxing
the test: Knowledge now holds an open lease on the validated Journal directory
through temporary-file creation and no-replace publication, preventing Unix
inode or Windows file-index reuse during the race window. The deterministic
identity test and all 112 Knowledge Rust tests, check, strict clippy, and
rustfmt passed before the final CI rerun.

## Next Steps

- Verify Windows W2 cold/hot receiver, packaged launch, expiry/regenerate smoke, and final CI.
- Implement persistent handoff status and explicit regenerate history only in follow-up #353.
