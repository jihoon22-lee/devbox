# devbox-manager — Devbox Manager

devbox 앱의 설치·업데이트·실행을 한 곳에서 관리하는 앱. GitHub Releases의 manifest를 단일 원본으로 신뢰한다.
산출물: `DevboxManager.exe` (`apps/devbox-manager`).

## 주요 기능

- **카탈로그 조회** — 설치 가능한 devbox 앱 목록 (휴대용/설치 패키지)
- **설치·업데이트·실행** — 휴대용 exe 또는 설치 패키지 선택, 버전별 관리·롤백
- **일괄 설치·업데이트** — 다중 선택, 앱별 독립 성공/실패 결과, 실패 항목만 재시도. batch는
  release manifest와 HTTP client를 한 번만 준비하고 최대 32개를 순차 처리한다.
- **검증된 설치 경로 표시** — app별 executable, install root, source manifest를 명시적 읽기 전용
  패널에 표시한다. portable만 실제 executable/root를 제공하고 installer 위치는 추측하지 않는다.
- **앱 행 컨텍스트 메뉴** — 우클릭/Shift+F10/Menu key로 설치·업데이트, 실행, 이전 버전 롤백, 설치 폴더 열기, 설치 경로 정보, 확인 후 제거. 메뉴를 연 행을 먼저 선택하고 닫히면 해당 행으로 focus 복구
- **안전 다운로드** — 허용 호스트 정책, SHA-256·크기 검증, `.partial` 스트리밍
- **Manager 소유 portable 경계** — catalog 대상·검증된 버전·기본 설치 layout·canonical registry executable이 모두 일치할 때만 실행/폴더 열기/제거. 제거 전 symlink·Windows reparse point와 bounded tree를 검사하며 별도 앱 사용자 데이터는 기본 보존
- **런타임 discovery 발행** — revision 기반 runtime catalog와 versioned install-root locator를 원자 갱신
- **환경 진단(dev environment doctor)** — WSL/git/node/pnpm/rustc/cargo/devbox-data/catalog-ids/runtime-metadata 점검
- **실행** — 설치된 앱 실행

## 기술

- `apps/catalog.json`(앱 단일 원본) + 릴리스 `release-manifest.json`만 신뢰
- 공용 catalog: `%LOCALAPPDATA%\devbox\catalog.json`
- install-root locator: `%LOCALAPPDATA%\devbox\install-roots\v1\registry.json`
- 설치 manifest는 Manager 데이터 root의 `registry.json`이 소유하며, locator에는 canonical root와 manifest 경로만 기록
- frontend의 설치·현재 버전 DTO에는 executable path를 포함하지 않는다. lifecycle command는 catalog app ID만 받고 backend에서 현재 registry와 고정 layout을 다시 검증한다.
- 경로는 별도 `install_path` 표시 command에서만 반환한다. build/runtime catalog revision과 locator
  provenance, canonical root/source manifest, manifest 전체의 catalog app ID와 portable exact executable을
  검증하고 source manifest가 현재 Manager 설치 목록의 manifest와 같은지 확인한 뒤 UTF-8로 안전하게
  표시 가능한 경로만 DTO에 넣는다. 조회는 파일 열기·복사·실행·쓰기를 하지 않는다.
- 일괄 요청은 빈 목록·중복 app ID·unknown catalog target·잘못된 mode를 mutation 전에 거부한다.
  backend SemVer 비교에서 available이 installed보다 클 때만 변경하므로 stale UI도 downgrade를 만들지
  않는다. 성공 앱은 유지하고 실패 앱만 재시도하며 lower-level URL·로컬 path 오류는 결과 DTO에 넣지 않는다.
- portable batch는 새 version을 검증한 뒤 `current.json`과 registry를 갱신하고 registry 기록 실패 시
  이전 current를 복구한다. setup batch의 성공은 설치 완료가 아니라 검증된 installer 실행을 뜻하며,
  UI가 여러 마법사 실행을 먼저 확인한다.
- runtime catalog는 build-time revision보다 낮으면 교체하고, 더 최신이면 보존한다. locator가 유효한 뒤의 manifest/path 오류는 legacy root로 우회하지 않는다.
- `reqwest` + redirect 호스트 정책 (`crates/...` 아니고 앱 내 `core/url_policy`)

현재 컨텍스트 메뉴의 실행·폴더 열기·제거는 검증된 휴대용 설치에만 제공한다. 설치 패키지의
source manifest 기록은 표시하지만 실제 설치 위치·uninstaller는 추측하지 않는다. custom root 변경과
app binary/user data를 분리한 확장 제거, Related Tools는 각각 별도 계획 항목이다.

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`
