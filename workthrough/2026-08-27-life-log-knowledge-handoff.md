# Life Log to Knowledge Draft Handoff

## Overview

Issue #307의 `knowledge-draft/v1` 네이티브 handoff를 구현했다. Life Log는 검증된
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

## Code Examples

### Preview before save

```rust
// apps/knowledge-base/src-tauri/src/commands/handoff.rs
let claim = store.claim(&id, EXPECTED_KIND, CONSUMER_APP, now_ms)?;
let payload = handoff::parse_claim(&claim)?;
pending_slot.put_if_empty(ClaimedKnowledgeDraft { claim, payload });
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

## Next Steps

- Parent agent performs core review, rebase, and squash before any PR workflow.
- Verify Windows W2 cold/hot receiver, packaged launch, expiry/regenerate smoke, and final CI.
- Implement persistent handoff status and explicit regenerate history only in follow-up #353.
