# workbench — Workbench (프로젝트 orchestration 셸)

기존 앱 UI를 복제하는 통합 앱이 아니라, **프로젝트를 기준으로 여러 앱·서비스를 조정하고 상태를 요약**하는 셸.
산출물: `Workbench.exe` (`apps/workbench`).

## 주요 기능

- **ProjectProfile CRUD** — wsl-desktop 프로젝트와 Life Log `projects/v1` snapshot을 흡수 (canonical identity 단일 규칙)
- **사전 점검(read-only health)** — Git/WSL distro/예상 포트/Run Manager 서비스 상태
- **Start Workspace** — 사전 점검 → Run Manager 서비스 시작 → 예상 포트 확인 → WSL Desktop에 구체적인 경로 전달·Code Pad workspace 열기 (단계별 idempotency·실패·rollback 표시; 저장된 WSL Desktop layout을 보내는 기능은 아님 — 설계: [`docs/superpowers/specs/2026-08-17-app-interop-design.md`](../../docs/superpowers/specs/2026-08-17-app-interop-design.md))
- **Stop What I Started** — Workbench가 시작한 자원만 정리 (기존 실행 자원은 건드리지 않음)

v0.4.1의 `Path`에는 distro나 profile 정보가 없다. 따라서 Start Workspace가 WSL Desktop으로
보내는 것은 프로필의 구체적인 경로이며, WSL Desktop은 앱에서 선택된 distro를 사용하고 선택값이
없으면 기본 distro를 사용한다. 프로필의 distro와 저장된 터미널 layout 복원은 v0.5.0으로 미룬다.

## 기술

- 공용 크레이트 `crates/wsl`·`crates/integration`·`crates/filesystem`(snapshot 경로 안전 규칙)·`crates/launch`(Start Workspace)·`crates/git`(git_status, workspace.rs)
- 실행 context는 CLI argument로 전달 (custom URL scheme 아님)
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
