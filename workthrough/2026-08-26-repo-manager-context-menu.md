# Repo Manager Repository Context Menu

## Overview

Issue #258의 P1-06-RP 범위로 Repo Manager repository card에 `@devbox/context-menu`를 적용했다.
pointer right-click, `Shift+F10`, Menu key는 같은 app-owned menu/action 경로를 사용하고, 메뉴를 열기
전에 canonical repository identity로 정확한 card를 선택하며 닫힌 뒤 원래 card로 focus를 복구한다.

정확한 menu topology:

```text
다른 앱으로 열기 ▸
  <현재 설치된 catalog path/workspace capability target>
worktree 생성
경로 복사
탐색기에서 열기
```

worktree 생성은 사용자가 이미 채워야 하는 branch/target directory form으로 focus만 이동한다. 메뉴를
누르는 즉시 Git command를 실행하지 않는다. 경로 복사와 OS file manager 열기는 card의 frontend path를
그대로 신뢰하지 않고 backend가 action 시점에 absolute/traversal/existence/`.git`을 재검증한다.

Issue body의 generic “remove danger confirmation” 문구와 달리 확정 UX 설계의 repository menu에는
remove가 없다. 실제 worktree/branch 제거는 dirty/untracked/locked/main worktree 차단, preview/result를
소유하는 P3 #364 safe cleanup의 독립 기능이다. 이번 PR은 기존 read-only `worktree_clean` 안내를
유지하고 파괴 action을 추가하지 않는다. Git history와 cleanup은 명시적 비범위다.

## Context

- repository card는 inline installed-app buttons와 worktree form만 있었고 pointer/keyboard context menu가
  없었다.
- inbound app-link 선택용 `selectedRepoKey`는 이미 있었지만 일반 pointer 선택과 menu target으로
  사용되지 않았다.
- repository path는 normal list DTO에 표시되지만 copy/open action 직전에 repository가 삭제·교체되거나
  frontend state가 stale해질 수 있다.
- catalog target은 app 시작 때 비동기로 발견하므로 loading/failure/empty 상태에서 submenu를
  fail-closed로 유지해야 한다.
- target discovery error가 기존에는 backend error 원문을 UI에 반향했다.
- card 전체에 context menu trigger를 단순 부착하면 내부 새 branch/target directory input의 기본
  context menu와 한국어 IME keyboard behavior를 가로챈다.
- 기존 `remove 확인`은 `git status --porcelain`으로 clean 여부만 읽고 실제 remove를 수행하지 않는다.
  이를 destructive menu item처럼 보이게 하거나 구현하면 #364의 안전 gate를 우회한다.

## Changes Made

### 1. Target-first repository menu

Files:

- `apps/repo-manager/package.json`
- `pnpm-lock.yaml`
- `apps/repo-manager/src/App.tsx`
- `apps/repo-manager/src/App.css`
- `apps/repo-manager/src/lib/contextMenu.ts`

repository card는 keyboard focus가 가능한 `tabIndex=0`이고 `data-repo-key`에 canonical key만 둔다.
공용 hook의 `onBeforeOpen`은 현재 repository collection에서 `sameRepositoryKey`로 대상을 다시 찾고
selection, registration draft, context snapshot을 동기화한다. rescan으로 repository가 사라지면 열린
menu와 stale selection을 닫는다.

공용 package가 viewport placement, root/submenu keyboard navigation, 바깥 click/Esc/scroll close,
disabled skip과 focus restore를 담당한다. Repo Manager는 exact items, target/busy state와 action dispatch를
소유한다.

### 2. Catalog-driven “open in” submenu

Files:

- `apps/repo-manager/src/App.tsx`
- `apps/repo-manager/src/lib/contextMenu.ts`
- 기존 `apps/repo-manager/src-tauri/src/core/open_targets.rs`
- 기존 `apps/repo-manager/src-tauri/src/commands.rs`

submenu는 기존 `open_targets` DTO의 app ID, display name, payload kind만 사용한다. backend의
`devbox_launch::installed_targets`가 현재 `path` capability와 실제 installed executable의 교집합을
제공하며 source app을 제외하고, 동일 app이 `workspace`도 받으면 Workspace payload를 우선한다.

loading, discovery failure, installed target 없음, busy 상태에서는 submenu를 비활성화한다. discovery
failure는 backend 상세 오류를 UI에 반향하지 않고 고정 메시지를 사용한다. action은 exact context
repository path와 selected target ID를 기존 `open_in`에 전달하고, backend는 target intersection과
repository를 다시 확인한 뒤 versioned app-link를 만든다.

### 3. Explicit validated path copy

Files:

- `apps/repo-manager/src-tauri/src/commands.rs`
- `apps/repo-manager/src-tauri/src/lib.rs`
- `apps/repo-manager/src/api.ts`
- `apps/repo-manager/src/App.tsx`

`repository_copy_path(path)`는 사용자가 메뉴를 선택한 순간 `validated_repository`를 다시 실행한다.
다음 조건을 모두 만족한 현재 repository path만 frontend에 반환한다.

- 비어 있지 않은 absolute path
- `.`/`..` segment 없음
- canonicalize 가능한 existing directory
- directory 안에 `.git` 존재

frontend는 backend 결과를 system clipboard에 한 번 기록한다. 검증 또는 clipboard failure는 고정 오류를
표시하고 rejected raw path/backend detail을 반향하지 않으며 clipboard를 변경하지 않는다.

### 4. Validated OS file-manager open

Files:

- `apps/repo-manager/src-tauri/src/commands.rs`
- `apps/repo-manager/src-tauri/src/lib.rs`
- `apps/repo-manager/src/api.ts`
- `apps/repo-manager/src/App.tsx`

`open_repository_folder(app, path)`도 같은 current repository validator를 통과한 path만 Tauri opener의
단일 path argument로 전달한다. shell string이나 external executable을 조립하지 않는다. opener 상세
오류와 raw path는 frontend에 보내지 않고 고정된 “repository 폴더를 열 수 없습니다” 오류만 반환한다.

### 5. Worktree creation affordance without automatic mutation

Files:

- `apps/repo-manager/src/App.tsx`

“worktree 생성”은 context repository를 선택한 뒤 해당 card의 existing “새 브랜치” input으로 focus를
옮긴다. menu close의 정상 card focus restore가 끝난 다음 animation frame에 input focus를 적용한다.
branch와 target directory가 채워지고 사용자가 기존 button을 별도로 눌러야만 `create_worktree`가
호출된다. 메뉴 action 자체는 Git state를 변경하지 않는다.

### 6. Text-input and cleanup boundaries

Files:

- `apps/repo-manager/src/App.tsx`
- `apps/repo-manager/src/App.test.tsx`
- `apps/repo-manager/README.md`
- `docs/architecture.md`

card 안 `input`, `textarea`, explicit contenteditable target에서 발생한 right-click과 Shift+F10/Menu event는
repository menu hook으로 전달하지 않는다. native WebView text context와 IME behavior를 유지한다.

기존 worktree `remove 확인`은 clean 여부를 read-only로 안내할 뿐 실제 삭제하지 않는다. repository menu
항목에도 remove를 추가하지 않았다. docs에는 #364 전까지 destructive cleanup 부재를 명시했다.

### 7. Documentation and dependency boundary

Files:

- `apps/repo-manager/README.md`
- `docs/architecture.md`
- `workthrough/2026-08-26-repo-manager-context-menu.md`

새 의존성은 기존 internal workspace package `@devbox/context-menu` 하나뿐이다. native plugin, Tauri
capability, registry package, filesystem write 범위와 external service는 추가하지 않았다. dependency notice
generator로 lockfile provenance만 갱신한다.

## Test Coverage

Frontend tests cover:

- 기존 repository/worktree list와 read-only remove check 회귀
- right-click한 exact repository의 target-first selection
- exact four-item menu topology와 cleanup/remove 부재
- installed capability submenu의 exact `(appId, repository path)` routing
- Shift+F10 path copy의 backend result와 close 뒤 card focus restore
- Menu key OS folder open의 exact repository routing
- worktree create action의 exact card input focus와 backend 미호출
- text input right-click/Shift+F10 native behavior 보존
- target discovery failure의 disabled submenu와 raw error 비노출
- copy failure의 raw path/detail 비노출과 clipboard untouched
- 기존 cold/hot app-link selection/registration draft tests
- catalog capability, current selection과 truncated scan 회귀

Rust tests cover:

- explicit path copy의 current repository revalidation과 rejected path 비반향
- absolute/existing/`.git` repository validator와 traversal 거부
- scan identity와 inbound repository identity 일치
- ignored directory pruning, depth/visited truncation 경계
- catalog installed path/workspace target intersection과 source exclusion
- versioned Path/Workspace request shape
- branch status/worktree parser와 pending app-link slot 회귀

## Verification Results

PR 직전 최종 결과로 갱신한다.

### Frontend tests

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter repo-manager test -- --maxWorkers=1
Test Files  4 passed (4)
Tests      23 passed (23)
exit 0
```

### Rust tests

```text
$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p repo-manager -j1
16 passed; 0 failed
exit 0
```

### Frontend build

```text
$ NODE_OPTIONS=--max-old-space-size=768 pnpm --filter repo-manager build
vite production build passed (44 modules)
exit 0
```

## Key Decisions

1. **repository identity는 canonical key로 선택하고 action path는 backend가 다시 검증한다.** display path나
   이전 selection을 menu target으로 대신 사용하지 않는다.
2. **cross-app submenu만 catalog-driven이다.** worktree/copy/folder action은 Repo Manager가 소유한다.
3. **copy 외에는 path를 새 frontend DTO로 반환하지 않는다.** opener는 backend에서 validated path를 직접
   소비하고 detail을 숨긴다.
4. **worktree 생성 menu는 form focus action이다.** branch/target 입력과 별도 button click 없이 자동 mutation을
   하지 않는다.
5. **text input context를 보존한다.** card-level trigger가 IME/cut/copy/paste 기본 동작을 덮지 않는다.
6. **remove는 #364 전까지 구현하지 않는다.** 현재 read-only clean check를 destructive action으로 확대하지
   않으며 force delete/reset/clean도 계속 제공하지 않는다.

## Follow-up Work

- #316: commit graph/history/detail와 working-tree/commit diff를 독립 P2 PR로 구현한다.
- #317: selected stage/unstage와 commit을 credential-helper 경계 안에서 구현한다.
- #318/#319: fetch/FF-only pull/push와 dirty/detached/upstream/diverged/rebase/merge preflight를 구현한다.
- #364: merged/stale 근거, dirty/untracked/locked/main 차단, preview/result를 갖춘 safe worktree/branch cleanup을
  구현하며 그때만 danger+confirmation remove action을 추가한다.
- W1 checkpoint에서 packaged WebView2 pointer/Shift+F10/Menu key, submenu, focus restore, clipboard와 Explorer
  open evidence를 수집한다. rejected path나 credential 원문은 evidence에 남기지 않는다.
