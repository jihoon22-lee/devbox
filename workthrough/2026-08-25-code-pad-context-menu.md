# Code Pad Tab and Editor Context Menus

## Overview

Issue #253의 P1-06-CP 범위로 Code Pad의 문서 탭과 CodeMirror 6 편집기 본문에
`@devbox/context-menu`를 적용했다. 탭 메뉴는 닫기·다른 탭 닫기·오른쪽 탭 모두 닫기,
canonical 경로 복사, Explorer 표시, 파일 이름변경, danger 삭제를 제공한다. 편집기 메뉴는
CM6 `EditorView.domEventHandlers`를 통해 잘라내기·복사·명시적 붙여넣기·정의로 이동·참조 찾기를
제공한다.

Quick Open tree, LSP 기능 자체, preview 기능은 확장하지 않았다. Code Pad 0.4.0 version bump도
release PR이 소유한다.

## Context

- 탭에는 close와 view 이동 button만 있었고 keyboard context menu, batch close, path/file action이
  없었다.
- 기존 dirty close dialog는 단일 `pendingCloseDocId`만 소유했다. 이 구조로 “다른 탭 닫기”나
  “오른쪽 탭 모두 닫기”를 구현하면 여러 dirty buffer 중 하나를 덮어쓰거나 확인 없이 버릴 수 있다.
- Code Pad의 열린 파일은 mtime·size·SHA-256을 optimistic concurrency snapshot으로 가진다. rename과
  delete도 save와 같은 현재 disk identity 검증 없이 frontend path만 신뢰하면 stale tab이 교체된
  파일을 변경할 수 있다.
- Unix `std::fs::rename`은 destination을 덮어쓸 수 있다. rename 직전 existence check만 추가하면
  check와 mutation 사이에 생성된 파일을 파괴하는 TOCTOU가 남는다.
- 파일 이름변경 뒤에는 tab label뿐 아니라 watcher path, external-change queue, LSP document URI,
  session recent path를 함께 이동해야 한다. dirty CodeMirror buffer와 long-lived editor instance는
  교체하면 안 된다.
- React `onContextMenu`로 CM6 바깥에서 selection을 처리하면 composition, multi-selection, cursor,
  keyboard event 순서를 잃는다. UX 설계는 CM6 DOM handler와 IME 보호를 명시한다.
- WebView clipboard read는 설치 후 오프라인에서도 일관된 explicit Paste가 필요하다. 이미 dependency
  gate를 통과한 Tauri clipboard plugin을 Code Pad에도 연결하되 read-text command만 허용했다.

## Changes Made

### 1. 대상 탭 우선 동기화와 정확한 메뉴 topology

Files:

- `apps/code-pad/src/components/TabBar.tsx`
- `apps/code-pad/src/components/ViewPane.tsx`
- `apps/code-pad/src/components/TabBar.test.tsx`
- `apps/code-pad/package.json`

각 tab button은 `data-doc-id`를 가진 공용 context-menu trigger다. mouse right-click과
Shift+F10/Menu key가 같은 hook을 사용하고, `onBeforeOpen`은 action target을 캡처한 뒤 해당 탭을
먼저 활성화한다.

```tsx
[
  { type: "item", id: "close", label: "닫기" },
  { type: "item", id: "close-others", label: "다른 탭 닫기" },
  { type: "item", id: "close-right", label: "오른쪽 탭 모두 닫기" },
  { type: "item", id: "copy-path", label: "경로 복사" },
  { type: "item", id: "reveal", label: "탐색기에서 열기" },
  { type: "item", id: "rename", label: "이름 변경" },
  { type: "item", id: "delete", label: "삭제", danger: true },
]
```

- 다른 탭이 없으면 “다른 탭 닫기”, 오른쪽 탭이 없으면 “오른쪽 탭 모두 닫기”를 disabled로 둔다.
- action은 menu를 연 시점의 `view`, `docId`로 전달하므로 React active-tab rerender와 독립적이다.
- target 문서가 닫히면 열린 메뉴와 stale target state를 함께 정리한다.
- 기존 Arrow/Home/End/Delete tab keyboard 동작을 유지하고 menu key만 공용 hook에 위임한다.
- composition event와 keyCode 229는 menu open으로 처리하지 않는다.
- 메뉴가 닫히면 공용 primitive가 원래 tab button에 focus를 복구한다.

### 2. 데이터 손실 없는 다중 탭 닫기 큐

Files:

- `apps/code-pad/src/App.tsx`
- `apps/code-pad/src/App.test.tsx`

단일 pending ID를 stable ordered `pendingCloseDocIds` queue로 바꿨다.

- 요청한 탭 중 clean 문서는 즉시 닫는다.
- dirty 문서는 현재 view 순서대로 queue에 한 번만 추가한다.
- dialog는 queue 첫 문서와 이후 대기 수를 보여준다.
- “변경 내용 버리고 닫기”는 현재 문서만 제거하고 다음 dirty 문서로 진행한다.
- “저장 후 닫기”는 제출 revision/text와 완료 시점 buffer가 모두 같은 경우에만 닫고 다음으로
  진행한다. 저장 중 새 편집이 있으면 현재 탭과 queue를 유지한다.
- 취소와 Escape는 아직 확인하지 않은 전체 queue를 취소한다.
- 외부 경로로 문서가 이미 제거되면 queue effect가 존재하는 문서 ID만 남긴다.

이 방식으로 batch close가 clean 탭을 막지 않으면서도 dirty buffer를 하나도 암묵적으로 버리지 않는다.

### 3. Snapshot-checked rename/delete backend

Files:

- `apps/code-pad/src-tauri/src/commands/file.rs`
- `apps/code-pad/src-tauri/src/lib.rs`
- `apps/code-pad/src/api.ts`

`FileActionRequest`는 canonical open 결과의 path와 lossless decimal mtime, size, SHA-256을 받는다.
rename/delete 직전에 파일을 다시 canonicalize하고 stable read한 exact bytes의 digest까지 비교한다.
mtime와 size가 같아도 content가 바뀌었으면 conflict로 거부한다.

Rename boundary:

- `newName`은 빈 값, 앞뒤 공백, `.`, `..`, `/`, `\\`, 복수 path component를 거부한다.
- destination은 canonical source와 같은 parent의 단일 sibling만 허용한다.
- existing file, directory, symlink, broken symlink destination을 모두 거부한다.
- Windows에서는 replacement flag 없는 `MoveFileW`를 사용한다.
- non-Windows test/development 경로에서는 create-new 성질의 sibling hard link를 만든 뒤 source를
  제거한다. 따라서 check 뒤 destination이 생성돼도 덮어쓰지 않는다. source 제거 실패 시 새 link를
  best-effort rollback해 원본 손실을 막는다.
- 성공 결과는 새 absolute path와 동일 disk snapshot을 반환한다.

Delete boundary:

- canonical existing regular file과 exact snapshot이 맞는 경우에만 `remove_file`을 수행한다.
- 성공한 namespace mutation 뒤 parent directory sync는 best effort다. sync 실패 때문에 frontend가
  이미 사라진 old path를 계속 소유하도록 성공을 실패로 뒤집지 않는다.

Tauri command 오류는 untrusted path, requested name, OS error를 반향하지 않고 고정된 복구 가능
메시지만 반환한다. Explorer action도 실행 시점에 canonical existing regular file을 다시 확인한 뒤
opener에 전달하며 detailed opener error를 노출하지 않는다.

### 4. Dirty buffer, watcher, LSP identity를 유지하는 rename transaction

Files:

- `apps/code-pad/src/App.tsx`
- `apps/code-pad/src/store/documentStore.ts`
- `apps/code-pad/src/store/documentStore.test.ts`

`renameDoc` reducer action은 기존 document ID, text, dirty, revision, cursor, bookmark를 그대로 두고
path와 disk snapshot만 바꾼다. 따라서 `DocHost` key가 유지되고 long-lived CodeMirror instance가
remount되지 않는다. recent files에서는 old/new 중복을 제거하고 새 path를 앞으로 이동한다.

Native rename 성공 뒤 App은 다음 순서로 authoritative state와 side effect를 정리한다.

1. old LSP close와 old watcher 해제를 시작하고 old external-change entry를 제거한다.
2. in-flight navigation request와 old diagnostics를 무효화하고 back/forward history path를 remap한다.
3. reducer가 같은 document ID의 path/snapshot만 즉시 갱신한다. native mutation 성공 뒤 LSP가
   느리더라도 UI가 이미 사라진 old path를 계속 가리키지 않는다.
4. new path watcher를 먼저 등록한다.
5. old LSP close가 끝난 뒤 최신 dirty text를 가진 document snapshot으로 didOpen을 다시 보낸다.
6. workspace가 열려 있으면 Quick Open listing을 다시 읽는다.

native 작업 도중 사용자가 buffer를 편집해도 최신 text/revision을 reducer에서 다시 읽으므로 rename
응답이 buffer를 덮어쓰지 않는다. 탭이 먼저 닫힌 race에서는 rename 결과를 억지로 다시 열지 않고
workspace listing만 갱신한다.

Delete는 danger styling에 더해 path, 영구 삭제, 미저장 buffer 복구 불가를 포함한 explicit confirm을
요구한다. backend 성공 뒤에만 document/LSP/watcher를 정리하며 snapshot conflict나 command failure면
탭과 buffer를 유지한다.

### 5. CodeMirror-native editor menu

Files:

- `apps/code-pad/src/editor/CodeEditor.tsx`
- `apps/code-pad/src/editor/contextActions.ts`
- `apps/code-pad/src/editor/CodeEditor.test.tsx`
- `apps/code-pad/src/components/DocHost.tsx`
- `apps/code-pad/src-tauri/capabilities/default.json`
- `apps/code-pad/src-tauri/Cargo.toml`

메뉴 event는 각 long-lived editor의 `EditorView.domEventHandlers` extension 안에서 처리한다.

- right-click 좌표가 현재 selection 밖이면 CM6 cursor를 그 위치로 먼저 옮긴다. selection 안이면
  선택 text를 유지한다.
- keyboard open은 현재 cursor/selection을 유지하고 content DOM을 focus restore target으로 둔다.
- CM6 `compositionStarted`, `event.isComposing`, keyCode 229 동안 menu event를 가로채지 않는다.
- 선택이 없으면 Cut/Copy를 disabled로 두며 read-only editor에서는 Cut/Paste를 추가로 disabled한다.
- multi-selection Copy는 document line break로 결합하고 Cut은 `changeByRange` transaction으로 각
  selection을 안전하게 제거한다.
- Paste는 사용자가 항목을 고른 순간에만 native plain-text clipboard read를 수행하고
  `replaceSelection()`으로 현재 selection을 교체한다.
- clipboard Promise가 대기하는 동안 document나 selection이 실제로 바뀌면 late Cut/Paste를 취소한다.
  단순 focus transaction은 허용해 메뉴 focus 복구가 정상 action을 막지 않는다.
- definition/reference 항목은 현재 editor의 clicked cursor와 document ID를 App에 전달한다. App은 해당
  문서의 negotiated capability와 global LSP busy guard를 다시 확인하고 기존 request/panel 경로를 쓴다.
- clipboard action 실패와 late target 취소는 기존 recoverable error banner로 전달한다.

Tauri capability는 `clipboard-manager:allow-read-text` 하나만 추가했다. image/write/clear IPC,
background read, history, persistence, network, sidecar는 없다. Copy/Cut의 쓰기는 기존 foreground
WebView clipboard API를 사용한다.

### 6. Locks, notices, dependency rationale, and documentation

Workspace wiring은 이미 구현·검토된 internal `@devbox/context-menu`와 locked
`tauri-plugin-clipboard-manager 2.3.2`의 Code Pad direct-consumer edge만 추가한다. 새 registry package,
version, executable, daemon, external download, network path는 추가하지 않았다. 이 기능은 설치 뒤
인터넷 없이 공용 HTML menu와 OS clipboard IPC로 동작한다.

Lockfile provenance가 바뀌어 generator로 `THIRD_PARTY_NOTICES.md`의 Cargo/pnpm SHA-256을 갱신했다.

Documentation updates:

- `apps/code-pad/README.md`: tab/editor menu, dirty queue, snapshot rename/delete, clipboard boundary
- `docs/architecture.md`: context-menu rollout, Code Pad data flow, clipboard direct consumer
- `docs/dependency-policy.md`: 승인된 clipboard plugin의 Code Pad 소비와 동일 최소 권한

## Test Coverage

Frontend tests cover:

- tab exact topology, target-first activation, disabled/danger state, pointer/keyboard open, focus restore, IME
- clean right-hand tab immediate close와 dirty tab ordered queue, discard/cancel progression
- dirty rename buffer preservation, old/new watcher migration, recent path reducer semantics
- delete confirmation, snapshot failure tab preservation, successful delete close/unwatch
- canonical tab path copy와 validated backend Explorer delegation
- editor Cut/Copy/Paste exact CM6 mutation, read-only disabled state, clicked cursor LSP navigation
- editor keyboard open, focus restore, IME protection, late clipboard selection race cancellation
- 기존 editor lifetime, external value, cursor/bookmark, LSP, session, recovery, watcher 회귀

Rust tests cover:

- rename success와 bytes/snapshot 보존
- traversal, nested path, padded name, existing destination 무덮어쓰기
- rename/delete stale mtime/size/hash conflict
- validated delete success
- command error의 raw path non-echo
- 기존 open/save/encoding/preview/session/watcher/LSP 전체 회귀

## Verification Results

### Frontend tests and production build

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter code-pad test -- --maxWorkers=1
Test Files  13 passed (13)
Tests      100 passed (100)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 pnpm --filter code-pad build
vite v7.3.6
2170 modules transformed
dist/assets/index-CnUZtp5v.js  1,668.09 kB | gzip 505.82 kB
dist/assets/index-BzYPqkOu.css     16.51 kB | gzip   4.24 kB
exit 0
```

Vite의 기존 Mermaid/CodeMirror 500 kB chunk warning은 유지되며 build 실패가 아니다.

### Rust formatting, tests, compile, and lint

```text
$ cargo fmt --all -- --check
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p code-pad --lib -j1
152 passed; 0 failed
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo check -p code-pad -j1
Finished dev profile
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo clippy -p code-pad --all-targets -j1 -- -D warnings
Finished dev profile
exit 0
```

### Dependency, notice, catalog, and diff gates

```text
$ python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml

$ pnpm audit --audit-level moderate
No known vulnerabilities found

$ cargo deny --locked check
advisories ok, bans ok, licenses ok, sources ok

$ python3 .github/scripts/test-build-manifest.py
build-manifest notice tests passed

$ bash .github/scripts/check-catalog.sh
exit 0

$ git diff --check
exit 0
```

모든 로컬 명령은 frontend heap 768 MiB, Vitest 1 worker, Cargo 1 job과 Linux-native shared target
directory를 사용했다. 전체 workspace와 Windows compile은 GitHub Actions PR gate를 권위 있는
검증으로 사용한다.

## Initial Test Corrections

첫 full frontend run은 98개 중 97개가 통과했다. 새 editor LSP test가 right-click 좌표로 cursor를
0에 옮기는 정상 selection-first 동작을 두고 이전 prop cursor 4를 기대해 실패했다. 기대값을 실제
clicked cursor 0으로 수정했다.

late clipboard 보호를 처음에는 `EditorState` 객체 identity로 판별했다. 메뉴가 닫히며 focus가
editor로 돌아오는 정상 transaction도 새 state 객체를 만들기 때문에 Cut이 취소됐다. 문서 `Text`와
`EditorSelection`의 실제 equality만 비교하도록 좁히고, selection이 진짜 바뀐 delayed Promise test를
추가했다. focused CodeEditor 9개와 App 23개가 통과한 뒤 최종 100개 전체 suite를 통과했다.

PR 직전 집중 검토에서 native rename 성공 뒤 old LSP close가 느리면 UI가 이미 사라진 old path에
머무는 순서와 rename 후 navigation/diagnostics identity, hidden view의 portal menu 수명을 보강했다.
변경된 App·TabBar·CodeEditor 3개 파일의 36개 test를 다시 실행해 모두 통과했고 production build도
다시 생성했다.

첫 PR CI의 Linux Rust, frontend, dependency, catalog gate는 통과했지만 Windows Rust test에서
rename 결과의 canonical long path(`\\?\C:\\...`)를 `tempfile`이 돌려준 8.3 short path와 문자열로
직접 비교한 test assertion 하나가 실패했다. 구현은 열린 문서와 watcher가 사용하는 canonical path를
정상적으로 반환하고 있었으므로, assertion을 rename 뒤 destination의 canonical path와 비교하도록
고쳤다. 같은 단일 test를 Linux에서 다시 실행해 통과했으며 Windows CI를 재실행한다.

## Security and Failure Boundaries

- rename/delete는 frontend path가 아니라 실행 직전 canonical regular file과 exact disk snapshot을
  검증한다.
- rename destination은 source sibling 한 개뿐이며 existing file/directory/symlink를 덮어쓰지 않는다.
- mutation/reveal command 오류는 local path, requested name, OS detail을 반향하지 않는다.
- delete는 danger style과 영구 삭제·dirty loss explicit confirmation을 모두 요구한다.
- batch close는 dirty buffer마다 명시적 discard/save 선택을 요구한다.
- clipboard read는 foreground explicit Paste 한 번에 한정되고 저장·전송되지 않는다.
- late clipboard result는 새 문서/selection을 변경하지 않는다.
- read-only editor mutation, IME composition, 기존 CodeMirror shortcuts는 context menu가 침범하지 않는다.
- LSP menu는 새 server/request 구현을 만들지 않고 기존 negotiated capability와 stale/busy 경계를 재사용한다.
- Quick Open tree menu, multi-file rename, LSP cache, preview 구분은 이 PR의 범위가 아니다.

## Follow-up

- #254~#261: 나머지 기존 앱별 context menu
- Code Pad P1 polish: Quick Open tree grouping, LSP panel UX, preview 시각 구분
- P2-12 managed LSP offline cache/local archive
- P3-12 safe multi-file rename
- Windows W1: packaged WebView2의 tab/editor pointer·Shift+F10·Menu key·IME, clipboard permission,
  Explorer reveal, dirty batch dialog, rename/delete와 focus restore 실기 evidence
