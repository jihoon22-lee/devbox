# Knowledge Path·Query Single-Instance Receive

## Overview

P1-04-K [#241](https://github.com/jihoon22-lee/devbox/issues/241)의 Knowledge Base
inbound 기능을 구현했다. Knowledge는 catalog revision 2부터 `path`와 `query`를 수신하며,
cold-start argv와 이미 실행 중인 instance의 재호출을 같은 one-shot `PendingOpen` 경로로
frontend에 전달한다.

`Path`는 현재 Knowledge root 내부의 실제 Markdown note만 열 수 있다. canonical path 검사,
root containment, `.md` 확장자, 10 MiB 상한을 통과한 파일을 backend에서 바로 bounded read해
root-relative path와 내용만 frontend로 반환한다. `Query`는 공백과 크기를 검증한 뒤 기존 FTS
검색 UI에 연결한다. 실패 메시지에는 raw path, query, OS 오류나 parser 입력을 반향하지 않는다.

Wikilink/backlink, quick capture, handoff draft는 이 PR에 포함하지 않았다. 각각 계획된 후속
기능 경계를 유지한다.

## Context

Knowledge Base는 v0.4.x에서 catalog에 inbound capability를 선언하지 않았고, 실행 인자를
해석하거나 두 번째 instance를 기존 창으로 전달하는 코드도 없었다. 다른 devbox 앱이 note
또는 검색어를 전달하려 해도 Knowledge는 평소 초기 화면만 열었다.

Code Pad, WSL Desktop, Workbench에는 이미 다음 경합 방지 패턴이 있었다.

1. `tauri-plugin-single-instance`를 첫 plugin으로 등록한다.
2. cold argv와 hot relaunch argv를 모두 `crates/applink::parse_argv`로 해석한다.
3. 해석한 request를 `PendingOpen`에 저장한다.
4. frontend가 event listener를 먼저 등록한 뒤 pending slot을 pull한다.
5. hot event payload는 알림으로만 쓰고 authoritative request는 다시 pending slot에서 take한다.

이번 구현은 이 패턴을 Knowledge에 적용하면서, 앱 고유의 note-root 보안 경계와 기존 검색
상태 연결만 추가했다.

## Changes Made

### 1. Catalog capability and dependency wiring

- `apps/catalog.json`
  - `catalogRevision`을 1에서 2로 단조 증가시켰다.
  - `knowledge-base.accepts`를 `path`, `query`로 선언했다.
  - 다른 앱 capability는 변경하지 않았다.
- `crates/catalog/tests/catalog.rs`
  - repository catalog 계약 fixture를 revision 2와 Knowledge Path·Query 대상으로 갱신했다.
- `apps/devbox-manager/src-tauri/src/core/catalog.rs`
  - Manager build-time adapter fixture도 같은 revision과 Knowledge capability를 검증한다.
- `apps/knowledge-base/src-tauri/Cargo.toml`
  - hot-instance forwarding을 위해 `tauri-plugin-single-instance`를 추가했다.
  - 공용 argv 계약을 위해 `devbox-applink`를 추가했다.
  - path fixture를 위해 test-only `tempfile`을 추가했다.
- `Cargo.lock`, `THIRD_PARTY_NOTICES.md`
  - Knowledge package dependency edge와 lockfile provenance hash를 갱신했다.
  - 새로운 third-party version이나 runtime license 항목은 생기지 않았다.

### 2. One-shot backend delivery

새 `apps/knowledge-base/src-tauri/src/applink.rs`는 `Mutex<Option<OpenRequest>>` 기반
`PendingOpen`을 소유한다. `take_pending_open`은 값을 반환하면서 slot을 비워 페이지 reload나
hot event가 같은 request를 다시 적용하지 않게 한다. 새 request가 소비 전 도착하면 가장 최근
사용자 의도를 보존하도록 이전 값을 교체한다.

`src-tauri/src/lib.rs`는 single-instance plugin을 opener와 setup보다 먼저 등록한다. hot
relaunch에서는 request를 pending slot에 저장한 다음 `devbox://open`을 emit하고, main window를
show·unminimize·focus한다. cold start는 setup에서 같은 parser와 같은 pending state를 쓴다.
parser 오류 로그는 입력값을 포함하지 않는 `applink: invalid request`로 고정했다.

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

### 3. Canonical Path boundary and bounded note read

새 `src-tauri/src/core/inbound.rs`는 untrusted Path를 다음 순서로 처리한다.

- 빈 값, 32 KiB를 넘는 path, NUL, raw `.`/`..` segment를 거부한다.
- configured Knowledge root와 candidate를 canonicalize한다.
- candidate가 canonical root 내부의 regular file인지 확인한다.
- case-insensitive `.md` 확장자만 허용한다.
- metadata와 bounded reader 양쪽에서 10 MiB 상한을 적용한다.
- canonical target을 직접 읽어 원래 symlink path가 read 사이에 바뀌는 창을 줄인다.
- frontend에는 `/` 구분자의 root-relative path와 UTF-8 content만 반환한다.

`open_inbound_note` Tauri command는 root lookup 오류, filesystem 오류, UTF-8 오류를 모두
`요청한 노트를 열 수 없습니다`로 축약한다. fixture는 absolute/relative 성공, traversal,
missing path, directory, non-Markdown, outside-root path, Unix symlink escape, oversized path/file,
secret-like path가 오류에 나타나지 않는지를 검증한다.

### 4. Frontend routing and search integration

새 `src/lib/applink.ts`는 request를 세 action으로 바꾼다.

- valid Path → `openNote`; filesystem 권한 판단은 backend에 남긴다.
- trim 후 1~512자인 Query → `search`; 기존 `searchDocs`와 결과 목록을 재사용한다.
- empty/oversized/NUL/unsupported target → raw 입력 없는 recoverable `error`.

`App.tsx`는 listener 등록이 완료된 뒤 cold pending request를 한 번 가져온다. hot event callback도
event payload를 직접 적용하지 않고 pending slot을 take하므로, stale payload와 duplicate
application을 피한다. Path는 기존 dirty-note 확인을 유지하고, 성공하면 editor state를
교체한다. Query는 search input과 FTS result state를 함께 갱신한다. listener 또는 IPC 실패도
앱을 닫지 않고 상단 error 영역에 표시한다.

`api.ts`에는 wire-compatible `OpenTarget`, `OpenRequest`, `InboundNote` 타입과
`takePendingOpen`, `onOpenRequest`, `openInboundNote` wrapper를 추가했다.

### 5. Fixtures and documentation

- `src/lib/applink.test.ts`
  - Path routing, Query trim, invalid/oversized/unsupported target를 검증한다.
- `src/App.applink.test.tsx`
  - listener-before-take cold delivery를 검증한다.
  - hot event의 stale payload 대신 pending request를 적용하는지 검증한다.
  - listener 실패 뒤 cold pull fallback과 generic invalid Path 오류를 검증한다.
  - invalid Query가 검색을 실행하지 않고 앱을 유지하는지 검증한다.
- `src/App.test.tsx`
  - 기존 UI fixture에 새 inbound API의 neutral mock을 추가했다.
- `apps/knowledge-base/README.md`, `docs/architecture.md`
  - capability, single-instance 흐름, canonical/bounded Path 경계를 기록했다.

## Verification Results

로컬 자원 점유를 제한하기 위해 Knowledge package만 단일 job/worker로 검증했다. frontend는
main worktree의 dependency tree를 임시 symlink로 연결했고 모든 명령의 exit trap에서 즉시
제거했다.

### Rust

```text
cargo test -p knowledge-base --lib -j 1
23 passed; 0 failed

cargo check -p knowledge-base -j 1
passed

CARGO_BUILD_JOBS=1 cargo clippy -p knowledge-base --all-targets -- -D warnings
passed

cargo fmt --all
passed

cargo test -p catalog --test catalog -j 1
11 passed; 0 failed

cargo test -p devbox-manager --lib -j 1
37 passed; 0 failed
```

### Frontend

```text
pnpm exec tsc --noEmit
passed

vitest full app run before the final fixture-only correction
App.test.tsx + lib/applink.test.ts: 7 passed

pnpm exec vitest run src/App.applink.test.tsx --maxWorkers=1
4 passed
```

마지막 변경은 실패했던 test-order recorder만 고친 것이며 production source는 바뀌지 않았다.
따라서 현재 3개 test file의 11개 test가 각각 통과한 근거를 확보했다. PR CI가 동일한 현재
tree에서 Knowledge frontend 전체 suite를 다시 실행한다.

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

- Actual Windows packaged cold/hot launch, second-instance focus restore와 화면·로그 evidence는
  계획된 W1 checkpoint에서 검증한다.
- Knowledge wikilink/backlink/rename, quick capture/image, draft handoff는 별도 issue다.
- Everything+ Query와 Repo Manager Path inbound는 P1-04의 별도 앱 PR이다.
- protocol v2 Handoff 추가는 이 PR에서 `crates/applink`를 변경하지 않는다.
