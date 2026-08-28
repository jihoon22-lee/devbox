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
  않으며 기존 설치와 사용자 data는 건드리지 않는다.
- **앱 행 컨텍스트 메뉴** — 우클릭/Shift+F10/Menu key로 설치·업데이트, 실행, 이전 버전 롤백, 설치 폴더 열기, 설치 경로 정보, 확인 후 제거. 메뉴를 연 행을 먼저 선택하고 닫히면 해당 행으로 focus 복구
- **안전 다운로드** — 허용 호스트 정책, SHA-256·크기 검증, `.partial` 스트리밍
- **중단 다운로드 보호** — target과 `.partial` sibling을 regular-file slot으로 확인하고
  기존 `.partial`은 `create_new`로 덮어쓰지 않는다. 중단 파일은 다음 Manager 시작 때만
  active root 아래 catalog-derived exact download slot에서만 bounded preflight 후 정리한다.
  다른 이름·위치의 사용자 `.partial`은 보존하며, 같은 실행 중 재시도는 fail-closed한다.
- **Manager 소유 portable 경계** — catalog 대상·검증된 버전·active 설치 layout·canonical registry executable이 모두 일치할 때만 실행/폴더 열기/제거. 제거 전 symlink·Windows reparse point와 bounded tree를 검사하며 별도 앱 사용자 데이터는 기본 보존
- **안전한 앱 제거 (#309)** — 제거 전 exact app-owned tree와 manifest revision/digest를
  preview하고 별도 확인을 받는다. portable의 `current.json`, 보존 version과 정확한
  executable만 non-recursive로 제거하며, custom root에서도 같은 경계를 적용한다. 권한·잠금
  실패는 partial 상태와 재시도 안내를 남기고 앱 사용자 data는 항상 보존한다. installer의
  실제 위치·uninstaller, arbitrary path와 강제 삭제는 지원하지 않는다.
- **런타임 discovery 발행** — revision 기반 runtime catalog와 versioned install-root locator를 원자 갱신
- **환경 진단(dev environment doctor)** — WSL/git/node/pnpm/rustc/cargo/devbox-data/catalog-ids/runtime-metadata 점검
- **Data Inspector (#354)** — Manager가 catalog에서 파생한 devbox SQLite만 read-only/query-only로
  발견·스키마 조회·bounded `SELECT`/`WITH`/`EXPLAIN` preview한다. arbitrary path와 write/attach/
  pragma 및 `pragma_*` table-valued function을 차단하고, 512 MiB DB·16 KiB SQL·64 columns·1,000
  rows·64 KiB cell·1 MiB serialized result·2초 실행 상한과 취소를 적용한다. path component와
  `-wal`/`-shm`/`-journal` sidecar의 link/reparse/non-regular 파일을 거부하며, Unix에서는 검사한
  regular file을 열린 descriptor에 고정한 뒤 SQLite를 열어 경로 교체 TOCTOU를 줄인다. `immutable`
  checkpoint image로 읽으므로 live WAL을 진단에 병합하지 않는다.
- **Data Inspector privacy/export** — SQLite column-origin metadata로 직접 column의 실제 source를
  확인한다. secret/token/password/auth/cookie/API key뿐 아니라 username/user-id/login/email source,
  민감한 alias와 변환 expression을 `[REDACTED]`로 마스킹하며 expression label은 `column_N`으로
  안정화한다. 자유 텍스트의 credential·Bearer/JWT·email/path·binary도 redaction하고, CSV string과
  header의 `=`, `+`, `-`, `@` formula prefix는 apostrophe로 escape한다. 결과는 opaque one-time
  preview token으로만 보관하고, 명시적 JSON/CSV export 때 DB revision을 다시 확인한다.
- **Redacted support bundle (#355)** — 앱/catalog/schema/log metadata와 bounded 진단 요약만 offline
  preview로 묶는다. raw DB·raw log·query·credentials/auth/cookie·환경변수·임의 업로드와 원본 경로는
  포함하지 않으며, log metadata도 앱당 128 files·512 entries·4 MiB까지만 센다. 생성 결과는 512 KiB
  이하이고 5분 TTL의 one-time preview token에 exact bytes로 보관한다. export는 catalog revision,
  DB 상태/파일 identity revision, log metadata source revision을 재검증하므로 source가 바뀌면
  stale 처리하고, preview에서 검토한 bytes를 그대로 내보낸다. redaction contract와 omitted
  sections를 확인한 뒤에만 JSON export를 수행한다. export claim은 성공·stale·실패 모두
  소비되므로 UI도 재시도 버튼을 남기지 않고 새 preview를 요구한다.
- **Related Tools** — PowerToys, Windows Terminal, VS Code, Bruno, DBeaver/DB Browser, GitHub Desktop,
  Podman/Docker Desktop의 공식 사이트·라이선스와 Windows 설치 감지를 표시한다. WinGet 설치는
  사용자가 확인한 한 건만 exact ID로 실행하며, 설치된 실행 파일만 직접 실행한다. 감지와 이미
  설치된 도구 실행은 네트워크 없이도 가능하지만, WinGet 설치에는 Windows App Installer와
  네트워크가 필요하고 공식·라이선스 링크는 브라우저/네트워크 상태를 따른다. WinGet이 없어도
  Manager의 native 앱 설치·업데이트·실행에는 영향을 주지 않는다.
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
  `.partial` sibling도 regular-file slot인지 확인한다. 제거도 locator가 가리키는 active root에서
  catalog와 manifest가 증명한 app-owned tree만 대상으로 하며, 기존 user data를 삭제하지 않는다.
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
- Related Tools 목록은 `apps/catalog.json`의 devbox 앱 목록과 분리된 Manager 내부 curated
  metadata다. 각 항목은 공식 URL·license URL·고정 WinGet ID·실행 파일 이름만 포함하고
  사용자 경로·버전·패키지 검색 결과는 저장하지 않는다. 감지는 PATH를 최대 128개 항목·항목당
  4 KiB로 제한한 직접 파일 probe와 제한된 표준 설치 위치의 regular-file/reparse 검사를
  조합해 확인하고 `path`/`known-location`/`not-found`/`unavailable`만 반환한다. OS가 소유한
  `%LOCALAPPDATA%\Microsoft\WindowsApps`의 `wt.exe`·`winget.exe` alias만 고정 이름으로
  예외 허용하며, 그 밖의 reparse executable은 거부한다. `where.exe`나
  임의의 PATH 재탐색 결과를 UI에 전달하지 않으며, probe는 read-only다.
  WinGet은 `install --id <curated-id> --exact --source winget`과 agreement 플래그만 사용하며
  UI의 명시적 확인과 Windows 전용 direct process spawn, 120초 timeout을 통과해야 한다. PATH에서
  확인한 WinGet 경로는 Windows Known Folder/System Directory API로 확인한 OS 소유
  WindowsApps/System32 또는 그 하위의 bounded PATH 후보로 제한하고 즉시
  regular-file/reparse 검사를 다시 통과해야 하며, stdout·stderr,
  resolved path와 installer 위치는 UI·로그 DTO에 넣지 않는다. WinGet 설치·실행·감지는 하나의
  native single-flight 경계를 공유하고, 설치 process tree는 Windows Job Object의
  `KILL_ON_JOB_CLOSE`로 소유한다. process는 suspended 상태로 생성해 Job Object에 할당한 뒤에만
  resume하며, 성공은 Job Object accounting의 active process가 0이 된 뒤에만 반환한다.
  timeout·실패·앱 종료 시 root와 helper를 bounded reap/종료한다.
  frontend API도 고정 catalog metadata와 detection/installed 정합성, action tool ID/status를
  검증하고 native message·오류는 고정된 안전 문구로 치환한다. 늦은 install/launch 응답은
  mount/action generation과 일치할 때만 화면 상태를 갱신한다. Related Tools 화면에는 감지·기존
  실행은 오프라인 가능하고 WinGet 설치·공식 링크는 Windows/네트워크 전제라는 안내를 표시하며,
  오프라인 또는 WinGet 부재 오류는 선택 기능의 상태로만 남고 Manager native 기능을 막지 않는다.

설치 root 경계의 public 오류는 고정된 안전 메시지만 반환하고 입력 경로, locator/manifest 원문,
OS 오류, credential을 반사하지 않는다. locator/manifest bytes와 row 수, path 길이에는 상한이 있으며
손상된 locator는 legacy root로 조용히 우회하지 않는다(locator 자체가 없는 v0.4.x 상태만 read-only
fallback). 브라우저 개발 모드의 API mock은 화면 흐름을 위한 모의 응답일 뿐 native filesystem
적용 성공을 증명하지 않는다.

Data Inspector와 support bundle도 같은 경계를 따른다. 브라우저 mock은 bounded/sanitized 화면
흐름만 제공하고 native command가 실제 catalog-derived path, SQLite authorizer/query-only, SQLite
allocation limit, timeout/cancel/stale 검사를 수행한다. native preview가 성공해도 사용자가 명시적으로
export하기 전에는 파일을 만들지 않으며, export는 만료·revision 재검증과 고정 redaction contract를
통과해야 한다. query와 support preview는 claim 시점에 Mutex에서 원자적으로 제거되므로 동시 export
호출 중 하나만 성공한다.

환경 진단 command도 외부 프로세스를 무제한 실행하지 않는다. WSL/git/node/pnpm/rustc/cargo/docker
version probe는 stdin/stderr를 닫고 stdout 64 KiB·첫 줄 256자·프로세스당 2초로 제한한다. Unix
process group과 Windows Job Object를 함께 정리해 timeout 뒤 helper process가 남지 않게 하며,
출력은 UI/support bundle 경계에 들어오기 전에 username/email/path/credential redaction을 거친다.

현재 컨텍스트 메뉴의 실행·폴더 열기·제거는 검증된 휴대용 설치에만 제공한다. 설치 패키지의
source manifest 기록은 표시하지만 실제 설치 위치·uninstaller는 추측하지 않는다. custom root는
명시적 preview/확인 뒤 빈 root에만 적용하고, 앱 제거는 별도 preview/확인 및 manifest CAS를
통과한 Manager-owned binary tree에 한정한다. root migration/reset, user-data 삭제와 Related
Tools는 각각 별도 계획 항목이다.

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`
