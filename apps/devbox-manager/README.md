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
- **설치 root preview·적용** — 사용자가 고른 기존 디렉터리를 native backend가 canonical path,
  symlink/reparse point, 보호 경로, 비어 있음, 여유 공간과 현재 설치 상태까지 읽기 전용으로
  확인한 뒤, 별도 확인을 거쳐 다음 설치 root로 적용한다. 기존 설치를 자동 이동하거나 삭제하지
  않으며 custom root의 제거는 #309 후속 범위다.
- **앱 행 컨텍스트 메뉴** — 우클릭/Shift+F10/Menu key로 설치·업데이트, 실행, 이전 버전 롤백, 설치 폴더 열기, 설치 경로 정보, 확인 후 제거. 메뉴를 연 행을 먼저 선택하고 닫히면 해당 행으로 focus 복구
- **안전 다운로드** — 허용 호스트 정책, SHA-256·크기 검증, `.partial` 스트리밍
- **중단 다운로드 보호** — target과 `.partial` sibling을 regular-file slot으로 확인하고
  기존 `.partial`은 `create_new`로 덮어쓰지 않는다. 중단 파일은 다음 Manager 시작 때만
  active root 아래 catalog-derived exact download slot에서만 bounded preflight 후 정리한다.
  다른 이름·위치의 사용자 `.partial`은 보존하며, 같은 실행 중 재시도는 fail-closed한다.
- **Manager 소유 portable 경계** — catalog 대상·검증된 버전·active 설치 layout·canonical registry executable이 모두 일치할 때만 실행/폴더 열기/제거. 제거 전 symlink·Windows reparse point와 bounded tree를 검사하며 별도 앱 사용자 데이터는 기본 보존
- **런타임 discovery 발행** — revision 기반 runtime catalog와 versioned install-root locator를 원자 갱신
- **환경 진단(dev environment doctor)** — WSL/git/node/pnpm/rustc/cargo/devbox-data/catalog-ids/runtime-metadata 점검
- **실행** — 설치된 앱 실행

## 기술

- `apps/catalog.json`(앱 단일 원본) + 릴리스 `release-manifest.json`만 신뢰
- 공용 catalog: `%LOCALAPPDATA%\devbox\catalog.json`
- install-root locator: `%LOCALAPPDATA%\devbox\install-roots\v1\registry.json`
- 설치 manifest는 active Manager root의 `registry.json`이 소유하며, locator에는 canonical root와
  manifest 경로만 기록한다. locator의 `schemaVersion`, 양의 단조 `registryRevision`, catalog
  provenance와 `updatedAtMs`를 함께 검증하고 tmp+rename으로 갱신한다.
- `preview_install_root({ path })`는 파일을 만들거나 고치지 않는 read-only preflight다. 입력은
  bounded absolute path이며 unresolved environment variable, `~`, `.`/`..`, root/home/workspace,
  symlink/reparse component, 일반 파일, canonical path가 아닌 alias를 거부한다. 후보는 이미 존재하는
  빈 디렉터리여야 하고 쓰기 권한과 OS가 반환한 free space가 최소 128 MiB 이상이어야 한다. active manifest의
  설치 기록이나 root 내부 artifact가 하나라도 있으면 `existing-install`로 표시한다.
- `apply_install_root({ path, expectedRegistryRevision })`는 preview에서 얻은 revision을 CAS token으로
  다시 확인하고 모든 path·manifest·free-space 검사를 재실행한다. 새 후보에 `apps/`를 안전하게
  만들고 빈 `registry.json`은 기존 파일을 대체하지 않는 exclusive create+sync로 준비한 뒤 locator를
  원자적으로 publish한다. publish 직전 candidate direct entries, empty `apps/`, exact manifest를
  다시 확인하며, locator commit이 실패하면
  이번 호출이 만든 두 항목만 비우고 기존 root·registry·user data는 건드리지 않는다. manifest가
  이미 `[]` 이외의 내용으로 바뀌었으면 삭제하지 않고 rollback 실패를 보고한다. revision,
  catalog revision, root ID가 바뀌면 적용하지 않는다.
- 적용 이후 설치·실행·rollback·경로 조회는 locator가 가리키는 active root를 사용한다. 설치 디렉터리는
  `create_dir_all`로 symlink/reparse를 따라가지 않고 각 component를 생성·확인하며, 다운로드 target과
  `.partial` sibling도 regular-file slot인지 확인한다. custom root에서는 기존 binary removal을
  제공하지 않고 #309의 별도 안전 제거 PR을 기다린다.
- non-legacy lifecycle은 locator의 catalog provenance가 현재 선택 revision과 같고 registry의 모든
  app ID가 현재 Manager 대상일 때만 동작한다. startup sync 실패 뒤 stale custom state나 제거된
  catalog app을 그대로 사용하지 않는다.
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
- startup metadata sync도 present locator를 먼저 bounded·strict하게 읽는다. locator가
  손상됐거나 그 부모 component가 symlink/reparse이면 default metadata를 새로 써서 복구하지
  않고 fail-closed하며, locator 자체가 없는 v0.4.x 상태만 legacy fallback 대상이다. 유효한
  custom root/manifest는 다시 검증한 뒤 선택 catalog revision만 locator에 단조 전파하고,
  더 앞선 revision을 downgrade하지 않는다.
- root preview/apply가 진행되는 동안 refresh, 환경 진단, tab/app/batch action도 같은
  single-flight guard로 비활성화한다. 반대로 metadata refresh/환경 진단 중에도 root·app
  mutation을 막아 locator 전환과 다른 Manager 상태 갱신이 겹치지 않게 한다.
- `reqwest` + redirect 호스트 정책 (`crates/...` 아니고 앱 내 `core/url_policy`)

설치 root 경계의 public 오류는 고정된 안전 메시지만 반환하고 입력 경로, locator/manifest 원문,
OS 오류, credential을 반사하지 않는다. locator/manifest bytes와 row 수, path 길이에는 상한이 있으며
손상된 locator는 legacy root로 조용히 우회하지 않는다(locator 자체가 없는 v0.4.x 상태만 read-only
fallback). 브라우저 개발 모드의 API mock은 화면 흐름을 위한 모의 응답일 뿐 native filesystem
적용 성공을 증명하지 않는다.

현재 컨텍스트 메뉴의 실행·폴더 열기·제거는 검증된 휴대용 설치에만 제공한다. 설치 패키지의
source manifest 기록은 표시하지만 실제 설치 위치·uninstaller는 추측하지 않는다. custom root는
명시적 preview/확인 뒤 빈 root에만 적용하고, app binary/user data를 분리한 제거·root migration,
Related Tools는 각각 별도 계획 항목이다.

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`
