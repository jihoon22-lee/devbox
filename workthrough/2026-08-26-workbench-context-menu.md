# Workbench Project Profile Context Menu

## Overview

Issue #256의 P1-06-WB 범위로 Workbench 프로젝트 프로필 행에 `@devbox/context-menu`를 적용했다.
mouse right-click, Shift+F10, Menu key는 같은 app-owned menu와 action 경로를 사용하며, 선택되지 않은
프로필에서 메뉴를 열면 먼저 그 행을 선택하고 닫힐 때 원래 행으로 focus를 복구한다.

정확한 topology:

```text
Start Workspace
Stop What I Started (danger)
────────
프로필 편집
삭제 (danger)
────────
경로 복사
다른 앱으로 열기 ▸
  <현재 설치된 catalog capability target>
```

기존 inline Stop과 profile 삭제는 확인 없이 실행됐고, `run` snapshot은 한 개뿐이라 다른 profile을
선택한 화면에서도 잘못된 Stop을 노출하거나 두 번째 run으로 첫 run ID를 잃을 수 있었다. 메뉴 도입과
함께 run/profile ownership을 명확히 해 backend run을 reload 뒤 복원하고, active run이나 start transition이
있으면 다른 profile start를 막는다. stop은 run ID와 profile ID가 backend에서 일치할 때만 수행하며,
active/starting profile은 stop 또는 transition 완료 전까지 삭제하지 못하게 했다.

“다른 앱으로 열기”는 앱 ID나 실행 파일을 하드코딩하지 않는다. 현재 설치된 `path`/`workspace`
capability target만 backend가 반환하고, action에는 profile ID와 target ID만 전달한다. backend는 현재
profile store와 안전한 project path를 다시 읽어 versioned app-link를 만든다.

profile template, start retry, services/ports editor, project environment/preflight, profile/layout handoff는
각 P1/P2 후속 issue의 범위이며 이 PR에 포함하지 않는다.

## Context

- sidebar는 profile name, edit, delete inline button만 제공해 행 단위 pointer/keyboard menu가 없었다.
- 우클릭한 행과 이전 `selectedId`가 다를 수 있어 action이 stale selection에 의존하면 다른 project를
  시작·편집·삭제할 수 있었다.
- 기존 delete는 confirmation 없이 즉시 `project-profiles.json`을 원자 교체했다.
- 기존 Stop은 UI가 가진 run ID만 backend에 보내 profile ownership을 재확인하지 않았다.
- 하나의 frontend `run` state가 있는 동안 다른 profile을 시작하면 첫 run이 backend registry에는 남지만
  UI가 stop에 필요한 ID를 잃는다.
- frontend reload는 기존 `run` state를 잃지만 같은 Tauri process의 backend registry는 유지하므로,
  reload 뒤 ownership을 복원하지 않으면 Start/Delete gate가 잘못 열린다.
- profile은 Windows path와 WSL/POSIX path를 함께 또는 각각 가질 수 있다. workspace payload는 Windows
  project path가 필요하고 일반 path payload/copy는 안전한 Windows path 우선, WSL path fallback이 맞다.
- catalog target과 installed executable의 교집합은 action 직전에도 바뀔 수 있어 submenu snapshot만
  신뢰해서는 안 된다.

## Changes Made

### 1. 공용 menu primitive와 target-first selection

Files:

- `apps/workbench/package.json`
- `pnpm-lock.yaml`
- `apps/workbench/src/App.tsx`
- `apps/workbench/src/App.css`

기존 workspace package `@devbox/context-menu`를 direct dependency로 추가했다. registry package,
sidecar, native menu, network service, Tauri capability는 추가하지 않았다.

각 profile row는 focusable하며 `data-profile-id`에 opaque ID만 둔다. 공용 hook의 `onBeforeOpen`은 현재
profiles collection에서 그 ID를 다시 찾고 `selectedId`와 `contextProfile`을 함께 갱신한다. refresh나
delete로 profile이 사라지면 pending target discovery를 무효화하고 열린 menu와 stale snapshot을 닫는다.

공용 package가 root/submenu viewport flip, 바깥 클릭·Esc·scroll close, 위/아래/Enter navigation,
disabled skip, focus restore를 담당한다. Workbench는 항목, profile/run 상태 gate와 action dispatch를
소유한다.

### 2. Lifecycle ownership와 confirmation

Files:

- `apps/workbench/src/App.tsx`
- `apps/workbench/src-tauri/src/commands/workspace.rs`
- `apps/workbench/src/api.ts`

상태 표:

| 상태 | Start | Stop | 편집 | 삭제 |
|---|---:|---:|---:|---:|
| tracked run 없음 | 가능 | 불가 | 가능 | 확인 후 가능 |
| context profile의 tracked run | 불가 | 확인 후 가능 | 가능 | 불가 |
| 다른 profile의 tracked run | 불가 | 불가 | 가능 | 확인 후 가능 |
| action 진행 중 | 불가 | 불가 | 불가 | 불가 |

frontend는 시작/refresh 때 `current_workspace_run`으로 backend의 단일 tracked run ownership을 복원한다.
restore DTO는 run ID와 profile ID만 포함하며 기존 step detail, PID, profile path를 다시 보내지 않는다.
둘 이상의 run이 있으면 임의 하나를 고르지 않고 fail-closed 오류로 처리한다. tracked run 하나가 있으면 모든
profile의 추가 Start를 비활성화해 기존 run ID를 잃지 않는다.
Stop은 `run.profileId === contextProfile.id`일 때만 활성화한다. backend `stop_workspace`도 `runId`와
`profileId`를 함께 받고, mismatch면 run registry에서 아무것도 제거하지 않은 채 고정 오류를 반환한다.
없는 run ID는 기존 idempotent behavior대로 0을 반환한다.

Stop 확인 문구는 “Workbench가 시작한 resource만 중지하고 시작 전부터 실행 중이던 resource는
유지한다”는 ownership을 명시한다. delete 확인은 “profile 정의만 삭제하며 project file과 이미 실행
중이던 external resource는 변경하지 않는다”는 범위를 명시한다. cancel이면 IPC를 호출하지 않는다.

backend의 단일 transition claim은 start 전체와 synchronous delete를 직렬화해 concurrent start와
start/delete race를 막는다. delete는 현재 in-memory registry에 같은 profile의 run이 있으면 거부하고,
존재하지 않는 profile ID는 성공처럼 처리하지 않는다. inline button과 context menu는 같은 handler를
사용한다. claim은 성공·오류·future cancellation에서 RAII로 해제된다.

### 3. Catalog-driven cross-app targets

Files:

- `apps/workbench/src-tauri/src/core/open_targets.rs`
- `apps/workbench/src-tauri/src/core/mod.rs`
- `apps/workbench/src-tauri/src/commands/profile_actions.rs`
- `apps/workbench/src-tauri/src/commands/mod.rs`
- `apps/workbench/src-tauri/src/lib.rs`
- `apps/workbench/src/api.ts`

`devbox_launch::installed_targets("path")`와 `installed_targets("workspace")`가 runtime/build catalog capability와
Manager locator/manifest에서 검증된 executable의 교집합을 제공한다. Workbench 자신은 제외한다. 동일
target이 두 capability를 모두 받으면 더 구체적인 `workspace` payload를 선택하고, workspace-only future
target도 안전한 Windows profile path가 있으면 노출할 수 있다.

frontend DTO는 다음 세 필드뿐이다.

```text
WorkbenchOpenTarget = id + displayName + payloadKind
```

executable과 profile path는 submenu discovery에서 반환하지 않는다. 메뉴를 열 때 exact profile ID로
target을 비동기 조회하며 pending 동안 submenu를 비활성화한다. profile이 바뀐 오래된 응답은 request
generation으로 버린다. discovery 실패는 원문 error를 반향하지 않고 고정된 복구 가능 메시지를
표시한다.

### 4. Safe profile path와 versioned app-link

Files:

- `apps/workbench/src-tauri/src/core/open_targets.rs`
- `apps/workbench/src-tauri/src/commands/profile_actions.rs`

backend는 action마다 현재 `project-profiles.json`에서 profile ID를 다시 찾고
`devbox_filesystem::parse_safe_project_path`로 다음을 검증한다.

- 최대 4,096 bytes
- absolute Windows drive, UNC share 아래 project, 또는 POSIX project path
- root 자체, relative/traversal, control character, Windows device path/alias, unsafe component 거부
- workspace payload는 Windows drive/UNC path만 허용
- path payload와 명시적 copy는 안전한 Windows path 우선, 없으면 안전한 WSL/POSIX path 사용

`profile_open_targets(profileId)`는 profile이 실제로 만들 수 없는 payload target을 제거한다.
`open_profile_in(profileId, appId)`은 target 목록과 path를 다시 계산하고 두 opaque ID가 여전히 유효할 때만
`OpenRequest`를 만든다. source는 `workbench`이고 payload는 `OpenTarget::Path` 또는
`OpenTarget::Workspace`다. `devbox_launch::launch_open`이 versioned argv와 현재 executable을 다시
해석한다. 오류에는 전달된 target ID나 path 원문을 포함하지 않는다.

### 5. Explicit path copy

Files:

- `apps/workbench/src/App.tsx`
- `apps/workbench/src/api.ts`
- `apps/workbench/src-tauri/src/commands/profile_actions.rs`

profile DTO에 이미 path가 있더라도 stale frontend 값을 바로 복사하지 않는다. 사용자가 “경로 복사”를
선택한 순간 backend `profile_copy_path(profileId)`가 현재 store를 다시 읽어 같은 safe-path validator를
통과한 값만 반환한다. frontend는 그 응답을 system clipboard plain text에 한 번 기록하며 snapshot,
log, settings를 추가로 만들지 않는다. path가 없거나 검증에 실패하면 clipboard를 변경하지 않는다.

### 6. Documentation and dependency boundary

Files:

- `apps/workbench/README.md`
- `docs/architecture.md`
- `workthrough/2026-08-26-workbench-context-menu.md`

README에는 exact menu, run ownership gate, installed capability와 path validation 경계를 기록했다.
architecture에는 Workbench row selection, stop/delete confirmation, run/profile matching, cross-app DTO와
safe path 흐름을 추가했다.

새 의존성은 locked internal workspace package 하나뿐이다. dependency notice generator로 lockfile
provenance를 갱신하며 registry dependency, license surface, CSP, filesystem/network capability에는 변화가
없다.

## Test Coverage

Frontend tests cover:

- profile 자동 선택과 health 회귀
- right-click한 exact profile target 우선 선택
- exact six-action menu topology와 danger/disabled state
- Shift+F10 exact Start와 close 후 row focus restore
- Menu key exact profile edit
- active run의 same/different profile Start/Stop/Delete gate
- frontend reload 뒤 backend run ownership 복원과 menu gate
- Stop cancel/confirm과 exact `(runId, profileId)` routing
- delete cancel/confirm과 exact profile removal
- backend-validated path copy와 clipboard value
- catalog-derived submenu target과 exact `(profileId, appId)` routing
- discovery failure의 disabled submenu와 raw error 비노출
- 기존 empty profile save guard와 app-link listener tests

Rust tests cover:

- path/workspace installed capability selection, source exclusion, workspace preference와 future workspace-only target
- safe Windows path priority와 safe POSIX fallback
- POSIX-only profile의 workspace target 제거
- traversal/unsafe path failure의 raw value 비노출
- selected installed target만 versioned `OpenRequest`로 변환
- mismatched run/profile stop이 registry entry를 보존
- active profile run detection, exact take, missing run idempotency
- transition claim의 concurrent start 차단·RAII release와 multiple-run restore fail-closed
- restore ownership DTO가 step detail, PID와 raw path를 직렬화하지 않음
- 기존 profile identity, health, app-link pending slot과 Life Log snapshot consumer 회귀

## Verification Results

PR 직전 최종 검증 결과로 갱신한다.

### Frontend tests

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter workbench test -- --maxWorkers=1
Test Files  3 passed (3)
Tests      18 passed (18)
exit 0
```

### Rust tests

```text
$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p workbench --lib -j 1
30 passed; 0 failed
exit 0
```

### Frontend build

```text
$ NODE_OPTIONS=--max-old-space-size=768 pnpm --filter workbench build
vite production build passed
exit 0
```

## Key Decisions

1. **Start는 frontend가 추적 가능한 run 하나로 제한한다.** 두 번째 start로 첫 run ID를 잃는 것보다 현재
   run을 명시적으로 stop한 뒤 전환하는 것이 Stop What I Started ownership을 보존한다.
2. **Stop은 run ID만으로 실행하지 않는다.** profile ID를 함께 검증하고 mismatch면 registry와 process를
   변경하지 않는다.
3. **active profile delete는 fail-closed다.** profile 정의를 먼저 지우면 UI가 run ownership을 설명하고
   stop할 기준을 잃는다.
4. **backend run은 reload 뒤 복원하고 start/delete transition을 claim한다.** UI state만 진실로 삼지 않고,
   concurrent transition과 multiple-run 손상 상태를 fail-closed로 처리한다.
5. **cross-app target은 catalog/installed state가 소유한다.** frontend나 Workbench가 app allowlist 또는
   executable path를 하드코딩하지 않는다.
6. **workspace는 Windows path가 있을 때만 만든다.** generic receiver에 distro 없는 POSIX 값을 workspace로
   보내지 않으며, 일반 path target은 safe WSL/POSIX fallback을 사용할 수 있다.
7. **copy는 explicit action 때 현재 store를 다시 읽는다.** 평소 target discovery에서는 profile path를
   frontend로 추가 노출하지 않는다.

## Follow-up Work

- P1-09: Workbench services/ports editor와 WSL runtime suggestion을 독립 PR로 구현한다.
- P2-14: project environment secret reference와 required app/distro/path/port/service dependency preflight를
  구현한다.
- P3: profile template, run retry, richer orchestration history는 별도 후보 PR로 유지한다.
- W1 checkpoint에서 packaged WebView2의 pointer/Shift+F10/Menu key, submenu flip, focus restore, clipboard,
  installed target launch, Stop/Delete confirmation과 ownership evidence를 수집한다.
