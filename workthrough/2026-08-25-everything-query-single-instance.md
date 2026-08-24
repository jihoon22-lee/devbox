# Everything+ Query Single-Instance Search

## Overview

P1-04-E [#242](https://github.com/jihoon22-lee/devbox/issues/242)의 Everything+
inbound Query 검색을 구현했다. catalog revision 3에서 Everything+가 `query`를 받도록 선언하고,
cold-start argv와 이미 실행 중인 instance 재호출을 같은 one-shot `PendingOpen` 경로로
frontend에 전달한다.

유효한 Query는 공백을 제거하고 1~512자로 제한한 뒤 Everything+의 name/non-regex 검색 상태에
적용한다. 기존 150ms 검색 pipeline과 FTS index를 그대로 사용하며, content index, root,
saved query 또는 semantic search 상태를 만들거나 변경하지 않는다. invalid/unsupported 요청과
parser 오류는 raw 입력을 반향하지 않는 고정 오류로 처리한다.

## Context

Everything+는 v0.4.x에서 catalog inbound capability가 비어 있었고 single-instance plugin도
없었다. 외부 devbox 앱이 `--query`를 전달해도 별도 process가 열릴 수 있었으며, 기존 instance의
검색 input으로 연결되는 경로가 없었다.

선행 #241에서 Knowledge Base가 검증한 listener-first delivery pattern을 동일하게 적용했다.

1. single-instance plugin을 DB/opener/setup보다 먼저 등록한다.
2. cold/hot argv를 모두 `crates/applink::parse_argv`로 해석한다.
3. request를 `PendingOpen`에 저장한다.
4. frontend는 event listener를 먼저 등록한 뒤 pending slot을 pull한다.
5. hot event payload는 trigger로만 사용하고 authoritative request는 slot에서 take한다.

## Changes Made

### 1. Catalog revision and contracts

- `apps/catalog.json`
  - `catalogRevision`을 2에서 3으로 증가시켰다.
  - `everything-plus.accepts`를 `query`로 선언했다.
- `crates/catalog/tests/catalog.rs`
  - repository revision 3을 고정했다.
  - query target 순서가 `everything-plus`, `knowledge-base`인지 검증한다.
- `apps/devbox-manager/src-tauri/src/core/catalog.rs`
  - Manager build-time adapter도 revision 3과 Everything+ Query capability를 검증한다.

Catalog 외 fixture의 revision 값은 runtime freshness와 v1/v2 오류 시나리오를 위한 독립
fixture이므로 변경하지 않았다.

### 2. Backend single-instance delivery

- `apps/everything-plus/src-tauri/Cargo.toml`
  - `tauri-plugin-single-instance`와 `devbox-applink`를 추가했다.
- `apps/everything-plus/src-tauri/src/applink.rs`
  - `Mutex<Option<OpenRequest>>` 기반 `PendingOpen`을 추가했다.
  - `take_pending_open`은 반환과 동시에 slot을 비운다.
  - 소비 전 새 요청이 오면 최신 사용자 의도가 이전 요청을 교체한다.
- `apps/everything-plus/src-tauri/src/lib.rs`
  - single-instance plugin을 첫 plugin으로 등록했다.
  - hot relaunch는 pending set → `devbox://open` emit → show/unminimize/focus 순서로 처리한다.
  - cold setup은 같은 parser와 pending state를 사용한다.
  - parse 실패 로그를 `applink: invalid request`로 고정해 raw argv 값을 기록하지 않는다.

```rust
match devbox_applink::parse_argv(&args) {
    Ok(Some(request)) => {
        app.state::<applink::PendingOpen>().set(request.clone());
        let _ = app.emit("devbox://open", request);
    }
    Ok(None) => {}
    Err(_) => eprintln!("applink: invalid request"),
}
```

### 3. Query routing and existing search pipeline

- `apps/everything-plus/src/api.ts`
  - applink wire shape과 일치하는 `OpenTarget`, `OpenRequest`를 추가했다.
  - `takePendingOpen`, `onOpenRequest` Tauri wrapper를 추가했다.
- `apps/everything-plus/src/lib/applink.ts`
  - Query만 허용한다.
  - trim 후 empty, 512자 초과, NUL query를 generic 오류로 거부한다.
  - Path/Profile/Workspace도 raw payload 없는 unsupported 오류로 거부한다.
- `apps/everything-plus/src/App.tsx`
  - listener 등록 후 cold pull을 수행한다.
  - hot event callback은 payload를 적용하지 않고 pending slot을 다시 take한다.
  - valid Query는 `mode=name`, `regex=false`, `regexError=null`로 만든 뒤 `query` state에 넣는다.
  - 기존 query effect가 150ms debounce, stale sequence guard와 `searchFiles`를 그대로 담당한다.
  - name/content 검색 응답에도 sequence guard를 적용해 이전 검색이 늦게 끝나더라도 inbound
    Query 결과를 덮어쓰지 못하게 했다.
  - invalid 또는 IPC 실패는 앱을 닫지 않고 기존 error 영역에 표시한다.

name/non-regex를 강제한 이유는 발신자가 Everything+의 현재 UI mode를 알 수 없기 때문이다.
같은 Query가 실행 중 사용자의 content/regex 상태에 따라 다른 의미가 되는 것을 막고, catalog의
일반 `query` capability를 항상 파일명 검색으로 해석한다.

### 4. Tests

- `src-tauri/src/applink.rs`
  - one-shot take와 newest-wins를 검증한다.
- `src/lib/applink.test.ts`
  - trim된 Query, empty/oversized/NUL, unsupported target와 generic 오류를 검증한다.
- `src/App.test.tsx`
  - listener-before-take cold Query와 즉시 검색을 검증한다.
  - stale hot event payload 대신 pending Query를 적용하는지 검증한다.
  - listener 등록 실패의 cold pull fallback을 검증한다.
  - invalid Query가 검색이나 app 종료를 일으키지 않는지 검증한다.
  - 진행 중이던 이전 검색 응답이 inbound Query 결과를 덮어쓰지 않는지 검증한다.

### 5. Documentation and notices

- `apps/everything-plus/README.md`
  - Query 수신과 privacy 경계를 기록했다.
- `docs/architecture.md`
  - revision 3, listener-first pending, deterministic name/non-regex routing을 기록했다.
- `Cargo.lock`, `THIRD_PARTY_NOTICES.md`
  - 기존 locked dependency의 Everything+ direct edge와 provenance hash만 갱신했다.

## Verification Results

로컬 자원 사용을 제한하기 위해 package 단위, `-j 1`, frontend 단일 worker로 실행했다.
dependency tree는 main worktree에서 임시 symlink했고 exit trap에서 즉시 제거했다.

### Rust

```text
cargo test -p everything-plus --lib -j 1
25 passed; 0 failed

cargo test -p catalog --test catalog -j 1
11 passed; 0 failed

cargo test -p devbox-manager --lib -j 1
37 passed; 0 failed

cargo check -p everything-plus -j 1
passed

CARGO_BUILD_JOBS=1 cargo clippy -p everything-plus --all-targets -- -D warnings
passed

cargo fmt --all
passed
```

### Frontend

```text
pnpm exec tsc --noEmit
passed

pnpm exec vitest run --maxWorkers=1
2 files, 8 tests passed

NODE_OPTIONS=--max-old-space-size=1024 pnpm build
36 modules transformed; production build passed
```

### Repository gates

```text
bash .github/scripts/check-catalog.sh
passed

python3 .github/scripts/check-dependencies.py check
dependency policy OK

python3 .github/scripts/test-check-dependencies.py
passed

git diff --check
passed
```

## Follow-up Boundaries

- Windows packaged cold/hot Query와 second-instance focus evidence는 W1 checkpoint에서 남긴다.
- content index 강화, saved query CRUD/snapshot, semantic search는 별도 P2/P3 issue다.
- Repo Manager inbound Path는 P1-04의 다음 별도 app PR이다.
- protocol v2 Handoff는 이 PR에서 `crates/applink`를 변경하지 않는다.
