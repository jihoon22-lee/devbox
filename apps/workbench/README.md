# workbench — Workbench (프로젝트 orchestration 셸)

기존 앱 UI를 복제하는 통합 앱이 아니라, **프로젝트를 기준으로 여러 앱·서비스를 조정하고 상태를 요약**하는 셸.
산출물: `Workbench.exe` (`apps/workbench`).

## 주요 기능

- **ProjectProfile CRUD** — wsl-desktop·life-log의 기존 프로젝트 저장소 흡수 (canonical identity 단일 규칙)
- **사전 점검(read-only health)** — Git/WSL distro/예상 포트/Run Manager 서비스 상태
- **Start Workspace** — 사전 점검 → Run Manager 서비스 시작 → 예상 포트 확인 → WSL Desktop layout·Code Pad workspace 열기 (단계별 idempotency·실패·rollback 표시)
- **Stop What I Started** — Workbench가 시작한 자원만 정리 (기존 실행 자원은 건드리지 않음)

## 기술

- 공용 크레이트 `crates/wsl`·`crates/integration`
- 실행 context는 CLI argument로 전달 (custom URL scheme 아님)
- 다른 앱의 DB를 직접 수정하지 않음, 앱 없으면 Devbox Manager 설치 화면으로 안내

## 데이터

- `%LOCALAPPDATA%\com.devbox.workbench\project-profiles.json` (원자 교체)

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: `docs/superpowers/specs/2026-08-14-workbench-design.md`
