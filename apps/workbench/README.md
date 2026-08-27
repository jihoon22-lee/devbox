# workbench — Workbench (프로젝트 orchestration 셸)

기존 앱 UI를 복제하는 통합 앱이 아니라, **프로젝트를 기준으로 여러 앱·서비스를 조정하고 상태를 요약**하는 셸.
산출물: `Workbench.exe` (`apps/workbench`).

## 주요 기능

- **ProjectProfile CRUD** — wsl-desktop 프로젝트와 Life Log `projects/v1` snapshot을 흡수 (canonical identity 단일 규칙)
- **사전 점검(read-only health)** — Git/WSL distro/예상 포트/Run Manager 서비스 상태
- **Start Workspace** — read-only preflight review → 예상 포트 확인 → WSL Desktop에 구체적인 경로 전달·Code Pad workspace 열기 (Run Manager 서비스 자체는 시작·변경하지 않으며, 단계별 실패·rollback 표시; 저장된 WSL Desktop layout을 보내는 기능은 아님 — 설계: [`docs/superpowers/specs/2026-08-17-app-interop-design.md`](../../docs/superpowers/specs/2026-08-17-app-interop-design.md))
- **Stop What I Started** — Workbench가 시작한 자원만 정리 (기존 실행 자원은 건드리지 않음)
- **프로필 컨텍스트 메뉴** — 우클릭/Shift+F10/Menu key로 Start Workspace, Stop What I Started, 프로필 편집, 확인 후 삭제, 검증된 경로 복사, catalog 기반 다른 앱으로 열기. 메뉴를 연 행을 먼저 선택하고 닫히면 focus 복구
- **실행 소유권 gate** — backend run을 reload 뒤 복원하고, 추적 중인 run 또는 start transition이 있으면 다른 profile start를 막는다. stop은 run/profile ID가 일치할 때만 수행하며 active profile은 stop 전까지 삭제할 수 없다.
- **서비스·포트 프로필 입력** — 편집 화면은 저장 DTO와 분리된 안정적인 draft buffer를 사용한다. 예상 포트는 원문 입력을 유지한 채 1~65535·중복·빈 토큰을 검증하고, Run Manager 서비스 ID는 행 단위로 추가·수정·삭제한다. 유효성 검사를 통과한 경우에만 저장하며, 백엔드 IPC 경계에서도 이름·경로·포트·서비스 ID를 다시 검증한다. 이 화면은 Run Manager 서비스를 생성·수정·시작하지 않는다.
- **WSL runtime 포트 제안** — `wsl-desktop/runtime/v1` read-only snapshot의 published TCP host port를 distro/container/target provenance와 함께 표시한다. 기존 예상 포트를 보존하면서 사용자가 고른 항목만 편집 draft에 추가하고, 저장 버튼 전에는 profile store를 변경하지 않는다. 2분 이하는 fresh, 2분 초과 15분 이하는 stale로 구분해 stale 반영을 다시 확인하고, 15분 초과 expired·missing·corrupt source는 반영하지 않는다. 반영 직전에 snapshot을 다시 읽어 사라진 후보와 만료 전환도 차단한다.
- **프로젝트 환경 (.env)** — 사용자가 고른 프로젝트 상대 `.env`/`.env.<name>`을 native에서 bounded UTF-8 dotenv로 확인하고, 이름·source·충돌·opaque revision·`crates/secrets` reference와 masked value만 표시·저장한다. 원문은 profile/IPC/log/clipboard에 들어가지 않으며, Start Workspace가 시작하는 child process에만 실행 직전 revision/metadata 재검증 후 ephemeral overlay로 전달한다. disabled 설정은 파일을 읽지 않고, 변수 없는 파일은 주입 없는 성공으로 처리한다. 중복·예약 이름, stale/changed file, unsafe path·symlink/reparse, malformed/oversized input과 secret backend 불가 상태는 fail-closed한다.

v0.4.1의 `Path`에는 distro나 profile 정보가 없다. 따라서 Start Workspace가 WSL Desktop으로
보내는 것은 프로필의 구체적인 경로이며, WSL Desktop은 앱에서 선택된 distro를 사용하고 선택값이
없으면 기본 distro를 사용한다. 프로필의 distro와 저장된 터미널 layout 복원은 v0.5.0으로 미룬다.

## 기술

- 공용 크레이트 `crates/wsl`·`crates/integration`·`crates/filesystem`(snapshot 경로 안전 규칙)·`crates/launch`(Start Workspace)·`crates/git`(git_status, workspace.rs)
- 공용 package `packages/context-menu`; app 고유 항목과 상태 gate는 Workbench가 소유
- 실행 context는 CLI argument로 전달 (custom URL scheme 아님)
- “다른 앱으로 열기”는 `crates/launch`가 확인한 installed `path`/`workspace` capability만 표시한다. frontend는 executable이나 profile path를 받지 않고 profile ID와 target ID만 전달하며 backend가 현재 저장소와 안전한 project path를 다시 검증한다.
- 다른 앱의 DB를 직접 읽거나 수정하지 않음, 앱 없으면 Devbox Manager 설치 화면으로 안내

## 데이터

- `%LOCALAPPDATA%\com.devbox.workbench\project-profiles.json` (원자 교체)
- 저장소는 version·프로필 ID·canonical project identity·항목 수·문자열·서비스/포트 개수·직렬화 파일 크기를 읽기와 쓰기 양쪽에서 제한한다. 파일이 없을 때만 빈 저장소를 시작하며, JSON 손상·지원하지 않는 version·알 수 없는 필드·unsafe path·중복 identity·크기 초과는 기존 파일을 보존한 채 실패한다.
- CRUD writer는 앱 수명 동안 하나의 lock으로 load → validate → replace → CAS 재검증 → atomic write를 직렬화한다. 저장 직전에 관찰한 원본 바이트가 바뀌면 충돌로 중단하므로 두 요청이 서로의 프로필을 덮어쓰지 않는다. update는 새 프로필을 별도 후보 store에서 검증한 뒤 교체하므로 canonical collision에서 기존 항목이 먼저 삭제되지 않는다.
- Life Log 입력: `%LOCALAPPDATA%\devbox\integration\life-log\v1\summary.json`의
  `projects/v1` read-only view. 누락은 no-op이고 손상·schema mismatch·unsafe path는 기존
  프로필을 유지한다. producer가 꺼져 있어도 마지막 정상 snapshot은 freshness와 함께 읽는다.
- Run Manager 입력: `%LOCALAPPDATA%\devbox\integration\run-manager\v1\summary.json`의
  flat `activeServices`를 read-only로 읽는다. snapshot 자체가 없으면 지정 서비스는 미실행으로
  보이지만, 손상·잘못된 schema·음수 uptime·중복/잘못된 ID·128개 초과는 빈 정상 상태로 축소하지
  않고 “서비스 상태를 확인할 수 없습니다”로 표시한다.
- WSL Desktop 입력: `%LOCALAPPDATA%\devbox\integration\wsl-desktop\v1\summary.json`의
  `runtime/v1` view. 공용 integration reader로 envelope·link·size·freshness를 확인한 뒤 distro
  64개, container 512개, mapping 1,024개와 문자열/identity를 다시 검증한다. host port별 source를
  정렬·중복 제거하고 `ProjectProfile.expectedPorts`가 표현할 수 없는 UDP/SCTP mapping은 제안에서
  제외한다. snapshot 절대 경로, container ID, raw Docker output/image/command/environment는
  frontend DTO에 포함하지 않는다.

### 프로젝트 환경 안전 계약 (#312, P2-14)

환경 파일 기능은 Workbench가 프로젝트 작업을 편하게 시작하도록 하는 native/offline
보강이다. 별도 도구를 다운로드하거나 global environment를 편집하지 않는다.

- `source`는 profile project root 바로 아래의 `.env` 또는 `.env.<alphanumeric/._->` 한
  파일명(최대 256 bytes)뿐이다. absolute/relative traversal/separator/colon, Windows
  filename alias가 되는 trailing dot, root·file의 symlink와 reparse point는 거부하고,
  canonical source가 canonical root 밖이면 읽지 않는다.
- native parser는 UTF-8, file 256 KiB, line 8 KiB, variable 128개, key 128 bytes, value
  64 KiB, 모든 value 합계 128 KiB로 제한한다. blank/comment와 exact `export ` prefix,
  single/double quote의 제한된 escape만 허용하며 expansion, command substitution,
  multiline shell 문법은 실행하지 않는다.
- profile에는 `enabled`, source, SHA-256 revision, 변수 metadata만 둔다. metadata는
  name/source/conflict(`none`, `duplicate`, `reserved`, `duplicateAndReserved`)와
  민감한 이름의 `secret-ref/v1` reference를 포함한다. duplicate/reserved는 preview에서
  보이지만 enabled 실행을 차단한다.
- preview는 `maskedValue`만 IPC로 반환한다. Start Workspace는 source를 다시 읽고
  revision과 metadata가 preview와 정확히 일치할 때만 값을 memory의 zeroizing holder에
  담아 WSL Desktop/Code Pad child spawn의 environment overlay에 전달한다. profile 저장,
  clipboard, telemetry, snapshot, Run Manager DB에는 raw value가 없다.
- preview(5초), health/Git/WSL/snapshot(10초), Start Workspace(30초)는 하나의
  monotonic operation budget을 공유한다. 같은 종류의 native 요청은 single-flight로
  제한하고, 새 preview/health는 이전 작업의 cancellation bit를 세운다. preview와 health의
  cancel은 per-request exact key를 사용해 늦은 navigation 요청이 새 작업을 취소하지 않는다.
  이전 작업 종료를 기다리는 pending request도 같은 exact key ticket으로 취소할 수 있고,
  blocking worker와 WSL native-child worker lease가 실제 native 작업 종료 전 slot을 유지한다.
  Start Workspace의 “시작 취소”는 Git worker와 WSL child를 kill/reap하고 blocking
  profile/snapshot worker도
  join한 뒤에만 결과를 버린다. UI의 request sequence는 늦은 결과를 버리는 마지막 방어선이다.
- 파일은 canonical root/source와 모든 existing component의 symlink/reparse 여부를 먼저
  확인한 뒤 no-follow handle을 열고, preflight handle identity와 read 후 path/file identity를
  다시 비교한다. profile 자체도 같은 경계를 사용한다. Start Workspace는 각 child 직전에
  profile revision과 `.env` metadata를 재검증하고 source를 다시 읽는다.
- child는 `crates/launch`의 fixed argv 경계를 통해서만 열리며, project overlay 외에는
  PATH/TEMP/Windows user-directory 같은 최소 runtime allowlist만 상속한다. environment가
  없거나 disabled여도 빈 overlay로 같은 경계를 적용하며, overlay는
  128개·name 128 bytes·value 64 KiB 이내이고 control/대소문자 중복을 거부한다. Windows의
  일반 path 기반 CreateProcess는 마지막 파일 교체를 완전히 원자적으로 고정하지 못하므로
  packaged W2에서 install rollback/reparse race를 별도로 확인한다.
- 환경 선택·확인은 명시적이며 저장 전에 source와 preview를 다시 확인해야 한다. native
  secret provider가 없는 플랫폼은 secret 변수를 실행하지 않고 fixed error로 멈춘다.
  #313의 required app/WSL/cwd/port/service preflight와 Run Manager lifecycle은 이 기능의
  scope가 아니다.

### Start Workspace 사전 점검과 grouped PR 경계 (#313)

이 worktree는 #312 project environment와 #313 workspace preflight를 같은 사용자 흐름인
`Start Workspace`로 묶는다. 두 acceptance는 독립된 fixture·문서·rollback 판단을 유지한다.
환경 metadata/secret provider가 실패해도 preflight가 이미 관찰한 resource를 변경하지 않고,
preflight가 실패해도 `.env`를 읽거나 child를 시작하지 않는다. 둘 중 하나가 stale이면 전체
start를 fail-closed하고 Workbench가 시작한 PID만 rollback한다.

- preflight는 앱 capability(`wsl-desktop:path`, `code-pad:workspace`), 선택한 WSL distro의
  존재·running 상태, Windows/WSL working directory, 예상 TCP port, Run Manager service
  dependency를 read-only로 확인한다. stopped/missing/unsafe/unavailable는 구분하며 WSL을
  확인만 하려고 시작하지 않는다.
- UI는 사용자가 누른 Start 뒤에만 bounded 결과 modal을 열고, `warning`은 기존 resource를
  유지한다는 설명과 함께 명시적 Continue를 요구한다. failure/unavailable는 Continue를
  막는다. Escape/Cancel, profile selection, unmount와 late response는 generation guard로
  무시하고 Continue 중 profile navigation/double submit은 target을 고정한다.
- 각 결과는 고정 key/detail과 `ResourceProvenance`(available/existing/notRunning/
  workbenchStarted 등)만 가진다. executable path, PID, service raw payload, stderr와
  credential은 IPC/DOM/log에 들어가지 않는다. backend는 UI modal을 신뢰하지 않고 child
  spawn 직전에 preflight를 다시 실행하며 resource 소유권을 다시 기록한다.
- #313는 service 생성·수정·시작, 자동 복구/강제 종료, global environment editor, cloud
  store, `.env` write/upload를 포함하지 않는다. #312의 bounded parser·masked preview·revision/
  metadata-only persistence와 child overlay 보안 경계는 별도 acceptance로 유지한다.

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

프로필 편집의 서비스·포트 입력은 `src/lib/profileEditor.ts`에서 보존·검증한다. 취소하면
저장된 `ProjectProfile`은 변하지 않고, 저장 시에는 정규화된 포트 배열과 서비스 ID 배열만
backend에 전달한다. 서비스 ID는 최대 128자·128개, 프로필 이름은 최대 120자로 제한하며
제어 문자·중복 항목·0 또는 65535 초과 포트를 거부한다. 예상 포트는 최대 128개와
제한된 입력 길이를 가지며, 프로젝트 경로는 4KiB, WSL distro는 128자로 제한한다.
프로젝트 경로와 WSL distro의 최소 유효성은 프론트와 `core/profile.rs` 양쪽에서 확인한다.
WSL distro는 공용 `crates/wsl` argv 안전 문자 규칙을 사용한다.
편집 응답은 request sequence로 보호되어 늦게 도착한 목록/health 결과가 현재 선택을
되돌리지 않는다. 저장 중에는 재요청·입력을 막고, Escape 취소·Enter 제출·label/fieldset/
aria 오류 상태를 제공한다. backend 오류는 경로·credential·subprocess stderr를 UI에
그대로 전달하지 않고 고정된 사용자 메시지로 변환한다.

### #280 범위와 실패 계약

- 포함: ProjectProfile 안의 `expectedPorts`와 `runManagerServiceIds` CRUD, stable editing
  buffer, frontend + IPC + storage validation, malformed/collision/concurrent-save fixture.
- 제외: Run Manager 서비스 자체의 생성·수정·시작, #313 project environment preflight,
  template wizard, 다른 앱 DB 직접 수정. 서비스 상태는 기존 integration snapshot을 읽기만 하며,
  snapshot 손상은 “상태를 확인할 수 없음”으로 fail-closed 처리한다.

### #281 범위와 실패 계약

- 포함: versioned WSL runtime snapshot read/validation, fresh·stale·expired·missing·corrupt 구분,
  published TCP port의 deterministic source aggregation, 명시적 선택과 accept 직전 재검증,
  기존 expectedPorts를 보존하는 draft-only merge.
- 제외: WSL/Docker command 실행, container/resource 시작·변경, producer 쓰기, Run Manager service
  ID 추론, accept 시 profile 자동 저장. raw snapshot 오류·절대 경로·Docker detail은 UI에 반향하지
  않고 source/version/freshness와 검증된 distro/container/port provenance만 표시한다.

### #312 범위와 실패 계약

- 포함: 프로젝트 상대 `.env` 선택, native bounded parser, masked preview, metadata-only
  profile persistence, revision/metadata stale check, `crates/secrets` reference와
  Start Workspace child overlay.
- 제외: raw environment value/ciphertext persistence, global/system environment editor,
  cloud secret store, `.env` 생성·수정·업로드, 다른 앱 DB 변경, Run Manager service
  lifecycle 및 #313 preflight.
- Rust/frontend 양쪽이 source/name/value/file/row bounds와 canonical/symlink/reparse
  경계를 확인한다. malformed UTF-8/quote/escape, duplicate/reserved key, changed file,
  missing file와 unavailable secret은 fixed error로 중단하며 path·credential·raw value를
  error/UI에 반향하지 않는다. empty file은 no-op으로 표시한다.
- Start Workspace child와 context-menu의 installed-app 실행은 모두 `crates/launch`의
  최소 runtime allowlist 경계를 사용한다. context-menu는 프로젝트 `.env`를 적용하지 않고
  빈 overlay만 전달한다.

설계 문서: `docs/superpowers/specs/2026-08-14-workbench-design.md`
