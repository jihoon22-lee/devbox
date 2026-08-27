# Workbench 설계 — Project Workspace Orchestrator

- 상태: 제안(Proposal) — Stage 4
- 작성일: 2026-08-14
- 근거: `docs/product-opportunities.md` §15.2, §10.2, §10.3, §17.8
- 선행: PR 2(identifier), PR 6(catalog), PR 14(crates/wsl), PR 28(ProjectProfile 설계), PR 26(producer snapshot), PR 31(service 상태 API)

## 1. 제품 정의

기존 앱 UI를 한 창에 복제하는 통합 앱이 아니다. 프로젝트를 기준으로 여러 앱과
서비스를 조정하고 상태를 요약하는 orchestration shell이다.

## 2. 핵심 흐름

```text
프로젝트 선택
  → required app/WSL/cwd/포트/service read-only 사전 점검
  → 사용자의 explicit Continue
  → 실행 직전 profile/environment 재검증
  → 예상 포트 준비 확인
  → WSL Desktop layout 열기
  → Code Pad workspace 열기
  → 필요하면 API request 열기
```

## 3. 앱 간 ownership

| 데이터 | owner(writer) | reader |
|---|---|---|
| ProjectProfile | **Workbench** (단일 writer) | life-log(attribution), wsl-desktop, code-pad, run-manager, port-manager |
| integration snapshot | producer 앱 | consumer 앱 (Workbench 포함) |
| 각 앱의 DB | 해당 앱 | **아무도 직접 수정하지 않는다** |

- Workbench는 다른 앱의 DB를 직접 수정하지 않는다.
- 실행 context는 §10.3의 명시적 CLI argument로 전달한다 (custom URL scheme은 나중).
- 다른 앱과의 상태 동기화는 integration snapshot 계약(§10.1) 또는 앱별 read API를 통해.

## 4. Start Workspace — 단계와 실패 정책

각 단계는 idempotency key로 실행 기록을 남긴다 (같은 실행을 두 번 눌러도 중복 시작 없음).

| # | 단계 | 성공 기준 | 실패 시 |
|---|---|---|---|
| 1 | required app/WSL/cwd/포트/service read-only preflight | 필수 점검 통과 또는 경고 표시 | warning은 Continue 검토, failure/unavailable는 중단 |
| 2 | explicit Continue 후 실행 직전 재검증 | 같은 profile/resource identity 유지 | 변경·stale이면 environment read/child spawn 없이 중단 |
| 3 | 예상 포트 준비 확인 | 포트 open 또는 bounded retry | 전체 deadline 후 실패 표시 |
| 4 | WSL Desktop layout 열기 | 프로세스 시작 | 실패·부분 시작이면 이번 PID만 rollback |
| 5 | Code Pad workspace 열기 | 프로세스 시작 | 실패·부분 시작이면 이번 PID만 rollback |
| 6 | API request 열기 (선택) | 후속 범위 | 이 grouped PR에서는 열지 않음 |

- "이미 실행 중이던 자원"과 "Workbench가 시작한 자원"을 구분한다:
  - 사전 점검에서 이미 running이던 서비스/포트 → Workbench가 종료하지 않는다.
  - Workbench가 시작한 것만 `Stop What I Started`의 대상.
- 부분 실패 후 상태를 사용자가 이해할 수 있어야 한다 (단계별 결과 + rollback 가능 여부 표시).

## 5. 실행 기록과 idempotency

- 실행마다 `workspace_run` 레코드: { run_id, profile_id, started_at, steps: [{name, status, started_owned}] }
- idempotency key: profile_id + 시작 시각(초 단위). 같은 초에 중복 실행 요청은 무시.
- `Stop What I Started`: run 기록에서 `started_owned=true`인 자원만 정리.

## 6. ProjectProfile 저장

- 저장: `%LOCALAPPDATA%\com.devbox.workbench\project-profiles.json` (임시 파일+rename 원자 교체)
- canonical identity: `crates/wsl::canonical_project_key` 단일 규칙
- 기존 두 저장소 흡수 (PR 28 §6): wsl-desktop localStorage `wsld-projects`, life-log settings `projects`

### 6.1 services·ports 입력 계약 (#280, P1-09-17)

`ProjectProfile`의 다음 두 필드는 Workbench가 소유하는 설정이다.

```text
expectedPorts: number[]              // 1..=65535, unique, at most 128
runManagerServiceIds: string[]       // trimmed, unique, at most 128
```

- 편집 화면은 `ProjectProfile`을 직접 수정하지 않고 `ProfileDraft`를 사용한다. 포트
  입력은 쉼표로 입력한 원문을 보존하고, 서비스는 `{ key, value }` 안정 행을 사용한다.
  행의 key는 React reconciliation 전용이며 저장하지 않는다.
- 저장 직전 순서는 `draft → parse/normalize → validate → ProjectProfile DTO → IPC`다.
  빈 토큰, 숫자가 아닌 토큰, 0/65535 초과, 중복 포트와 빈/중복/제어문자/128자 초과
  서비스 ID는 저장하지 않는다. 포트 입력 문자열은 8KiB, 포트와 서비스는 각각
  128개로 제한한다.
- 공통 경계 검증은 Rust에도 중복한다. 프로필 이름은 120자, profile/service ID는
  128자, WSL distro는 128자, 경로는 `crates/filesystem`의 4KiB safe-project-path
  규칙을 사용한다. store는 최대 512개 프로필, 직렬화 결과는 4MiB까지 허용한다.
  ID 중복과 canonical project identity 중복도 store 전체 검증에서 거부한다.

### 6.2 저장소 원자성·동시성 계약

CRUD writer는 앱 상태의 단일 `ProfileStoreState` lock 안에서 다음 순서를 지킨다.

```text
load bytes (missing만 empty 허용)
  → strict JSON/version/전체 store validate
  → clone에 insert/replace/remove
  → next store validate + size check
  → 원본 bytes 재확인(CAS)
  → unique temp file + flush/sync + atomic replace
```

- 손상·잘못된 version·알 수 없는 필드·unsafe link/경로·크기 초과·읽기 오류를 빈
  store로 대체하지 않는다. 그런 상태에서는 쓰기를 중단하고 원본을 보존한다. 초기
  파일이 정말 없는 경우만 디렉터리를 만들고 새 파일을 생성한다.
- update는 기존 항목을 먼저 삭제하지 않는다. 대상 ID를 찾고 후보 store에서 새
  profile을 검증한 뒤 canonical collision까지 통과할 때만 교체한다. 실패하거나
  저장이 실패하면 메모리 후보만 버리고 원본 파일은 그대로 둔다.
- 원본 load 이후 파일 바이트가 변경되면 conflict를 반환한다. 이 정책은 같은 앱의
  병렬 IPC 요청을 lock으로 직렬화하고, 외부 편집/다른 Workbench 프로세스가 CAS 전에
  저장한 일반적인 덮어쓰기도 감지한다. 사용자는 최신 목록을 다시 읽은 뒤 재시도한다.
  Workbench 밖의 임의 writer까지 협조시키는 OS advisory lock은 이 PR 범위가 아니므로,
  마지막 CAS 확인과 atomic rename 사이의 외부 writer race는 W1에서 별도로 관찰한다.

### 6.3 오류·privacy·ownership

- 명령 계층은 경로, raw credential, arbitrary service metadata, Git/subprocess stderr를
  반환 오류에 포함하지 않는다. UI는 고정된 한국어 메시지만 표시한다.
- profile 파일은 Workbench만 쓴다. Run Manager 서비스 정의/실행은 Run Manager가
  소유하며 Workbench는 service ID와 기존 integration snapshot을 읽기만 한다.
- `run-manager` snapshot이 없으면 지정 서비스가 미실행으로 보이고, 손상/unsafe
  snapshot이면 서비스 상태를 확인할 수 없음으로 표시한다. 손상 snapshot을 빈 정상
  상태로 취급하지 않는다.
- `project environment preflight`와 template wizard는 #280 범위가 아니다. 이 PR은
  입력·저장·health 참조만 다루며 다른 앱 DB나 프로젝트 파일을 변경하지 않는다.

### 6.4 UI 접근성·비동기 상태

- 편집기는 inline region으로 유지하되 `form` submit(Enter), Escape 취소, 자동 focus,
  명시적 label/id, `aria-invalid`, `aria-describedby`, `role=alert` 오류를 제공한다.
  저장 중에는 입력·취소·중복 submit을 막고 서비스 추가 버튼은 128개에서 비활성화한다.
- 목록 refresh와 profile health 요청에는 monotonic request sequence를 둔다. 늦게 도착한
  결과는 현재 요청/선택 ID와 일치할 때만 state를 바꾼다. 저장 성공 후 refresh도 같은
  규칙을 사용하여 stale response가 새 편집 결과를 되돌리지 않는다.

### 6.5 #280 검증 fixture와 PR 경계

- 순수 Rust: strict corrupt/version/oversize/duplicate-ID·identity, service/port/path
  bounds, collision update가 기존 항목을 보존하는지 검증한다.
- React: invalid port 원문 보존, stable service row add/edit/delete/order, duplicate/
  empty/bounds validation, form submit/Escape/disabled state, stale list/health response,
  generic error가 raw secret/path를 렌더링하지 않는지 검증한다.
- 명령/저장은 `cargo test`와 `cargo check`, UI는 `pnpm build` 및 Workbench 단위 테스트,
  Windows packaged smoke(W1)로 확인한다. 기능 단위는 이 services·ports 입력/저장
  계약 하나로 한정하며, Run Manager 서비스 lifecycle·environment preflight·template
  wizard는 별도 후속 PR이다.

### 6.6 #312+#313 grouped Start Workspace 계약

이 grouped PR은 #312 project environment와 #313 workspace preflight가 같은 Start Workspace
사용자 흐름과 native revalidation 기반을 공유한다는 이유로 함께 리뷰한다. 각 issue의
acceptance, fixture, error taxonomy와 rollback은 독립적으로 유지한다.

- #313의 `workspace_preflight`는 required app capability, WSL distro 존재·running 상태,
  Windows/WSL working directory, 예상 TCP port와 Run Manager `activeServices` snapshot을
  bounded read-only로 검사한다. stopped distro를 probe 때문에 시작하지 않으며, missing/
  existing/notRunning/unsafe/unavailable와 Workbench-started provenance를 고정 DTO로 구분한다.
- UI는 명시적 Start action 뒤 review modal을 열고 warning만 Continue 가능하게 한다. failure/
  unavailable는 차단하며 Escape/Cancel, profile navigation, unmount와 late response는
  generation guard로 폐기한다. Continue 중에는 target profile과 busy/cancel lifecycle을
  고정해 double submit이나 뒤늦은 결과가 다른 profile을 덮지 못한다.
- backend는 modal snapshot을 권한으로 사용하지 않고 `start_workspace` 직전에 preflight를
  재실행한다. 실패하면 #312 source를 읽거나 child를 시작하지 않는다. 통과 뒤 profile/root/
  source identity와 environment revision/metadata를 각 child 경계에서 다시 비교하고,
  process-local zeroizing overlay만 `crates/launch`로 전달한다.
- 첫 child 이후 변경·cancel·provider failure가 발생하면 `StartedPidGuard`가 이번 transition이
  만든 PID만 rollback한다. run/restore DTO에는 stable step status와 resource provenance만
  남기고 PID, path, stderr, raw service payload, secret value/ciphertext를 저장하지 않는다.
- #313은 service create/update/start, 자동 복구와 destructive cleanup을 포함하지 않는다.
  #312는 `.env` write/upload, global/system editor, cloud secret store와 다른 앱 DB 변경을
  포함하지 않는다. 둘의 공용 rollback boundary는 Workbench-owned process만 대상으로 한다.

#### 6.6.1 grouped fixture와 verification

- Rust `core/preflight`는 installed/missing app, running/stopped/missing distro, available/
  unsafe/missing cwd, free/existing/conflict port, missing/partial/unavailable service snapshot과
  stable resource serialization을 검증한다. command probe는 fixed argv, null stdin,
  discarded stderr, 2초 stdout/child timeout과 16 KiB WSL output bound를 사용한다.
- React는 ready warning, blocking failure, explicit Continue, Escape/Cancel, stale profile/
  unmount response, Continue 중 selection lock, backend stale rejection과 provenance rendering을
  검증한다. #312 parser/preview/masked metadata fixture는 별도 test file로 보존한다.
- Linux focused test/check/fmt와 frontend test/build 뒤 packaged Windows W2에서 real installed
  capability, WSL stopped/missing, junction/reparse, port race, changed `.env`, child overlay,
  rollback/no-replace를 확인한다. service lifecycle와 API handoff는 이 PR의 acceptance가 아니다.

## 7. MVP 범위

- ProjectProfile CRUD + 기존 두 저장소 흡수
- read-only project health (Git, WSL distro, expected port, Run Manager service)
- wsl-desktop `gitStatus` 이관
- 앱 실행 context 전달 (§10.3 CLI argument)
- Start Workspace / Stop What I Started
- 단계별 실패·rollback 표시

## 8. 안전 경계

- 시작 전부터 실행 중이던 process/service는 자동 종료하지 않는다.
- 각 단계에 idempotency key 또는 실행 기록을 둔다.
- 다른 앱의 DB를 직접 수정하지 않는다.
- 앱이 없으면 Devbox Manager의 해당 앱 설치 화면으로 안내한다.
- 범용 앱·파일·웹 launcher를 만들지 않는다.

## 9. 완료 조건

- 시작 전부터 실행 중이던 process/service를 자동 종료하지 않는다
- 다른 앱의 DB를 직접 수정하지 않는다
- 앱이 없으면 Devbox Manager 설치 화면으로 안내한다
- 부분 시작 실패 후 상태를 사용자가 이해할 수 있다
- wsl-desktop에 프로젝트 목록·git 상태 코드가 남아 있지 않다
