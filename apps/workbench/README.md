# workbench — Workbench (프로젝트 orchestration 셸)

기존 앱 UI를 복제하는 통합 앱이 아니라, **프로젝트를 기준으로 여러 앱·서비스를 조정하고 상태를 요약**하는 셸.
산출물: `Workbench.exe` (`apps/workbench`).

## 주요 기능

- **ProjectProfile CRUD** — wsl-desktop 프로젝트와 Life Log `projects/v1` snapshot을 흡수 (canonical identity 단일 규칙)
- **사전 점검(read-only health)** — Git/WSL distro/예상 포트/Run Manager 서비스 상태
- **Start Workspace** — 사전 점검 → Run Manager 서비스 시작 → 예상 포트 확인 → WSL Desktop에 구체적인 경로 전달·Code Pad workspace 열기 (단계별 idempotency·실패·rollback 표시; 저장된 WSL Desktop layout을 보내는 기능은 아님 — 설계: [`docs/superpowers/specs/2026-08-17-app-interop-design.md`](../../docs/superpowers/specs/2026-08-17-app-interop-design.md))
- **Stop What I Started** — Workbench가 시작한 자원만 정리 (기존 실행 자원은 건드리지 않음)
- **프로필 컨텍스트 메뉴** — 우클릭/Shift+F10/Menu key로 Start Workspace, Stop What I Started, 프로필 편집, 확인 후 삭제, 검증된 경로 복사, catalog 기반 다른 앱으로 열기. 메뉴를 연 행을 먼저 선택하고 닫히면 focus 복구
- **실행 소유권 gate** — backend run을 reload 뒤 복원하고, 추적 중인 run 또는 start transition이 있으면 다른 profile start를 막는다. stop은 run/profile ID가 일치할 때만 수행하며 active profile은 stop 전까지 삭제할 수 없다.
- **서비스·포트 프로필 입력** — 편집 화면은 저장 DTO와 분리된 안정적인 draft buffer를 사용한다. 예상 포트는 원문 입력을 유지한 채 1~65535·중복·빈 토큰을 검증하고, Run Manager 서비스 ID는 행 단위로 추가·수정·삭제한다. 유효성 검사를 통과한 경우에만 저장하며, 백엔드 IPC 경계에서도 이름·경로·포트·서비스 ID를 다시 검증한다. 이 화면은 Run Manager 서비스를 생성·수정·시작하지 않으며 WSL runtime 자동 제안은 후속 기능이다.

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
- 제외: Run Manager 서비스 자체의 생성·수정·시작, project environment preflight, template
  wizard, 다른 앱 DB 직접 수정. 서비스 상태는 기존 integration snapshot을 읽기만 하며,
  snapshot 손상은 “상태를 확인할 수 없음”으로 fail-closed 처리한다.

설계 문서: `docs/superpowers/specs/2026-08-14-workbench-design.md`
