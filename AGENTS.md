# AGENTS.md

devbox — Tauri 13개 데스크톱 앱 모노레포. v0.4.1 안정판을 기준으로 v0.5.0 개발에 착수한 상태다. 모든 규약의 기준은 루트 `CONVENTIONS.md` (반드시 먼저 읽을 것). 앱별 상세는 각 `apps/<app>/README.md` 또는 `docs/superpowers/specs/` 설계 문서.

## 저장소 사실
- 원격: `https://github.com/jihoon22-lee/devbox` (로컬 디렉터리명 `devbox`와 동일)
- git 초기화·원격 연결 완료 (`main`), CI 워크플로 동작 중
- `gh` CLI로 jihoon22-lee 로그인 완료 (원격 작업 가능)
- git identity: `jihoon22.lee <zkemzld1004@gmail.com>` (전역·로컬 설정됨)

## 현재 상태
- 13개 앱 모두 구현 완료 (v0.4.1 안정판 배포 완료, v0.5.0 개발 착수): port-manager, developer-toolbox, wsl-desktop, api-playground,
  everything-plus, knowledge-base, life-log, devbox-manager, code-pad, run-manager, workbench, webhook-lab, repo-manager
- 공용 크레이트: `crates/wsl`·`search`·`integration`·`secrets`·`filesystem`·`markdown`·`process`·`git`·`launch`·`applink`·`catalog`
- 공용 패키지: `packages/tokens`·`editor`·`diff-view`·`context-menu`
- 루트 `Cargo.toml`의 `[workspace] members`에 앱/크레이트가 생길 때마다 추가해야 함
- 프론트 워크스페이스는 `pnpm-workspace.yaml` + 루트 `package.json` (packageManager: pnpm@9)

## 명령
- 프론트: `pnpm install` / `pnpm build` / `pnpm dev` — **pnpm이지 npm이 아님**
- Rust(WSL 개발): `cargo test`(순수 로직) / `cargo check`(src-tauri 컴파일 검증)
  - 새 셸에서 Rust 사용 전 `source ~/.cargo/env`
- 실제 앱 실행·배포 빌드는 **Windows**에서만: `pnpm tauri dev` / `pnpm tauri build`
- WSL에서 src-tauri 컴파일엔 Linux 라이브러리 필요:
  `libwebkit2gtk-4.1-dev libgtk-3-dev build-essential libssl-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev patchelf`

## 워크플로 (필수 규칙)
- 브랜치: `feat/<app>/<scope>` (예: `feat/port-manager/netstat-parser`) — CONVENTIONS §8
- **기능 단위 1개 = PR 1개**. 여러 기능을 한 PR에 묶지 않음
- **모든 PR은 GitHub Actions CI(`.github/workflows/ci.yml`) 통과 후에만 main으로 머지**
- 커밋: Conventional Commits, 영어, 현재형 — `feat(port-manager): add netstat parser`
- 코드 산출물의 완료 정의: `cargo test` + `cargo check` + `pnpm build` 통과
- PR 머지 또는 작업 종료 시 같은 작업 안에서 전용 worktree가 clean이고 머지된 상태인지 확인한 뒤, 전용 worktree 제거 및 `git worktree prune`, 로컬 작업 브랜치 삭제, 원격 작업 브랜치 삭제를 순서대로 수행한다. 활성·잠김·미머지·dirty worktree는 삭제하지 말고 사용자에게 즉시 보고한다. 자동 생성 worktree나 호스트가 소유한 worktree는 무단으로 삭제하지 않는다.
- `완료`를 보고하기 전에 `git worktree list`와 로컬·원격 브랜치 목록을 다시 확인해 작업 잔존 여부를 검증한다.

## 함정 / 주의
- `/mnt/e`(9p 마운트)에서 cargo 컴파일은 느림 → `target-dir`을 Linux 네이티브 경로로 (`.cargo/config.toml`, `~/.cache/targets/...`)
- create-tauri-app 스캐폴드: `--yes`를 써야 하며, 생성 직후 파일 4곳의 `--name` 교체 필요 (CONVENTIONS §6)
- 순수 로직은 `apps/<app>/src-tauri/src/core/`에 두고 WSL에서 테스트. Windows 전용 코드는 `src-tauri` 명령 계층에 격리
- 공통화 원칙: 같은 코드가 두 번째 앱에서 필요해질 때만 `crates/`·`packages/`로 추출
