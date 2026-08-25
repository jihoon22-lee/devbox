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
- Life Log 입력: `%LOCALAPPDATA%\devbox\integration\life-log\v1\summary.json`의
  `projects/v1` read-only view. 누락은 no-op이고 손상·schema mismatch·unsafe path는 기존
  프로필을 유지한다. producer가 꺼져 있어도 마지막 정상 snapshot은 freshness와 함께 읽는다.

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: `docs/superpowers/specs/2026-08-14-workbench-design.md`
