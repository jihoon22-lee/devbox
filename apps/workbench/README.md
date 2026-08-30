# workbench — Workbench (프로젝트 orchestration 셸)

기존 앱 UI를 복제하는 통합 앱이 아니라, **프로젝트를 기준으로 여러 앱·서비스를 조정하고 상태를 요약**하는 셸.
산출물: `Workbench.exe` (`apps/workbench`).

## 주요 기능

- **ProjectProfile CRUD** — wsl-desktop 프로젝트와 Life Log `projects/v1` snapshot을 흡수 (canonical identity 단일 규칙)
- **사전 점검(read-only health)** — Git/WSL distro/예상 포트/Run Manager 서비스 상태.
  WSL profile은 `wsl.exe -l -v`에서 해당 distro가 이미 Running인지 먼저 확인하고,
  stopped/missing/unavailable이면 distro-scoped Git을 실행하거나 distro를 시작하지 않는다.
  Running일 때만 profile의 distro와 POSIX cwd를 구조화한 target으로 Git을 조회한다.
- **Start Workspace** — read-only preflight review → 예상 포트 확인 → WSL Desktop에 구체적인 경로 전달·Code Pad workspace 열기 (Run Manager 서비스 자체는 시작·변경하지 않으며, 단계별 실패·rollback 표시; 저장된 WSL Desktop layout을 보내는 기능은 아님 — 설계: [`docs/superpowers/specs/2026-08-17-app-interop-design.md`](../../docs/superpowers/specs/2026-08-17-app-interop-design.md))
- **Stop What I Started** — Workbench가 시작한 자원만 정리 (기존 실행 자원은 건드리지 않음)
- **프로필 컨텍스트 메뉴** — 우클릭/Shift+F10/Menu key로 Start Workspace, Stop What I Started, 프로필 편집, 확인 후 삭제, 검증된 경로 복사, catalog 기반 다른 앱으로 열기. 메뉴를 연 행을 먼저 선택하고 닫히면 focus 복구
- **실행 소유권 gate** — backend run을 reload 뒤 복원하고, 추적 중인 run 또는 start transition이 있으면 다른 profile start를 막는다. stop은 run/profile ID가 일치할 때만 수행하며 active profile은 stop 전까지 삭제할 수 없다.
- **서비스·포트 프로필 입력** — 편집 화면은 저장 DTO와 분리된 안정적인 draft buffer를 사용한다. 예상 포트는 원문 입력을 유지한 채 1~65535·중복·빈 토큰을 검증하고, Run Manager 서비스 ID는 행 단위로 추가·수정·삭제한다. 유효성 검사를 통과한 경우에만 저장하며, 백엔드 IPC 경계에서도 이름·경로·포트·서비스 ID를 다시 검증한다. 이 화면은 Run Manager 서비스를 생성·수정·시작하지 않는다.
- **WSL runtime 포트 제안** — `wsl-desktop/runtime/v1` read-only snapshot의 published TCP host port를 distro/container/target provenance와 함께 표시한다. 기존 예상 포트를 보존하면서 사용자가 고른 항목만 편집 draft에 추가하고, 저장 버튼 전에는 profile store를 변경하지 않는다. 2분 이하는 fresh, 2분 초과 15분 이하는 stale로 구분해 stale 반영을 다시 확인하고, 15분 초과 expired·missing·corrupt source는 반영하지 않는다. 반영 직전에 snapshot을 다시 읽어 사라진 후보와 만료 전환도 차단한다.
- **프로젝트 환경 (.env)** — 사용자가 고른 프로젝트 상대 `.env`/`.env.<name>`을 native에서 bounded UTF-8 dotenv로 확인하고, 이름·source·충돌·opaque revision·`crates/secrets` reference와 masked value만 표시·저장한다. 원문은 profile/IPC/log/clipboard에 들어가지 않으며, Start Workspace가 시작하는 child process에만 실행 직전 revision/metadata 재검증 후 ephemeral overlay로 전달한다. disabled 설정은 파일을 읽지 않고, 변수 없는 파일은 주입 없는 성공으로 처리한다. 중복·예약 이름, stale/changed file, unsafe path·symlink/reparse, malformed/oversized input과 secret backend 불가 상태는 fail-closed한다.
- **Dependencies / Packages (#484)** — 기존 app/distro/path/port/service 점검은
  `Environment`로 유지하고, `Packages`는 Repo Manager의
  `dependency-summary/v1` aggregate를 read-only로 표시한다. package name이나 경로를 받지 않고
  전체·직접·전이·중복·lockfile 진단과 ecosystem별 개수만 보여 준다.
- **Run Manager workspace task control (#486)** — 동기화된 task의 안전한 상태만 표시하고,
  Start/Stop은 opaque revision을 포함한 typed one-time handoff로 Run Manager에 전달한다.
  실제 실행 전 Run Manager 확인 화면을 거치며, 요청 결과는 receipt로 확인한다.

v0.4.1의 `Path`에는 distro나 profile 정보가 없다. 따라서 Start Workspace가 WSL Desktop으로
보내는 것은 프로필의 구체적인 경로이며, WSL Desktop은 앱에서 선택된 distro를 사용하고 선택값이
없으면 기본 distro를 사용한다. 프로필의 distro와 저장된 터미널 layout 복원은 v0.5.0에서
구현됐다. v0.5.1에서도 이 계약을 유지하며, historical v0.5.0 package와 v0.5.1의 정확한
tag/workflow evidence는 각 GitHub Release에서 구분해 확인한다.

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
  WSL UNC entry는 host path뿐 아니라 원래 distro와 case-sensitive POSIX path도 함께
  `WslProfile`로 복원하므로 health Git이 Windows native runner로 되돌아가지 않는다.
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
- Repo Manager 입력: `%LOCALAPPDATA%\devbox\integration\repo-manager\v1\summary.json`의
  `dependency-summary/v1` view. 선택 profile의 canonical project identity를 동일한
  namespace-separated SHA-256 opaque ID로 변환해 최대 256개 entry 중 하나만 찾는다. 전체 view를
  strict/deny-unknown 계약과 package 4,096개·edge 16,384개·입력 진단 256개 상한으로 검증하고,
  per-project `scannedAtMs` 기준 24시간 이하는 fresh, 7일 이하는 stale, 이후 expired로 구분한다.
  missing/corrupt는 서로 다른 상태이며 package manager, build script, repository 파일이나 다른 앱
  DB를 직접 열지 않는다. IPC에는 aggregate와 opaque revision만 반환한다.

- Run Manager workspace task 입력: `%LOCALAPPDATA%\devbox\integration\run-manager\v1\workspace-tasks.json`
  named view. Workbench에는 task id, 표시 label, source revision, `process`/`shell` 종류,
  source/shell trust, availability, dependency 존재 여부와 active-operation boolean만 들어온다. command, cwd, argv,
  environment 값·경로·process ID는 snapshot에 포함하지 않는다. snapshot이 없거나 envelope/view
  schema가 맞지 않으면 task panel을 정상 실행 목록으로 축소하지 않고 읽기 오류로 표시한다.
  Start/Stop 버튼은 현재 snapshot의 exact task id와 revision을 사용해 Run Manager의
  `task-control/v1` one-time handoff를 만들며, Workbench가 Run Manager DB나 process tree를
  직접 읽거나 변경하지 않는다.

### Run Manager task 확인과 receipt

Workbench의 task panel은 요청을 전송한 뒤 실행을 완료했다고 가정하지 않는다. Run Manager가
handoff를 claim하고 현재 task revision을 확인한 다음 자체 창에서 사용자의
명시적 확인을 받는다. 확인 화면에서는 task label/kind, action과 revision preview만 보여 주며
명령·경로·환경변수는 전달하지 않는다. 확인 중에는 one-time claim lease를 제한적으로 갱신하고,
거절·취소·만료·stale revision은 실행 없이 고정 오류로 끝낸다. Start는 Run Manager가
source를 다시 검증한 뒤 새 workspace-task operation을 만들고, Stop은 그 task가 root인
active operation의 exact owned child만 대상으로 한다.

Run Manager가 기록하는 receipt는 request/task/action, `accepted`/`rejected`/`started`/
`stopped`/`failed` 상태, owned operation id, timestamp와 고정 failure code만 가진다.
Workbench는 request id·task id·action이 일치하는 receipt만 표시하고, receipt snapshot에
명령·경로·environment·expected revision을 다시 노출하지 않는다. Run Manager 창이 없거나
구버전이면 자동 실행하지 않고 요청을 확인할 수 없는 상태로 남긴다.

Dependency Packages와 Run Manager task-control 패널은 사용자 기능 추가이므로 Workbench의
Cargo/package/Tauri 버전은 v0.3.0으로 함께 올린다.

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
  제한하고, 새 preview/health는 같은 read-only operation family의 이전 작업만
  cancellation bit를 세운다. 서로 다른 health surface는 같은 lane에서 순차 실행되며,
  read-only refresh는 진행 중인 `workspace-start` mutation을 취소하지 않는다. preview와 health의
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
start를 fail-closed하고 Workbench가 시작한 opaque owned receipt만 rollback한다. 검토한 상태가
유지된 채 개별 앱 실행만 실패한 경우에는 성공한 앱과 고정된 실패 단계를 부분 run으로 게시하며,
사용자가 `Stop What I Started`로 Workbench-owned process tree를 정리한다.

- preflight는 앱 capability(`wsl-desktop:path`, `code-pad:workspace`), 선택한 WSL distro의
  존재·running 상태, Windows/WSL working directory, 예상 TCP port, Run Manager service
  dependency를 read-only로 확인한다. stopped/missing/unsafe/unavailable는 구분하며 WSL을
  확인만 하려고 시작하지 않는다. Windows probe는 drive/UNC, WSL probe는 POSIX 경로만
  허용하고, 최종 대상 metadata보다 먼저 모든 existing component의 symlink/reparse 여부를
  확인하므로 link 아래의 아직 없는 descendant도 일반 `missing`으로 낮추지 않는다.
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

### P3-14 resilience/inspection 후보 (#359, #360, #361)

세 이슈는 `프로필 템플릿 선택 → 새 프로젝트 wizard → dependency health 확인 →
Start Workspace 결과 → 실패 단계 retry`라는 하나의 Workbench 사용자 흐름으로
검토한다. 전용 구현 후보와 acceptance/rollback 표는
[`docs/superpowers/plans/2026-08-28-workbench-resilience-tools.md`](../../docs/superpowers/plans/2026-08-28-workbench-resilience-tools.md)에
기록되어 있으며, 세 이슈의 데이터·관찰·실행 경계는 서로 섞지 않는다.

- **프로필 템플릿과 wizard (#359)** — `%LOCALAPPDATA%\com.devbox.workbench\profile-templates.json`
  별도 저장소에 bounded template CRUD를 둔다. 이름, 선택적 Windows/WSL/Git 기본
  경로, 예상 port와 Run Manager service ID만 저장하며 `.env`, secret reference,
  raw value/ciphertext는 저장하지 않는다. template read는 템플릿 배열과 native가
  계산한 opaque SHA-256 revision을 함께 반환한다. renderer는 편집/삭제 당시 읽은
  revision을 expected revision으로 보내고, backend는 profile store lock을 잡은
  critical section 안에서 현재 revision을 다시 확인한 뒤 atomic write한다. stale
  revision이면 변경을 거부하고, wizard는 사용자가 입력한 값을 우선하고 비어 있는
  field에만 기본값을 적용한다. template/profile validation, symlink/reparse 거부,
  revision CAS와 atomic write가 실패하면 기존 파일과 프로젝트 파일을 보존한다.
- **Dependency health (#360)** — `dependency_health`와 `workspace_preflight`는
  `health_operation` single-flight lane에서 Start Workspace/project health와
  native probe를 직렬화한다. Start Workspace preflight의 bounded DTO와 probe를
  그대로 재사용해 required app capability, distro/path, port와 service dependency를
  `pass/warning/failure/unavailable` 및 `ResourceProvenance`로 표시한다. read-only
  관찰만 하며 앱 설치, WSL/service 시작, 자동 복구와 외부 DB 변경은 하지 않는다.
  stale 응답은 현재 profile 화면을 덮어쓰지 않는다.
- **Idempotent retry (#361)** — 실패한 `wait-port → open-wsl-desktop → open-code-pad`
  suffix만 다시 실행하고, 성공한 단계는 다시 실행하지 않는다. process provenance는
  과거 상태만으로 skip하지 않고 backend가 보관한 cloneable `OwnedProcess` receipt의
  authoritative liveness를 다시 확인한다. 실제로 살아 있는 Workbench-owned process만
  건너뛰며, 종료된 owned process와 receipt가 없는 `Existing` 관찰은 retry 대상이 된다.
  profile/preflight 뒤와 각 child 경계에서 liveness를 다시 샘플링해 port 대기 중 종료·재기동된
  상태도 반영한다. profile/preflight/environment를 실행 직전에 재검증하고, 전환 무결성이
  깨지면 이번 retry가 새로 만든 owned receipt만 rollback한다.
  일반적인 앱 launch failure는 고정된 partial run으로 남겨 `Stop What I Started`가
  Workbench-owned process tree만 정리하게 한다. owned authority가 bounded stop을
  거부하면 실행 기록에 남겨 후속 Stop 재시도가 가능하다. 서비스 자동 시작이나 전체
  Workspace 재시작은 범위 밖이다.

`crates/launch::OwnedProcess`는 Windows에서 CREATE_SUSPENDED child의 sole primary
thread를 Job Object에 할당한 뒤 한 번만 resume하고, Job active-process accounting이
0이 될 때까지 전체 tree를 기다린다. Unix에서는 launch-time private process group에
TERM→KILL과 bounded full-group disappearance check를 적용한다. 모든 clone은 같은
authority를 공유하고 root `Child`는 weak background reaper가 회수하며, root 종료 직후
남은 group도 즉시 정리한다. Unix/macOS의 numeric process group은 Windows Job handle과
동일한 kernel identity가 아니므로 group이 비고 terminal 상태를 관찰하기 전 같은 ID가
재사용되는 극히 짧은 경쟁과, group 밖으로 탈출하는 malicious `setsid()` descendant는
잔여 경계다. macOS에는 Linux `/proc` identity fallback을 사용하지 않는다.

구현 보강 단계에서는 동시 작업과 `/mnt/e` 9p I/O를 고려해 검증 worker를 직렬화했다.
이번 owned-receipt/process-tree 변경은 `cargo fmt --all`과 `git diff --check`만 수행했으며,
parent가 `cargo check`·`cargo test`·`clippy`·TypeScript/프론트 빌드, CI와 Windows packaged
acceptance를 PR 직전에 최신 소스로 다시 수행해야 한다.
WSL의 Windows GNU source check는 Tauri build script 단계에서 호스트에
`x86_64-w64-mingw32-windres`가 없어 중단되었으므로, 이는 소스 오류가 아닌 toolchain
환경 제약이며 Windows packaged acceptance를 별도로 통과해야 한다.

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
