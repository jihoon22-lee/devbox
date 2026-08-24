# devbox-manager — Devbox Manager

devbox 앱의 설치·업데이트·실행을 한 곳에서 관리하는 앱. GitHub Releases의 manifest를 단일 원본으로 신뢰한다.
산출물: `DevboxManager.exe` (`apps/devbox-manager`).

## 주요 기능

- **카탈로그 조회** — 설치 가능한 devbox 앱 목록 (휴대용/설치 패키지)
- **설치·업데이트·실행** — 휴대용 exe 또는 설치 패키지 선택, 버전별 관리·롤백
- **안전 다운로드** — 허용 호스트 정책, SHA-256·크기 검증, `.partial` 스트리밍
- **런타임 discovery 발행** — revision 기반 runtime catalog와 versioned install-root locator를 원자 갱신
- **환경 진단(dev environment doctor)** — WSL/git/node/pnpm/rustc/cargo/devbox-data/catalog-ids/runtime-metadata 점검
- **실행** — 설치된 앱 실행

## 기술

- `apps/catalog.json`(앱 단일 원본) + 릴리스 `release-manifest.json`만 신뢰
- 공용 catalog: `%LOCALAPPDATA%\devbox\catalog.json`
- install-root locator: `%LOCALAPPDATA%\devbox\install-roots\v1\registry.json`
- 설치 manifest는 Manager 데이터 root의 `registry.json`이 소유하며, locator에는 canonical root와 manifest 경로만 기록
- runtime catalog는 build-time revision보다 낮으면 교체하고, 더 최신이면 보존한다. locator가 유효한 뒤의 manifest/path 오류는 legacy root로 우회하지 않는다.
- `reqwest` + redirect 호스트 정책 (`crates/...` 아니고 앱 내 `core/url_policy`)

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`
