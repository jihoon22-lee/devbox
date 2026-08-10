# AGENTS.md

devbox — Tauri 8개 데스크톱 앱 모노레포. 모든 규약의 기준은 루트 `CONVENTIONS.md` (반드시 먼저 읽을 것). 앱별 상세는 `apps/<app>/PLAN.md`.

## 저장소 사실
- 원격: `https://github.com/jihoon22-lee/devbox` (로컬 디렉터리명 `devbox`와 동일)
- **로컬에 git이 아직 초기화되지 않음**: 작업 시작 전 `git init` + `git remote add origin https://github.com/jihoon22-lee/devbox.git`
- `gh` CLI로 jihoon22-lee 로그인 완료 (원격 작업 가능)
- git identity(`user.name`/`user.email`) 미설정 — 커밋 전에 설정 필요

## 현재 상태 (계획 단계)
- `apps/*/PLAN.md`만 존재, **실제 코드 없음**. `packages/`, `crates/`는 비어 있음
- 루트 `Cargo.toml`의 `[workspace] members = []` — 앱/크레이트 생성 시 반드시 멤버에 추가
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

## 함정 / 주의
- `/mnt/e`(9p 마운트)에서 cargo 컴파일은 느림 → `target-dir`을 Linux 네이티브 경로로 (`.cargo/config.toml`, `~/.cache/targets/...`)
- create-tauri-app 스캐폴드: `--yes`를 써야 하며, 생성 직후 파일 4곳의 `--name` 교체 필요 (CONVENTIONS §6)
- 순수 로직은 `apps/<app>/src-tauri/src/core/`에 두고 WSL에서 테스트. Windows 전용 코드는 `src-tauri` 명령 계층에 격리
- 공통화 원칙: 같은 코드가 두 번째 앱에서 필요해질 때만 `crates/`·`packages/`로 추출
