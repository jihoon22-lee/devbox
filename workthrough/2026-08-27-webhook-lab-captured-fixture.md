# Webhook Lab captured fixture 저장 (#314)

## Overview

Webhook Lab이 수신한 request history를 사용자가 재사용할 수 있는 masked fixture로 저장하도록
구현했다. fixture는 Webhook Lab의 app-local JSON 파일 하나에만 저장하며, 저장 경계에서 다시
검증·redaction하고 corrupt/oversized/link-backed 파일과 동시 수정을 fail-closed한다.

이번 작업은 response-rule 초안을 Webhook Lab editor에 채우는 local action까지 포함한다. API
Playground `api-request/v1` handoff(#315)와 request replay/response sequence(#362)는 구현하지
않았다.

## Context

기존 앱은 request history와 response rule을 process memory에 유지했지만 captured request를
재사용할 durable fixture 저장소가 없었다. 원본 header vault·request body·request target이
앱 데이터나 UI로 새어 나가지 않도록, frontend가 경로나 raw request를 전달하지 않고 backend가
opaque history ID에서 masked snapshot을 가져오는 구조를 사용했다.

## Changes Made

### 1. Bounded masked fixture core

File: `apps/webhook-lab/src-tauri/src/core/fixtures.rs`

- `fixtures.json` 단일 app-local 파일과 schema v1 문서를 추가했다.
- fixture 200개, 파일 8 MiB, method/target/header/body/timestamp bounds를 저장·로드 양쪽에
  적용했다. 파일 read도 최대 1바이트 초과까지만 읽어 oversize를 감지한다.
- `Authorization`, `Cookie`, API key, token/secret/password/auth 계열 header와 JSON/text의
  credential marker를 `[REDACTED]`로 치환했다.
- absolute URL, dot-segment/path traversal, backslash, malformed percent encoding,
  token-shaped path는 `/[REDACTED_PATH]`로 바꾸고 안전한 query만 보존한다.
- fixture ID는 `fixture-<positive number>`만 허용하고, timestamp는 JavaScript `Date`가
  안전하게 표현할 수 있는 범위로 제한했다.
- corrupt, unknown schema, oversize, symlink/non-file 저장소를 자동 복구하지 않고 고정 오류로
  중단한다. atomic replace, raw-byte CAS, process-local write lock으로 partial write와
  competing writer overwrite를 막는다.
- 저장 경로는 absolute·clean path만 허용하며 root부터 각 기존/신규 parent component를
  개별 검증한다. symlink·Windows reparse point·non-directory ancestor를 따라가지 않고,
  누락된 app-owned directory도 한 단계씩 생성한 뒤 다시 검사한다.
- 목록은 capture timestamp 내림차순과 ID tie-break로 정렬하며, validated fixture에서 method/path
  만 사용하는 local response-rule draft를 생성한다.

핵심 흐름은 다음과 같다.

```rust
let request = state.history.lock()?.masked_record(history_id)?;
let fixture = fixture_from_request(fixture_id, &request)?;
save_document_if_current(&path, loaded.raw.as_deref(), &document)?;
```

### 2. Command/server integration

Files: `apps/webhook-lab/src-tauri/src/commands.rs`, `src/lib.rs`, `Cargo.toml`, `Cargo.lock`

- `list_fixtures`, `save_fixture`, `delete_fixture`, `clear_fixtures`, `fixture_to_rule` Tauri
  commands를 등록했다. save command는 `historyId`만 받고 body/header/path를 받지 않는다.
- fixture command 전체를 app-local path와 fixture mutex로 보호하고, 오류는 path/OS/secret 원문이
  없는 고정 메시지로 변환했다.
- server bind는 loopback 또는 명시적으로 확인한 LAN bind만 허용하고, request body는 읽기 전
  1 MiB byte cap과 120 requests/sec admission cap을 적용한다.
- rate window의 wall clock이 역행하면 future-dated admission 기록을 버리고 새 bounded window를
  시작해, 시스템 시간 보정 뒤 listener가 장기간 잠기는 현상을 막는다.
- `filesystem::atomic_write`를 app-owned fixture file에 연결하고 tempfile 기반 core tests를
  추가했다.

### 3. UI/API and safety UX

Files: `apps/webhook-lab/src/App.tsx`, `src/api.ts`, `src/App.css`, `src/lib/contextMenus.ts`

- history context menu와 각 history row에 `masked fixture 저장` action을 추가했다.
- Fixtures panel에서 deterministic list, local response-rule draft, per-item delete, confirmed
  clear를 제공한다. fixture time formatting은 invalid date도 renderer exception으로 만들지 않는다.
- fixture save/draft/delete/clear는 기존 shared busy guard를 사용해 double action을 차단하고,
  각 button에 stable accessible label을 제공한다.
- LAN 공개 시작은 명시적 확인을 한 번 더 요구한다. API Playground handoff menu는 disabled로
  유지한다.
- browser mock도 fixture list/save/delete/clear/draft 흐름과 deterministic ordering을 지원한다.

### 4. Tests and documentation

Files: `apps/webhook-lab/src-tauri/src/core/history.rs`, `src/App.test.tsx`,
`src/lib/contextMenus.test.ts`, `apps/webhook-lab/README.md`, `docs/architecture.md`,
`docs/roadmap.md`, `docs/windows-guide.md`, `docs/superpowers/specs/2026-08-14-webhook-lab-design.md`,
`docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

- Rust fixture tests cover masking, unsafe target redaction, bounds, timestamp/ID validation,
  corrupt/oversized preservation, store/parent symlink rejection, relative-path rejection, atomic
  output, deterministic sorting and competing CAS writers. History tests also cover rate-window
  recovery after a backwards wall-clock adjustment.
- Frontend tests cover fixture action labels, masking help, local rule draft, confirmation gates,
  shared busy guard/double action, safe error rendering and LAN confirmation.
- README/spec/roadmap/architecture/storage guide에 app-local `fixtures.json`, redaction, bounds,
  failure behavior와 #315/#362 scope를 기록했다.

## Verification Results

The final root review rebased the single feature commit onto latest main `72545cd` (#429), then
repeated the focused gates below from the dedicated worktree with
`CARGO_INCREMENTAL=0`, `CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-issue314`, and Cargo
parallelism capped at `-j2`.

### Rust

```text
cargo test -p webhook-lab --all-targets -j2
33 passed; 0 failed

cargo check -p webhook-lab -j2
Finished dev profile

cargo clippy -p webhook-lab --all-targets -j2 -- -D warnings
Finished dev profile

cargo fmt --all -- --check
passed
```

### Frontend

```text
pnpm --dir apps/webhook-lab test
Test Files 4 passed; Tests 49 passed

pnpm --dir apps/webhook-lab build
vite build: 42 modules transformed; build completed successfully
```

`git diff --check` also passed. Full workspace gates and Windows packaged smoke were intentionally
not run for this focused issue work. The dedicated Cargo target currently uses approximately `4.2G`
of disk (`du -sh /home/jihoon/.cache/targets/devbox-issue314`).

## Next Steps

- Implement the separately scoped `api-request/v1` handoff in #315.
- Implement replay/response sequence behavior in #362.
- Run Windows packaged acceptance and full workspace gates at the release checkpoint.
