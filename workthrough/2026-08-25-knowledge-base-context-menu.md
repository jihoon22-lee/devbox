# Knowledge Tree and Editor Context Menus

## Overview

Issue #252의 P1-06-KN 범위로 Knowledge의 파일 트리와 CodeMirror 편집기 본문에
`@devbox/context-menu`를 적용했다. 트리에서는 파일·폴더 생성, 이름변경, danger 삭제,
canonical absolute path 복사, Explorer 표시, catalog와 실제 설치 상태에서 발견된 devbox 앱으로
열기를 제공한다. 편집기에서는 CM6 `EditorView.domEventHandlers`를 거쳐 잘라내기·복사·명시적
붙여넣기·Markdown 링크 삽입을 제공한다.

wikilink/backlink와 link-aware rename preview/transaction은 P1-09의 독립 기능이므로 포함하지
않았다. 앱 버전 0.4.0 bump도 release PR이 소유한다.

## Context

- 기존 트리는 파일에만 작은 이름변경·삭제 버튼이 있었고 폴더 작업, keyboard context menu,
  경로 복사, Explorer 표시, 다른 devbox 앱으로 전달하는 흐름이 없었다.
- 기존 `rename_file`과 `delete_file`은 폴더 filesystem 작업을 수행할 수 있었지만 SQLite FTS에서는
  폴더 이름과 일치하는 단일 행만 지웠다. 폴더 메뉴에 그대로 노출하면 하위 문서가 이전 경로로
  검색되는 고아 인덱스가 남는다.
- 트리 상대 경로는 frontend 입력이다. 메뉴가 열린 뒤 항목이 사라지거나 symlink가 바뀔 수 있어
  Copy/Explorer/launch/rename/delete 실행 직전 canonical 검증이 필요했다.
- CodeMirror 영역의 React `onContextMenu`는 CM6 selection과 입력 event 처리를 우회한다. UX 설계는
  `EditorView.domEventHandlers` 경유, IME 보호, selection 유지, Shift+F10/Menu key와 focus 복구를
  요구한다.
- WebView2의 clipboard read는 browser API만으로 일관되게 보장되지 않는다. 이미 dependency gate를
  통과한 Tauri clipboard plugin을 Knowledge에도 연결하되 plain text read command 하나만 허용했다.

## Changes Made

### 1. 파일 트리 메뉴와 selection-first 동작

Files:

- `apps/knowledge-base/src/App.tsx`
- `apps/knowledge-base/src/App.test.tsx`
- `apps/knowledge-base/package.json`

각 tree button을 focus 가능한 공용 context-menu trigger로 만들고 다음 앱 고유 topology를 제공한다.

```tsx
[
  { type: "item", id: "new-file", label: "새 파일" },
  { type: "item", id: "new-folder", label: "새 폴더" },
  { type: "item", id: "rename", label: "이름 변경" },
  { type: "item", id: "delete", label: "삭제", danger: true },
  { type: "item", id: "copy-path", label: "경로 복사" },
  { type: "item", id: "reveal", label: "탐색기에서 열기" },
  {
    type: "submenu",
    id: "open-in",
    label: "다른 앱으로 열기",
    items: installedCatalogTargets,
  },
]
```

- mouse right-click과 Shift+F10/Menu key는 공용 hook의 동일 경로를 사용한다.
- `onBeforeOpen`에서 button의 `data-tree-path`/`data-tree-dir`을 action snapshot으로 만들고
  우클릭한 row를 먼저 선택한다.
- 편집 중인 문서 `selected`와 tree navigation selection을 분리했다. 폴더를 우클릭해도 열린
  문서와 unsaved buffer는 바뀌지 않는다.
- 새 파일·폴더 기본 경로는 폴더 target이면 그 내부, 파일 target이면 같은 부모를 사용한다.
- 폴더 이름변경은 열린 하위 문서와 tree selection 경로를 새 prefix로 remap한다. 폴더 삭제는
  하위에서 열려 있던 문서와 dirty state를 함께 정리한다.
- 삭제는 공용 danger 표현과 “되돌릴 수 없음” confirmation을 모두 거친다.
- tree가 새로고침되어 action target이 사라지면 열린 메뉴와 stale snapshot을 닫는다.
- catalog/install 대상이 없거나 discovery가 끝나지 않았으면 다른 앱 submenu를 disabled로 둔다.
- 메뉴가 닫히면 공용 primitive가 원래 tree button에 focus를 복구한다.

### 2. Canonical filesystem action boundary

Files:

- `apps/knowledge-base/src-tauri/src/core/entry_actions.rs`
- `apps/knowledge-base/src-tauri/src/core/mod.rs`
- `apps/knowledge-base/src-tauri/src/commands/docs.rs`
- `apps/knowledge-base/src-tauri/src/lib.rs`
- `apps/knowledge-base/src/api.ts`

`canonical_existing_entry()`는 빈 값, absolute, `.`, `..`, prefix/root component를 거부한 뒤 Knowledge
root와 항목을 canonicalize한다. root 아래의 각 기존 component에는 `symlink_metadata`를 적용해
root 밖으로 나가는 symlink뿐 아니라 root 내부 symlink와 broken symlink도 fail closed한다. 항목은
실행 시점에 실제로 존재하고 canonical root 내부여야 한다.

생성·이름변경 목적지는 아직 존재하지 않을 수 있어 `validated_new_entry()`가 가장 가까운 기존
조상을 canonicalize하고 root containment를 검증한다. broken symlink를 `Path::exists()`만으로
놓치지 않도록 component마다 `symlink_metadata`를 먼저 검사한다.

새 Tauri command 경계는 다음과 같다.

- `create_directory`: 검증된 상대 목적지만 재귀 생성
- `entry_path`: 사용자가 경로 복사를 선택한 경우에만 canonical absolute path 반환
- `reveal_entry`: 검증된 현재 항목만 opener의 Explorer reveal로 전달
- `open_targets`: catalog `path` capability와 Manager 설치 metadata/executable 교집합의
  `id`/`displayName`만 반환
- `open_in`: 항목과 app id를 실행 직전에 다시 검증하고 `OpenTarget::Path`,
  `from=knowledge-base` 요청만 `devbox_launch::launch_open()`에 전달

오류는 받은 로컬 path, app id, OS 오류를 반향하지 않는다. resolved executable과 install-root
metadata는 Rust process 안에 남고 frontend IPC에 노출되지 않는다.

### 3. 폴더 rename/delete의 FTS 일관성

Files:

- `apps/knowledge-base/src-tauri/src/core/db.rs`
- `apps/knowledge-base/src-tauri/src/commands/docs.rs`

`remove_docs_under()`는 exact path 또는 `path + '/'` prefix의 모든 문서를 지운다. SQL `LIKE` 대신
`substr` 비교를 사용해 실제 폴더명의 `%`와 `_`를 wildcard로 해석하지 않는다.

폴더 이름변경 뒤에는 하나의 SQLite transaction에서 이전 prefix를 제거하고 새 폴더를 순회해
읽을 수 있는 text file만 새 상대 경로로 인덱스한다. 각 하위 file도 다시 canonical/symlink
검증하므로 폴더 내부 symlink를 따라 외부 content를 FTS에 넣지 않는다. binary/non-UTF-8 파일은
기존처럼 건너뛴다. 인덱스 transaction이 실패하면 가능한 경우 filesystem rename도 원래 경로로
되돌린다. 폴더 삭제는 filesystem 삭제가 실패하면 prefix 제거 transaction도 rollback하며,
성공한 삭제에만 commit한다.

이 보강은 filesystem과 FTS 보조 인덱스의 일관성만 다룬다. 다른 문서 안의 wikilink text,
backlink 위치, 충돌 preview와 multi-file transaction은 변경하지 않는다.

### 4. CodeMirror-native editor menu

Files:

- `apps/knowledge-base/src/components/MarkdownEditor.tsx`
- `apps/knowledge-base/src/components/editorActions.ts`
- `apps/knowledge-base/src/components/MarkdownEditor.test.tsx`
- `apps/knowledge-base/src/components/editorActions.test.ts`
- `apps/knowledge-base/src-tauri/capabilities/default.json`
- `apps/knowledge-base/src-tauri/Cargo.toml`

메뉴 event는 CM6 extension의 `EditorView.domEventHandlers` 안에서 처리한다.

- right-click 좌표가 기존 selection 밖이면 CM6 cursor를 그 위치로 먼저 동기화한다. selection
  안이면 선택 text를 유지한다.
- keyboard open은 현재 CM6 selection을 유지하고 editor content DOM을 focus restore target으로 둔다.
- `event.isComposing`, keyCode 229 또는 CM6 `compositionStarted` 동안 메뉴 key/right-click을
  가로채지 않는다.
- handler는 ContextMenu/Shift+F10만 처리하므로 기존 Ctrl+X/C/V/Z와 CodeMirror keymap은 유지된다.
- 선택이 없으면 잘라내기와 복사는 disabled다. multiple selection copy는 document line break로
  결정적으로 결합하고 cut은 CM6 `changeByRange` transaction으로 모든 non-empty selection을 지운다.
- 붙여넣기는 메뉴 action 시점에만 `readClipboardText()`를 호출하고 CM6 `replaceSelection()`으로
  현재 selection을 교체한다.
- 링크 삽입은 선택 text를 `[label](url)`로 감싸고 selection이 없으면 `[링크](url)`을 삽입한다.
  이는 일반 Markdown 링크이며 P1-09 wikilink 자동완성이 아니다.
- clipboard/action 실패는 App의 복구 가능한 error banner로 전달하며 편집기를 닫지 않는다.

Tauri capability는 `clipboard-manager:allow-read-text` 하나만 추가했다. image/write/clear IPC,
background read, history, persistence, network, sidecar는 추가하지 않았다. Copy/Cut의 쓰기는 기존
사용자 action 기반 WebView clipboard write 경로를 사용한다.

### 5. Tests, locks, notices, and documentation

Tests cover:

- tree pointer/keyboard open, selection-first, exact topology, disabled/danger 표현, focus restore
- 파일·폴더 생성 기준 경로, rename, delete confirmation, canonical path copy, Explorer, catalog target
- missing install target과 recoverable command failure
- editor pointer/keyboard open, selection disabled state, copy/cut/paste/link exact document change,
  clipboard failure, focus restore, IME key 보호
- multi-selection pure CM6 transaction
- traversal/symlink/broken-symlink rejection과 raw input non-echo
- catalog order/source exclusion, allowed target request shape와 missing target rejection
- exact/prefix FTS cleanup, wildcard-like folder name, folder rename reindex와 recursive delete cleanup

Workspace wiring adds existing internal `@devbox/context-menu`/`devbox-launch` edges and the already locked,
reviewed `tauri-plugin-clipboard-manager 2.3.2` frontend/Rust edge. No new registry version, license,
external executable, daemon, download or network path was introduced. Lockfile provenance changed, so
`THIRD_PARTY_NOTICES.md`의 Cargo/pnpm SHA-256만 generator로 갱신했다.

Documentation updates:

- `apps/knowledge-base/README.md`: tree/editor menu, clipboard boundary, canonical action, folder FTS
- `docs/architecture.md`: context-menu rollout, launch consumer/data flow, clipboard consumers
- `docs/dependency-policy.md`: approved clipboard plugin의 Knowledge direct consumer와 동일 최소 권한

## Verification Results

### Frontend tests and production build

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter knowledge-base test -- --maxWorkers=1
Test Files  5 passed (5)
Tests      24 passed (24)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 pnpm --filter knowledge-base build
vite v7.3.6
2151 modules transformed
dist/assets/index-axBV1PTI.js  1,374.54 kB | gzip 404.46 kB
dist/assets/index-tEoNXQb4.css      7.71 kB | gzip   2.12 kB
exit 0
```

Vite의 기존 Mermaid/CodeMirror 500 kB chunk warning은 유지되며 build 실패가 아니다.

### Rust formatting, tests, compile, and lint

```text
$ cargo fmt --package knowledge-base -- --check
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p knowledge-base -j1
31 passed; 0 failed
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo check -p knowledge-base --all-targets -j1
Finished dev profile
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo clippy -p knowledge-base --all-targets -j1 -- -D warnings
Finished dev profile
exit 0
```

### Dependency, notice, catalog, and diff gates

```text
$ python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml

$ python3 .github/scripts/test-check-dependencies.py
dependency policy regression tests passed

$ python3 .github/scripts/test-build-manifest.py
build-manifest notice tests passed

$ bash .github/scripts/check-catalog.sh
exit 0

$ git diff --check
exit 0
```

모든 로컬 명령은 frontend heap 768 MiB, Vitest 1 worker, Cargo 1 job과 Linux-native shared target
directory를 사용했다. 전체 workspace와 Windows compile은 GitHub Actions의 PR gate를 권위 있는
검증으로 사용한다.

## Initial Test Corrections

첫 frontend run에서 새 menu 동작과 editor/action unit test는 통과했지만 두 fixture 문제가
발견됐다.

1. 기존 `App.applink.test.tsx`의 full API mock이 새 `openTargets` export를 제공하지 않았다.
   새 command API를 no-op/empty fixture로 명시해 app-link 범위가 계속 독립적으로 검증되게 했다.
2. tree folder의 visual label은 `▾`와 이름이 두 text node로 구성돼 exact `findByText("Notes")`가
   부적절했다. 실제 action identity인 `data-tree-path`로 row를 선택하도록 fixture를 바꿨다.

수정 후 기존 app-link/preview 8개와 새 tree/editor/action 16개를 함께 재실행했다.

## Security and Failure Boundaries

- frontend는 target executable, install locator/manifest 또는 copy action 전 absolute path를 받지 않는다.
- 모든 context filesystem/launch command는 실행 직전에 relative component, canonical root containment,
  symlink/broken symlink와 현재 존재 여부를 다시 검증한다.
- frontend가 조작한 app id는 현재 catalog/install 교집합에 없으면 실행되지 않는다.
- 오류에 untrusted rel/app id, absolute local path, OS error가 포함되지 않는다.
- directory reindex는 symlink content를 읽지 않고 binary/non-UTF-8 file을 건너뛴다.
- clipboard read는 foreground의 명시적 붙여넣기 action 한 번에 한정되고 저장·전송되지 않는다.
- 삭제는 danger styling과 confirmation을 모두 요구한다.
- IME composition과 기본 CodeMirror shortcut은 context-menu handler가 가로채지 않는다.
- wikilink rewrite, link-aware rename transaction, capture/image, arbitrary external executable 선택은 비범위다.

## Follow-up

- #253~#261: 나머지 기존 앱별 context menu
- P1-09 Knowledge wikilink/backlink와 link-aware rename preview/transaction
- P2-09 Knowledge global capture와 image workflow
- Windows W1: packaged WebView2의 tree/editor pointer·Menu key·IME, Explorer reveal,
  installed target cold/hot launch, danger confirmation과 focus restore evidence
