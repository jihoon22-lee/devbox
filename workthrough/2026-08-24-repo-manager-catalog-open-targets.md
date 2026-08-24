# Repo Manager Catalog Open Targets

## Overview

P1-03-R [#240](https://github.com/jihoon22-lee/devbox/issues/240)의 Repo Manager
catalog 기반 대상 해석을 구현했다. 기존 Rust allowlist와 React 버튼 세 개를 제거하고,
runtime/build-time catalog capability와 실제 설치 executable의 교집합으로 "다른 앱으로
열기" 대상을 생성한다.

Repo Manager는 repository를 path로 갖고 있으므로 `path` 수신 capability를 메뉴 진입
조건으로 사용한다. 같은 설치 앱이 `workspace` capability도 선언하면 앱 ID를 특별 취급하지
않고 더 구체적인 Workspace payload를 선택한다. 새 앱은 catalog 선언과 설치 manifest만
충족하면 Repo Manager 코드를 바꾸지 않고 자동으로 나타난다.

이 기능은 Repo Manager의 inbound Path/single-instance, commit·diff·cleanup, 전 앱 context
menu를 구현하지 않는다. 해당 기능은 각각 분리된 후속 issue 경계를 유지한다.

## Context

v0.4.x Repo Manager에는 target 지식이 두 번 하드코딩되어 있었다.

- backend `open_in`은 `code-pad | wsl-desktop | workbench`만 허용했다.
- frontend는 같은 세 앱의 버튼과 표시명을 직접 렌더링했다.
- Code Pad만 Workspace로 보내는 payload 분기도 앱 ID 비교였다.

이 구조에서는 catalog에 path를 받을 수 있는 새 앱을 추가해도 Repo Manager 코드를 함께
고쳐야 했다. 설치되지 않은 앱도 UI에 항상 보였고, frontend가 executable 존재 여부나
custom install root를 알 수 없으므로 클릭 뒤 실패하는 항목을 숨길 수 없었다.

선행 PR #371은 `crates/launch::installed_targets(capability)`를 추가했다. 이 API는
catalog revision freshness를 통과한 capability target 중 versioned install-root locator와
Manager manifest에서 안전한 portable executable이 실제로 해석되는 앱만 반환한다. 이번
기능은 그 경계를 Repo Manager의 UI와 command authorization에 연결한다.

## Changes Made

### 1. Pure target selection policy

`src-tauri/src/core/open_targets.rs`에 Repo Manager 고유의 순수 변환을 추가했다.

- `RepoOpenTarget`: frontend에 필요한 `id`, `displayName`, `payloadKind`만 소유한다.
- `OpenPayloadKind`: `path` 또는 `workspace`만 직렬화한다.
- `select_repo_open_targets`: 설치된 path target 목록을 authoritative base로 삼는다.
- 같은 ID가 설치된 workspace target 목록에도 있으면 Workspace를 우선한다.
- source app ID는 self-open 항목이 생기지 않도록 제외한다.
- executable 경로는 frontend 모델에 포함하지 않는다.

target의 catalog 순서는 유지한다. 별도 display-name 정렬이나 allowlist를 추가하지 않아
catalog가 제품군의 일관된 순서를 계속 소유한다.

순수 fixture는 다음을 고정한다.

- `future-sixteenth`처럼 기존 코드가 모르는 ID도 path 목록에 있으면 나타난다.
- Code Pad라는 이름을 검사하지 않아도 workspace capability가 있으면 Workspace를 쓴다.
- workspace-only 또는 missing-executable로 인해 installed path 목록에 없는 target은
  메뉴에 나타나지 않는다.
- source app 자신은 제외된다.
- 선택된 payload kind가 정확한 `OpenRequest` shape를 만든다.

### 2. Backend discovery and authorization

새 Tauri command `open_targets`는 다음 두 공용 조회를 결합한다.

```text
installed_targets("path")
installed_targets("workspace")
        │
        └─ select_repo_open_targets("repo-manager", ...)
```

frontend에는 app ID, 표시명, payload kind만 반환한다. locator, manifest, install root,
executable은 공용 launch 계층 안에 남는다.

`open_in`도 UI 입력을 그대로 신뢰하지 않고 매 호출마다 같은 available target을 다시
조회한다. 요청한 app ID가 현재 catalog/install 교집합에 없으면 generic 오류로 거부한다.
따라서 DOM에서 command를 직접 호출해 제거된 앱이나 catalog capability가 없는 앱을 실행할
수 없다. 실제 process spawn 직전에는 `launch_open`이 executable을 다시 해석하므로 설치
상태가 조회와 실행 사이에 바뀌어도 안전하게 실패한다.

### 3. Repository path boundary

target 앱에 전달하기 전에 repository path를 read-only 검증한다.

- 빈 값과 상대 경로를 거부한다.
- `/`와 `\` 양쪽 구분자의 raw `.`/`..` segment를 거부한다.
- 경로가 canonicalize 가능한 실제 directory여야 한다.
- directory 안에 `.git` file 또는 directory가 존재해야 한다.
- 오류에는 요청 app ID, raw path, filesystem error를 반향하지 않는다.

검증 뒤에도 shell 문자열을 만들지 않고 `OpenRequest`와 `Command::args` 경계를 사용한다.
worktree의 `.git`은 file일 수 있으므로 directory로 제한하지 않고 existence를 확인한다.

### 4. Catalog-generated frontend

React의 세 하드코딩 버튼을 `openTargets()` 결과 map으로 교체했다. 버튼 label은 catalog의
`displayName`이며 tooltip은 전달 payload kind를 설명한다. target이 없으면 설치된 대상 앱이
없다는 비활성 안내만 표시한다. click 실패는 기존 화면의 복구 가능한 error 영역에 표시한다.

browser/mock 모드도 별도 app ID 배열을 두지 않는다. `apps/catalog.json`을 import해
`accepts`에 path가 있는 앱을 파생하고, workspace 선언을 payload kind에 반영한다. mock은
설치 filesystem이 없는 개발 환경이므로 capability 대상 전체를 설치된 것으로 간주하지만,
identity와 표시명은 production과 같은 catalog 단일 원본을 사용한다.

UI fixture는 현재 catalog에서 Code Pad, WSL Desktop, Workbench가 생성되고 Repo Manager
자신은 버튼으로 나타나지 않는지 검증한다.

### 5. Documentation

Repo Manager README는 특정 앱 세 개를 열기 기능의 고정 목록으로 설명하지 않고, catalog
capability와 실제 설치 executable 기반 discovery를 설명하도록 갱신했다. architecture에는
Repo Manager를 runtime catalog/install-root 계약의 첫 동적 UI 소비자로 기록했다.

## Design Decisions

### Path eligibility, Workspace preference

repository는 모든 대상에게 전달 가능한 일반 path이므로 메뉴 eligibility는 `accepts:path`로
고정한다. `workspace`만 받고 path를 받지 않는 앱을 암묵적으로 노출하지 않는다. 다만 path와
workspace를 모두 받는 앱에는 repository 전체를 여는 의도가 더 정확하게 전달되도록
Workspace를 우선한다.

이 정책은 현재 Code Pad 동작을 유지하지만 Code Pad ID를 알지 않는다. 향후 다른 editor가
두 capability를 선언해도 같은 규칙을 자동 적용한다.

### Backend remains authoritative

frontend 목록은 표현용이며 authorization 경계가 아니다. `open_in`이 catalog/install 상태를
다시 확인해야 stale UI, 수동 IPC, 설치 제거 race를 안전하게 처리할 수 있다. executable
경로를 frontend로 보내지 않아 path disclosure와 프론트 측 launch 재구현도 피한다.

### No external-tool registry

이 메뉴는 devbox catalog 앱 discovery만 소비한다. 외부 editor/tool 설치 탐색은 이번 기능의
범위가 아니며, Repo Manager target allowlist를 OS executable allowlist로 바꾸지 않는다.

## Verification

로컬 자원 점유를 제한하기 위해 package 단위와 단일 job으로 검증했다. frontend는 기존 main
의 dependency tree를 임시 symlink로만 연결했고 명령 종료 trap에서 즉시 제거했다.

- `cargo test -p repo-manager --lib -j 1` — 12 passed
- `cargo check -p repo-manager -j 1` — passed
- `CARGO_BUILD_JOBS=1 cargo clippy -p repo-manager --all-targets -- -D warnings` — passed
- Repo Manager TypeScript `tsc --noEmit` — passed
- `vitest run src/App.test.tsx --maxWorkers=1` — 5 passed
- `cargo fmt --all` — passed
- `git diff --check` — passed

PR GitHub Actions가 전체 frontend, Linux Rust, Windows Rust, dependency, catalog gate를
수행한다. Windows packaged smoke와 화면/로그 evidence는 계획된 W1 checkpoint에서 남긴다.

## Follow-up Boundaries

- Repo Manager inbound Path와 cold/hot single-instance는 P1-04의 별도 issue다.
- repository context menu와 keyboard/focus behavior는 P1-06의 Repo Manager PR이다.
- diff/stage/commit/fetch/FF-pull/push는 P2 Repo Manager 기능이다.
- safe worktree/branch cleanup은 P3 Repo Manager 기능이다.
