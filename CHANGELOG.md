# Changelog

이 프로젝트의 모든 주요 변경사항은 이 파일에 기록한다.
형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르며, 버전은 `vX.Y.Z` 태그와 함께 릴리스된다.

## [Unreleased]

## [v0.6.0] - 2026-08-31

v0.6.0은 15개 앱의 WSL-native 개발 흐름, dependency/protocol/task 도구, 앱 간 handoff와
공통 UX 품질을 한 번에 배포할 예정인 통합 기능 릴리스다. 별도 신규 앱을 늘리지 않고 Dependency
Lens는 Repo Manager·Workbench, Dev Setup과 로컬 품질은 Devbox Manager, Protocol/MCP Lab은
API Playground, Task Runner는 Run Manager·Workbench 안에서 검증한다.

### Added

- **WSL-native 프로젝트 흐름 (#482)** — Life Log Git 집계, Repo Manager와 Workbench repository/
  worktree 흐름이 `\\wsl$`·`\\wsl.localhost` 경로를 distro/POSIX identity로 정규화하고 distro
  내부 Git을 bounded하게 실행한다. WSL Desktop은 login/non-login PATH를 함께 확인해 사용자
  설치 multiplexer를 찾고 profile/runtime snapshot을 발행한다.
- **Dev Setup과 로컬 품질 (#483, #491)** — Devbox Manager에 환경 capability 진단, package-only
  WinGet Configuration v3 preview/apply, local-only 설치·snapshot 품질 dashboard를 추가했다.
  mutation은 native picker, strict schema, 재검증, 명시적 확인과 rollback 경계 뒤에서만 실행한다.
- **Dependency Lens (#484)** — Repo Manager가 lockfile을 오프라인 분석하고 사용자가 승인한
  경우에만 OSV/deps.dev metadata를 bounded 조회한다. Workbench는 privacy-safe summary만 읽어
  프로젝트 health에 연결하며 경로·package 전체 목록을 snapshot으로 복사하지 않는다.
- **Protocol/MCP Lab (#485)** — API Playground에 MCP Streamable HTTP와 native stdio,
  OAuth authorization-code/PKCE, dynamic gRPC reflection/local proto 및 unary·streaming RPC,
  TLS roots와 DPAPI-backed mTLS credential 흐름을 추가했다.
- **Task Runner (#486)** — Run Manager가 package scripts·Justfile·Taskfile의 trusted workspace
  task를 argv 기반으로 import·실행하고, Workbench가 preview/confirm handoff와 receipt를 통해
  시작·중지·재시도를 조정한다. arbitrary shell 문자열과 secret snapshot은 만들지 않는다.
- **Typed launcher와 activity workflow (#487, #488)** — Launcher가 Workbench profile, Repo
  repository/worktree, WSL profile source를 typed snapshot으로 검색한다. API/Log/Launcher 선택
  텍스트를 Toolbox에서 preview 변환하고 Knowledge draft로 저장하며, Knowledge와 Run의 일별
  activity를 Life Log에서 provenance와 함께 집계한다.
- **관찰 가능성 연계 (#489)** — Port Manager에 Run/Workbench binding correlation, owner 이동,
  Log Lens handoff와 bounded session timeline을 추가했다. Webhook Lab은 deterministic rule
  conflict preview, bounded OpenAPI draft, disabled Run service export와 sanitized Log Lens handoff를
  제공하고, Log Lens는 saved view·reconnect·source-aware persistence를 제공한다.
- **WSL 파일 UX (#490)** — Everything+는 WSL root polling/reconcile을, Code Pad와 Knowledge는
  WSL-native read/write/watch와 disconnect/reconnect 상태를 지원한다. Mermaid는 필요할 때만
  로드하고 앱별 초기 bundle budget을 CI에서 검사한다.
- **15-app UX 계약 (#491)** — 모든 frontend에 `ko-KR`, semantic token, keyboard/focus/IME,
  forced-colors/reduced-motion, 실제 shell axe smoke와 Vite manifest/bundle gate를 적용했다.

### Changed

- **앱별 version** — Port Manager·Developer Toolbox `0.4.0`, WSL Desktop·API Playground·
  Everything+·Knowledge·Life Log·Devbox Manager·Code Pad·Run Manager `0.5.0`, Workbench·
  Webhook Lab·Repo Manager `0.3.0`, Devbox Launcher·Log Lens `0.2.0`이다. 각 앱의 Cargo,
  Tauri, frontend package version과 packaged-smoke config가 일치한다.
- **비공개 package checkpoint** — stable tag 전에 exact current `origin/main`에서만 15 portable와
  15 NSIS installer, notices, manifest를 만드는 수동 candidate workflow를 추가했다. tag/release가
  이미 있으면 fail-closed하며, 32-file/31-declared digest 검증과 disposable Windows runner의
  v0.5.1→v0.6.0 update/rollback lifecycle을 공개 release와 분리해 수행한다.
- **GitHub Actions runtime** — checkout/setup-node/cache/upload/download를 Node 24-capable 공식
  major로 갱신했다. pnpm 9는 repository `packageManager` pin을 Corepack으로 활성화해 Node 20
  runtime의 `pnpm/action-setup` 의존을 제거하고 Rust action도 audited composite/Node 24/Docker
  ref만 허용한다.
- **Cargo graph** — yanked `chacha20 0.10.1`을 compatible non-yanked `0.10.2`로 갱신하고
  lockfile 기반 third-party notices를 재생성했다.

### Fixed

- **Life Log migrated WSL paths** — `/home/jihoon/projects`를 가리키는 `\\wsl$` 또는
  `\\wsl.localhost` project path가 Today/Day/Week/Month 전체 조회를 실패시키던 문제와,
  늦게 도착한 Settings 응답이 방금 저장한 project 목록을 덮는 경합을 수정했다.
- **WSL Desktop zellij discovery** — distro 사용자의 `~/.local/bin/zellij`가 설치돼도 Windows
  process PATH만 보고 “없음”으로 표시하던 오탐을 수정했다.
- **Devbox Manager Docker Desktop discovery** — WSL Desktop에서는 `docker-desktop`과 version을
  확인할 수 있는데 Manager가 Windows app 등록만 보고 미설치로 표시하던 오탐을 수정했다.
- **Dependency Lens fixture** — filesystem mtime 경계 때문에 stale lockfile 회귀가 간헐적으로
  실패하던 fixture를 결정적인 시각 경계로 고정했다.
- **Windows packaged runtime gate (#493)** — 숨김 시작 Launcher의 내부 WebView2 창을 대표 창으로
  오인하던 검증을 exact titled native window 열거로 교체했다. WSL이 없는 호스트에서 WSL Desktop의
  주기적 snapshot writer가 예상 가능한 수집 실패를 GUI stderr에 출력하던 동작도 제거했다.

### Security

- WSL/Git/process/network 작업은 namespace, canonical identity, timeout, byte/count limit와
  process-tree ownership을 native 경계에서 다시 확인한다. path·command·credential·response/
  log body는 공용 snapshot 또는 품질 dashboard에 넣지 않는다.
- OAuth state/PKCE, TLS/mTLS, imported YAML, dependency metadata와 one-time handoff는 각각
  strict schema, fixed endpoint/host, no-redirect 또는 explicit confirmation, DPAPI/zeroizing,
  stale identity 재검증과 fail-closed 오류를 적용한다.
- Dependabot `glib 0.18.5` GHSA는 2026-08-31 다시 평가했다. 현재 Tauri 2.11.5의 Linux-only
  GTK3 graph에서 compatible patched line이 없어 2026-11-30 만료 예외를 유지하되 alert를
  dismiss하지 않고, Windows package에 link되지 않는 범위만 인정한다.

### Verification

- W01~W10 구현 PR은 frontend, Linux Rust, Windows Rust, catalog, dependency와 scope gate를
  통과한 뒤 main에 병합했다. W11은 전체 source gate와 exact version/notices/action-runtime
  계약을 다시 실행한다.
- stable tag는 비공개 15-app candidate의 asset/digest 및 installer lifecycle, 실제 Windows/WSL
  packaged 회귀를 #492/#493에 기록한 뒤에만 만든다. 공개 workflow는 exact annotated tag에서
  새 32개 asset을 build하고, 15 apps/31 manifest-declared/32 public/mismatch 0을 검증한 draft만
  Latest stable로 전환한다.

## [v0.5.1] - 2026-08-29

v0.5.1은 v0.5.0 공개 뒤 확인된 통합 누락과 배포 도구 결함을 보완하는 stable maintenance
bundle이다. 15개 앱 구성은 유지하며, 실제 바이너리가 달라진 Log Lens, Devbox Manager,
Run Manager, Devbox Launcher만 앱별 patch version을 올린다.

### Fixed

- **Log Lens Run source (#473)** — Run Manager handoff를 preview·추가한 뒤 항상 unavailable이 되던
  누락을 보완했다. identity-only 계약을 유지한 채 고정 app-data root의 stdout/stderr 회전
  segment만 logical cursor로 읽고, link/reparse·범위·크기 위반은 fail-closed한다.
- **Run Manager → Launcher task discovery (#479)** — 기존
  `integration/run-manager/v1/summary.json`을 바꾸지 않고 같은 version directory에
  `jobs-services.json` named sidecar를 추가해 저장된 전체 job/service를 검색할 수 있게 했다.
  Launcher는 sidecar가 없을 때만 legacy active-service summary로 fallback하며 손상·권한·
  symlink/reparse 오류에는 fail-closed한다.
- **Devbox Manager browser catalog (#478)** — 브라우저 개발 모드의 고정 fallback을 공개된
  v0.5.0 manifest와 정확히 맞추고, Manager 자신을 제외한 관리 대상 14개와 Launcher·Log Lens의
  version·asset name·size·SHA-256을 동기화했다. 실제 Tauri 앱은 계속 Latest stable release의
  manifest를 backend에서 검증해 사용한다.
- **Windows acceptance inventory (#470)** — PowerShell StrictMode에서 제3자 uninstall key가
  선택적 `DisplayName`을 갖지 않아 전체 inventory가 중단되던 harness 결함을 제거했다.
- **Frontend CI timing** — Developer Toolbox의 비동기 handoff 성공 테스트가 native promise
  호출과 React dialog close 사이에서 경합하지 않도록 최종 UI 상태까지 기다린다.

### Changed

- **변경 앱 version** — Log Lens와 Devbox Launcher는 `0.1.1`, Devbox Manager와 Run Manager는
  `0.4.1`이다. 각 앱의 Cargo package, Tauri config, frontend package version을 같은 값으로
  맞췄다. 나머지 11개 앱 version은 release tag와 독립적으로 유지한다.
- **Prerelease authorization (#477)** — exact annotated `vX.Y.Z` tag push와 명시적 stable
  manual dispatch는 유지하되, prerelease/RC tag push는 Windows build 전에 거부한다.
  prerelease는 수동 dispatch에서 exact tag와 `allow_prerelease=true`를 함께 지정한 경우에만
  허용한다. RC는 사용자가 명시적으로 요청하지 않는 한 생성하지 않는다.

### Security

- `jobs-services.json`에는 opaque definition ID, bounded label/detail과 fixed AppLink action만
  기록하며 command, cwd, environment, path, credential, log content를 포함하지 않는다.
- named snapshot도 기존 producer/version identity, size/depth/count bound, atomic write,
  symlink/reparse rejection을 그대로 적용한다. 잘못된 새 sidecar 때문에 더 오래된 데이터로
  조용히 낮춰 읽지 않는다.

### Verification

- v0.5.0 이후 수정 PR #470, #473, #477, #478, #479는 각각 required CI를 통과한 뒤 main에
  병합됐다. #479는 Linux/Windows Rust와 frontend를 포함한 CI run `33227743754`를 통과했고,
  관련 Rust 388 tests 및 workspace 전체 Clippy `-D warnings`도 태그 준비 전에 통과했다.
- stable release workflow는 exact annotated `v0.5.1` tag와 source commit을 먼저 검증하고,
  catalog의 15개 portable·15개 NSIS installer, notices, manifest인 정확한 32개 asset을 새로
  빌드한다. 다운로드한 31개 manifest-declared asset의 size·SHA-256과 추가/누락 여부를 독립
  검증한 draft만 Latest stable로 공개한다.
- issue #176의 자동·코드·기존 packaged 근거 63개와 별개로, 실제 사용자 Windows/WSL·NTFS·
  multi-monitor/IME·완전 offline 환경이 필요한 7개 수동 관찰은 완료했다고 주장하지 않는다.

## [v0.5.0] - 2026-08-29

v0.5.0은 기존 13개 앱의 native/offline 기능을 강화하고 Devbox Launcher와 Log Lens를 더해
15개 앱을 제공한다. 공용 기반, 앱 기능, 보안 경계를 함께 정리하고 single-instance와
Workbench preflight의 최종 안전성 보강을 포함한다.

### Added

- **native-first 공용 기반** — catalog schema v2·runtime fallback·install-root discovery,
  one-time `applink` handoff의 claim/ack/restore, bounded integration snapshot discovery,
  monitor/DPI-safe window state와 keyboard/IME-aware context menu를 공용 crate/package로
  제공한다. 핵심 흐름은 별도 runtime download나 외부 도구 실행에 의존하지 않는다.
- **15개 앱의 P1·P2 기능** — WSL workspace/profile/clipboard·terminal 안전성, API
  multipart/cookie/OpenAPI/GraphQL/SSE/WebSocket, Everything+의 bounded offline
  text/PDF/DOCX/XLS/XLSX/ODS index, Knowledge capture/image/wikilink, Life Log export·요약,
  Manager batch/custom-root/data-preserving remove, Code Pad Quick Open·offline LSP,
  Run log search, Workbench environment/preflight, Webhook fixture/handoff/replay/sequence,
  Repo Manager의 safe Git 일상 흐름을 추가했다.
- **선택 P3 보강** — Port refresh/diff/favorite/provenance, Toolbox smart workflow와 API
  handoff, WSL resource broadcast, API collection/history/binary, Everything saved query,
  Knowledge template, Life Log source explanation, Manager Data Inspector/support bundle/
  Related Tools, Code Pad multi-file rename, Run import/history, Workbench resilience tools,
  Webhook replay sequence와 Repo cleanup을 포함한다.
- **Devbox Launcher 0.1.0** — 앱과 검증된 profile/repository/job/saved-query snapshot을
  로컬에서 검색하고, 실행 직전 capability를 재검증해 AppLink 또는 명시적 handoff로 연다.
- **Log Lens 0.1.0** — local/WSL/journal/container 로그를 read-only로 tail·merge·filter·
  export하며, bounded ring과 Run Manager·WSL Desktop producer handoff를 제공한다.

### Changed

- v0.5.0 목표에 맞춰 Port Manager와 Developer Toolbox는 0.3.0, 기존 0.3 계열 앱 중
  API Playground·Code Pad·WSL Desktop·Everything+·Knowledge·Life Log·Devbox Manager·
  Run Manager는 0.4.0, Workbench·Webhook Lab·Repo Manager는 0.2.0으로 정렬한다.
  Launcher와 Log Lens는 0.1.0을 유지한다. 각 앱의 `package.json`, Cargo package와
  `tauri.conf.json` version은 서로 일치한다.
- **Native/offline boundary** — No external tool replaces the native/offline core. 외부
  도구는 사용자가 명시적으로 선택하는 보완재로만 제한한다. 오프라인 환경에서 반복 작업은
  devbox 내부에서 완결되고 WSL·Git·container처럼 대상 자체가 필요한 기능만 해당 대상의
  설치를 전제로 한다.

### Security

- v0.4.2에서 검증한 API secret의 backend-only resolve와 History·Collection fail-closed
  persistence/redaction 경계를 그대로 유지한다.
- handoff·snapshot·custom install root·document extractor·Git/process mutation은 opaque ID,
  canonical path, size/time/count bounds, stale identity 재검증과 fixed error를 적용한다.
- dependency allowlist, `cargo-deny`, pnpm audit와 lockfile 기반 `THIRD_PARTY_NOTICES.md`를
  15개 앱 installer와 release asset에 포함한다.

### Fixed

- **Single-instance coverage** — Port Manager, Developer Toolbox, Webhook Lab에
  `tauri-plugin-single-instance` 의존성과 초기화를 추가해 15개 release 앱 모두 두 번째 실행
  시 기존 `main` 창을 보이고 복원·focus한다. catalog gate가 의존성 선언과 초기화를 함께
  검사한다.
- **Workbench preflight path namespace** — Windows directory probe는 drive/UNC, WSL probe는
  POSIX 경로만 허용한다. 잘못된 namespace는 filesystem 또는 `wsl.exe` 실행 전에
  `Unsafe`로 거부한다.
- **Missing descendant link boundary** — 최종 target metadata보다 먼저 existing component의
  symlink 또는 Windows reparse point를 검사한다. link 아래의 아직 없는 descendant를 일반
  `Missing`으로 낮추지 않으며, 기존 operation budget·cancellation·process-tree ownership은
  그대로 유지한다.

### Verification

- 모든 구현 PR은 required CI를 통과한 뒤 main에 병합됐다.
- 현재 릴리스는 release workflow가 빌드하고 게시한다.

## [v0.4.2] - 2026-08-24

v0.4.2는 API Playground 0.3.2의 secret persistence 보안 핫픽스다. backend-only
secret resolve, History·Collection v2 fail-closed migration, cURL·응답·오류·redirect
redaction과 cross-origin credential·body stripping을 포함한다. 아래 section은 RC2 packaged
H1과 cleanup, docs-only stable preparation, exact stable asset 재검증까지의 경계를 기록한다.
RC2를 조상으로 하는 annotated `v0.4.2` tag에서 공식 stable release를 게시했다.

### Fixed

- **backend-only secret resolve** — 환경 변수 reference를 Rust 전송 경계에서만 해석하고,
  frontend state·저장 wire 형식·응답에 해석된 secret을 포함하지 않는다. browser fallback의
  `plain:` pseudo-sealing 경로는 제거했다.
- **History·Collection persistence** — 평문 포함 여부를 증명할 수 없는 v1 History는
  fail-closed로 격리·삭제하고 v2 write/read-back과 marker 순서를 보장한다. Collection v1의
  direct credential은 redaction하며 `requiresSecretReview` boolean metadata를 보존한다.
- **cURL·response·error·redirect redaction** — 민감 header/body/query/auth와 redirect
  destination을 마스킹하고, cross-origin hop에 credential·body를 전달하지 않는다.

### Verification

- immutable `v0.4.2-rc2` tag의 source commit은
  `8bcde4271778f83c23b7b1049634a65656662e89`다. [RC2 release workflow
  32700441413](https://github.com/jihoon22-lee/devbox/actions/runs/32700441413)이
  성공했고, [공개 prerelease](https://github.com/jihoon22-lee/devbox/releases/tag/v0.4.2-rc2)는
  13개 앱의 portable 13개와 NSIS installer 13개, `release-manifest.json` 1개인 정확한
  27 assets(26 binaries/1 manifest)를 가진다. missing 0, undeclared 0이며 독립적인
  size·SHA-256 대조도 통과했다. API Playground portable SHA-256은
  `bfec1475c87173515c6c6a21fb6f10a145090c070ed51d40d03e9167d874c053`다.
- API Playground의 frontend package, Rust Cargo package, Tauri package version은 모두
  `0.3.2`다.
- [H1 PASS evidence](https://github.com/jihoon22-lee/devbox/issues/176#issuecomment-5392680030)는
  Windows `10.0.26200`, PowerShell `5.1.26100.9168`, WebView2 `151.0.4129.101`,
  Node `24.18.1`에서 H1-A~D 전체 통과를 기록한다. logical localStorage 전체 scan에서
  plaintext가 없었고 cleanup 뒤 API Playground process 0, app-data 부재, backup residue
  0을 확인했다.
- host firewall이 unbound IPv4/IPv6 loopback을 timeout으로 매핑하므로 직접적인
  `ConnectionRefused`라고 부를 수 없었다. H1-D의 generic non-timeout transport failure는
  accept-and-reset server로, timeout은 1초 제한보다 늦게 응답하는 loopback server로 각각 확인했다.
  정확한 multi-format clipboard 보존을 보장할 수 없어 clipboard는 건드리지 않았다.
- H1 harness SHA-256은
  `a5c628c91967375fa4508ea65048fb3dfa06bd7b17a722e60dc379c1f2d44794`, redacted result
  SHA-256은 `d3beac59f928f8f1b8b97614245cdecb88a912468de3721c12379e617a111080`이며,
  evidence에 secret 원문이나 sealed blob은 없다.

### Release status

`v0.4.2-rc1`은 immutable failed-H1 historical candidate로 보존하고, RC2는 H1을 통과한
stable basis로 보존한다. stable preparation PR
[#233](https://github.com/jihoon22-lee/devbox/pull/233)은 required CI 뒤 commit
`c9a320ef52ac2d6abe30d9f6e5364a09780b54c4`에 병합됐고, 같은 commit의 annotated
`v0.4.2` tag에서 [workflow 32708402180](https://github.com/jihoon22-lee/devbox/actions/runs/32708402180)의
Build, Publish, Verify 세 job이 성공했다. [stable release](https://github.com/jihoon22-lee/devbox/releases/tag/v0.4.2)는
`draft=false`, `prerelease=false`, GitHub Latest이며 정확한 27 assets를 가진다. 별도 download의
26 binaries와 manifest는 모든 size·SHA-256, missing 0, undeclared 0을 통과했다.

Stable API Playground portable SHA-256은
`c7927b833633d5abf038eca6adda726d9d5ea2a5929b4b1649e777de207d6a10`이다. RC2와 stable
binary digest가 달라 byte reproducibility를 가정하지 않고 exact stable portable에서 H1-A~D를
다시 통과했다. cleanup 뒤 API process 0, app-data 부재, backup residue 0을 독립 확인했으며
상세 결과는 [issue #176](https://github.com/jihoon22-lee/devbox/issues/176#issuecomment-5393695037)에
기록했다. H1 이후 product code를 바꾸면 새 RC부터 다시 검증한다. GitHub Actions의 Node 20
action-runtime deprecation annotation은 향후 유지보수 항목이며 v0.4.2 blocker가 아니다.

## [v0.4.2-rc2] - 2026-08-24

v0.4.2-rc2는 API Playground 0.3.2의 secret persistence 보안 핫픽스를 새 immutable
release candidate에서 다시 검증하기 위한 prerelease다. `v0.4.2-rc1`의 Windows
패키지·asset 검증은 통과했지만, packaged H1에서 Collection v1→v2 변환의
`requiresSecretReview` boolean metadata가 backend sanitizer에 의해 문자열
`[REDACTED]`로 바뀌어 parse에 실패하는 결함을 발견했다. 따라서 RC1은 안정판으로
승격하지 않았고, RC2의 새 package와 전체 H1 재검증이 끝나기 전에는 v0.4.2 안정판을
게시하지 않는다.

### Fixed

- **Collection·History persistence schema metadata** — `requiresSecretReview`가 정확히
  boolean인 경우에만 보존하고, 해당 필드의 non-boolean 값은 즉시 redaction한다. 실제
  secret field의 redaction 경계를 완화하지 않으면서 persisted History·Collection wire
  shape가 frontend schema parser를 통과하도록 수정했다.
- **Regression coverage** — persisted History·Collection wire shape와 실제 sensitive field를
  함께 검증하는 Rust 회귀 테스트를 추가했다. API Playground Rust 테스트는 14개에서
  16개로 늘었고, 전체 CI가 통과했다.

### Security

- **backend-only secret resolve** — `{{NAME}}`와 `${NAME}` environment reference를 URL,
  query, header key/value, body 및 auth field에서 Rust가 전송 직전에만 해석한다. 해석된
  요청은 frontend state나 응답 wire 형식에 포함하지 않으며 unseal 실패는 ciphertext
  fallback 없이 안전하게 종료한다.
- **History·Collection v2** — 평문 포함 여부를 증명할 수 없는 `apip-history` v1은 UI에서
  즉시 격리하고, 빈 v2 write/read-back과 raw key delete/read-back이 성공한 뒤에만
  migration marker를 기록한다. Collection v1의 직접 입력 credential은 reference 또는
  `[REDACTED]`와 `requiresSecretReview`로 변환하며 raw backup·quarantine을 만들지 않는다.
- **cURL·응답·오류 redaction** — 기본 cURL은 Authorization, Cookie, API key와 auth 값을
  마스킹한다. 원문 cURL은 명시적 확인 뒤 일회성 clipboard 복사에만 사용한다. 응답 header/body,
  URL userinfo/query, redirect location/final URL, 알려진 token 패턴과 network error에도 같은
  redaction 경계를 적용한다.
- **redirect credential stripping** — redirect를 최대 10회까지 직접 처리하고 cross-origin
  hop에는 Authorization, Cookie, API key 계열 header, 알려진 secret이 든 일반 header,
  auth, body 및 stale body metadata를 전달하지 않는다. 307/308도 같은 경계를 적용하며,
  목적지 URL 자체에 민감정보가 있으면 다른 origin에 연결하기 전에 차단한다.
- **browser fallback 제거** — WebView가 아닌 browser preview에서는 secret seal/send/reveal을
  거부하며 기존 `plain:` base64 pseudo-sealing 경로를 제거했다.

### RC1 and RC2 verification boundary

- `v0.4.2-rc1` annotated tag `371c404`의 공식 Windows release는 workflow
  [32693958102](https://github.com/jihoon22-lee/devbox/actions/runs/32693958102)에서
  Build, Publish, Verify 세 job이 통과했고, 13개 portable·13개 NSIS installer·manifest를
  포함한 정확한 27 assets와 26 binaries를 별도 다운로드로 독립 검증했다. API Playground
  portable asset도 size·SHA-256 대조를 통과했다.
- RC1 packaged H1에서는 DPAPI sealing, backend-only resolve, History v1 fail-closed 삭제,
  cURL·응답·오류 redaction, cross-origin redirect credential/body 차단 및 민감한 목적지
  연결 전 차단을 확인했다. Collection v1→v2 변환은 위 schema sanitizer 결함으로 실패했고,
  raw v1이 logical storage에 남아 stable gate를 차단했다. 이 결과와 cleanup evidence는
  [issue #176 comment](https://github.com/jihoon22-lee/devbox/issues/176#issuecomment-5391635404)에
  기록했다.
- PR [#231](https://github.com/jihoon22-lee/devbox/pull/231)이 `be2c64e`로 main에
  병합되어 위 결함과 wire-shape 회귀를 수정했다. RC2는 새 immutable annotated tag로
  빌드하고 정확한 27 assets/26 binaries를 다시 검증한 뒤, H1-A~D 전체를 재수행해야 한다.
  H1 재검증과 cleanup이 모두 통과하기 전에는 v0.4.2 안정판으로 간주하지 않는다.

## [v0.4.2-rc1] - 2026-08-24

v0.4.2-rc1은 API Playground 0.3.2의 secret persistence 보안 핫픽스를 실제 Windows
패키지에서 검증하기 위한 prerelease다. 안정판은 계속 v0.4.1이며, 아래 H1 packaged
acceptance가 통과하기 전에는 v0.4.2로 승격하지 않는다.

### Security

- **backend-only secret resolve** — `{{NAME}}`와 `${NAME}` environment reference를 URL,
  query, header key/value, body 및 auth field에서 Rust가 전송 직전에만 해석한다. 해석된
  요청은 frontend state나 응답 wire 형식에 포함하지 않으며 unseal 실패는 ciphertext
  fallback 없이 안전하게 종료한다.
- **History·Collection v2** — 평문 포함 여부를 증명할 수 없는 `apip-history` v1은 UI에서
  즉시 격리하고, 빈 v2 write/read-back과 raw key delete/read-back이 성공한 뒤에만 migration
  marker를 기록한다. Collection v1의 직접 입력 credential은 reference 또는 `[REDACTED]`와
  `requiresSecretReview`로 변환하며 raw backup·quarantine을 만들지 않는다.
- **cURL·응답·오류 redaction** — 기본 cURL은 Authorization, Cookie, API key와 auth 값을
  마스킹한다. 원문 cURL은 명시적 확인 뒤 일회성 clipboard 복사에만 사용한다. 응답 header/body,
  URL userinfo/query, redirect location/final URL, 알려진 token 패턴과 network error에도 같은
  redaction 경계를 적용한다.
- **redirect credential stripping** — redirect를 최대 10회까지 직접 처리하고 cross-origin
  hop에는 Authorization, Cookie, API key 계열 header, 알려진 secret이 든 일반 header,
  auth, body 및 stale body metadata를 전달하지 않는다. 307/308도 같은 경계를 적용하며,
  목적지 URL 자체에 민감정보가 있으면 다른 origin에 연결하기 전에 차단한다.
- **browser fallback 제거** — WebView가 아닌 browser preview에서는 secret seal/send/reveal을
  거부하며 기존 `plain:` base64 pseudo-sealing 경로를 제거했다.

### Changed

- API Playground의 frontend·Rust·Tauri package version을 0.3.2로 올렸다.
- Rust 1.98의 신규 clippy lint에 맞춰 WSL·Code Pad의 고정 2바이트 청크 처리를
  `as_chunks`로 바꾸되 기존 UTF-16·SHA 파싱 동작을 회귀 테스트로 보존했다.

### Verification status

- API Playground frontend 55개 및 Rust 14개 보안 회귀 테스트, 전체 Cargo
  test/check, 13개 앱 순차 frontend build와 GitHub Actions Linux/Windows job이 통과했다.
- v0.4.1 W0에서 API Playground를 포함한 격리 가능한 portable 10개 앱의 cold start가
  통과했다.
- RC1에서는 API Playground 0.3.2 packaged 실행의 DPAPI ciphertext, logical localStorage
  raw History 삭제·Collection 변환, cURL/응답/오류 redaction과 cross-origin credential
  stripping, 307/308 body 억제와 민감한 redirect destination 연결 전 차단을 H1으로 직접
  확인한다.

## [v0.4.1] - 2026-08-20

v0.4.1은 v0.4.0 이후 발견된 터미널·앱 간 열기·Run Manager lifecycle·identifier 이관
결함을 누적해 수정한 안정판 핫픽스다.

### Fixed

- **wsl-desktop 터미널** — PTY 읽기 경계에서 잘린 UTF-8을 carry 버퍼로 재조립하고,
  ConPTY·Unicode11·scrollback·resize 하드닝과 세션 정리를 적용했다. Open Terminal은 셸
  문자열 조립 대신 `wsl.exe -d <distro> --cd <cwd> --`의 분리 argv로 경로 경계를 보존한다.
- **앱 간 열기** — `crates/applink`, `devbox://open`, single-instance pending-open 수신을
  Code Pad·WSL Desktop·Workbench에 적용했다. Repo Manager는 Code Pad에 `Workspace`, WSL
  Desktop·Workbench에 `Path`를 전달하고, Workbench도 WSL Desktop에 실제 프로젝트 경로를 전달한다.
- **Run Manager lifecycle** — Tauri `setup` 경계에서 runtime 없이 실행되던 scheduler와
  maintenance task를 Tauri async runtime에서 시작하도록 바꿔 시작 시 panic을 방지했다.
- **identifier 이관** — 10개 앱에서
  `crates/filesystem::migrate_legacy_identifier_dir`를 `tauri::Builder::default()`보다 먼저
  실행한다. 현재 identifier 디렉터리가 있으면 merge하거나 덮어쓰지 않고 건너뛰며, 실패는
  로그를 남기고 다음 실행에서 재시도한다.

### Data safety and recovery guidance

> **중요:** v0.4.0 또는 그 이전 RC를 실행한 뒤에는 같은 앱에 `com.devbox.*`와
> `com.workbench.*` 디렉터리가 모두 남아 있을 수 있다. v0.4.1은 더 최신일 수 있는 현재
> 상태를 덮어쓰지 않기 위해 이 경우 자동 migration을 의도적으로 건너뛴다. 두 디렉터리를
> 모두 백업하고, 어느 상태를 사용할지 확인한 뒤에만 필요한 데이터를 수동으로 복구하거나
> 이동하라. 자동 merge나 자동 recovery는 하지 않는다.

### Verification boundary

- 자동화된 migration 사례와 10개 앱의 `tauri::Builder::default()` 이전 호출 위치는 검증했다.
- Windows C1/C2는 사용 가능한 Windows 장비에서 legacy path가 이미 제거되어 안전하게 재현할 수
  없었다. 이는 packaged-runtime 검증이 아니며, 남은 수동 acceptance는 [issue #176](https://github.com/jihoon22-lee/devbox/issues/176)에서 계속 관리한다.

## [v0.4.1-rc4] - 2026-08-20

v0.4.1-rc4는 identifier 변경 뒤 앱 로컬 데이터를 안전하게 이관하는 코드를 확인하기 위한
Windows acceptance 후보 빌드다. 선택된 Windows acceptance가 끝났다는 뜻이 아니며, 안정판
v0.4.1 완료를 의미하지 않는다.

### Fixed

- **Developer Toolbox 0.2.2 — RC3에서 직접 관찰한 packaged build 결함** — Windows 패키지에서
  WebView/Tauri가 setup보다 먼저 현재 경로인
  `com.devbox.developertoolbox/EBWebView`를 만들었다. 기존 setup 시점 migration은 현재
  디렉터리(`com.devbox.developertoolbox`)가 이미 존재하는지 검사하는 guard였으므로
  destination-exists로 판단해 migration을 건너뛰었고, 기존
  `com.workbench.developertoolbox` 데이터가 이관되지 않았다.
- **10개 identifier 이관 앱** — 공용
  `crates/filesystem::migrate_legacy_identifier_dir`를 `tauri::Builder::default()`보다 먼저
  실행해 구 identifier 디렉터리 전체를 현재 identifier로 rename한다. 현재 디렉터리가 이미
  있으면 merge하거나 덮어쓰지 않고 그대로 건너뛰며, rename 또는 로컬 데이터 경로 확인에
  실패해도 앱을 중단하지 않고 재실행 때 다시 시도하도록 로그를 남긴다.

### Affected packaged versions

| 앱 | 버전 |
|---|---:|
| api-playground | 0.3.1 |
| code-pad | 0.3.3 |
| devbox-manager | 0.3.1 |
| developer-toolbox | 0.2.2 |
| everything-plus | 0.3.1 |
| knowledge-base | 0.3.1 |
| life-log | 0.3.1 |
| port-manager | 0.2.2 |
| run-manager | 0.3.3 |
| wsl-desktop | 0.3.3 |

### Data safety and recovery guidance

> **중요:** v0.4.0 또는 그 이전 RC를 실행한 뒤에는 같은 앱에
> `com.devbox.*`와 `com.workbench.*` 디렉터리가 모두 남아 있을 수 있다. RC4는 이 경우
> 더 최신일 수 있는 현재 상태를 덮어쓰지 않기 위해 자동 migration을 의도적으로 건너뛴다.
> 두 디렉터리 모두 보존되며 자동 merge나 자동 recovery는 하지 않는다. RC4를 실행하기 전에
> 두 디렉터리를 모두 백업하고, 어느 상태를 사용할지 확인한 뒤에만 필요한 데이터를 수동으로
> 복구하거나 이동하라.

### Release status

- RC4는 코드와 자동화 게이트를 확인하기 위한 Windows acceptance 후보이며, 선택된 Windows
  실기 검증이 끝날 때까지 안정판 v0.4.1로 간주하지 않는다.

## [v0.4.1-rc3] - 2026-08-20

v0.4.1-rc3는 RC2 Windows acceptance 중 발견된 Run Manager 시작 결함을 수정한 추가 acceptance
후보 빌드다. 이 후보는 안정판 v0.4.1의 Windows acceptance가 완료됐다는 뜻이 아니다.

### Fixed

- **run-manager 0.3.2** — RC2 Windows acceptance에서
  `apps/run-manager/src-tauri/src/lifecycle.rs:144`의 scheduler `tokio::spawn`이 Tauri `setup`
  경계에서 현재 Tokio runtime 없이 실행되어 panic하고 프로세스가 즉시 종료되는 현상을 직접
  관찰했다. 후속 코드 검토와 회귀 테스트에서 maintenance task에도 같은 setup-runtime 결함이
  있음을 확인했으며, 두 lifecycle task를 Tauri가 구성한 async runtime에서 시작하도록 변경했다.

### Tests

- 동기 `setup` 경계에서 scheduler와 maintenance task가 panic 없이 시작·종료되는 회귀 테스트를
  추가했다.

### Release status

- RC3는 수정 사항을 확인하기 위한 Windows acceptance 후보이며, 선택된 실기 검증이 끝날 때까지
  안정판 v0.4.1로 간주하지 않는다.

## [v0.4.1-rc2] - 2026-08-20

v0.4.1-rc2는 코드 변경과 자동화 게이트를 반영한 Windows acceptance 후보 빌드다. 이 후보는
Windows 실기 실행이 완료됐다는 뜻이 아니며, 선택된 Windows acceptance 검증을 남겨 둔다.

### Fixed

- **wsl-desktop 0.3.2** — Windows cwd를 변환하거나 셸 문자열로 조립하지 않고 `wsl.exe --cd`의
  별도 argv 값으로 그대로 전달해 공백이 있는 경로도 경계를 보존한다.
- **wsl-desktop** — 자연 종료된 터미널 세션의 리소스 정리를 추가하고, 오래된 reader가 교체된
  세션을 지우지 않도록 teardown 및 cleanup 경합을 안전하게 처리했다.

### Tests

- resize 거부 후 재시도와 활성화 시 대기 중 resize 취소 회귀 테스트를 추가했다.
- 동시 세션 생성에서 ID가 충돌하지 않는지 검증하는 회귀 테스트를 추가했다.

### Release gate

- 릴리스 노트 섹션이 없거나 비어 있으면 실패하도록 추출 게이트를 fatal로 변경했다.
- RC2는 Windows acceptance 후보이며, 안정판 v0.4.1 배포를 위한 Windows runtime 완료를 주장하지 않는다.

## [v0.4.1-rc1] - 2026-08-19

v0.4.1 핫픽스의 릴리스 후보(v0.4.1-rc1)다. 터미널 PTY 전송과 앱 간 링크의 결함을 수정했으며,
Windows 수동 검증 매트릭스를 위한 후보 빌드다.

### Fixed

- **wsl-desktop** — PTY 읽기 경계에서 잘리던 UTF-8을 carry 버퍼로 재조립하고, ConPTY 빌드 정보·Unicode11·
  스크롤백·리사이즈 하드닝을 적용해 한글·박스 드로잉과 창 크기 변경 시 터미널 화면 손상을 줄였다. v0.3.0에서
  `bash -lc "cd ..."`가 파일명으로 처리되던 Open Terminal 실패도 `wsl.exe -d <distro> --cd <cwd> --` 형태의
  정확한 분리 argv로 수정했다.
- **앱 간 링크** — `devbox://open` 수신과 single-instance pending-open 전달을 보강해 콜드/웜 시작 모두 대상 경로를
  소비하도록 했다. repo-manager는 Code Pad에 `Workspace`, WSL Desktop·Workbench에 구체적인 `Path`를 보내며,
  Workbench도 WSL Desktop에 프로필 id 대신 실제 프로젝트 경로를 전달한다.

### Release status

- 이 RC의 검증 범위는 Windows 실기·패키지·프로토콜·경로·시각 수동 검증이며, 결과는 안정판 배포 판단의 근거로
  사용한다.

## [v0.4.0] - 2026-08-18

기능 추가 릴리스. 신규 앱 3종(Workbench, Webhook Lab, Repo Manager)과 devbox-manager 환경 진단이 추가되어
총 13개 앱이 되었다. 배포 워크플로가 카탈로그 기반으로 정비되고, 신규 앱에도 CSP 기준선이 적용되었다.

### 업그레이드 참고 (v0.3.0 이하 사용자)

PR #180에서 릴리스 asset 명명이 `<ProductName>_<태그버전>_x64-setup.exe`에서
`<app-id>_<앱버전>_x64-setup.exe`로 바뀌었고, portable 설치 layout도
`apps/<id>/<tag>/<id>.exe`에서 `apps/<id>/versions/<version>/<id>.exe`로 바뀌었다.
**v0.3.0 이하 Devbox Manager는 이 새 이름·layout을 인식하지 못해 "Install (setup)"
버튼이 동작하지 않는다** (휴대용 재설치는 계속 동작한다). Releases 페이지에서 새
Devbox Manager를 직접 내려받아 먼저 설치한 뒤, 나머지 앱을 새 Manager로 설치하세요.

### Added

- **workbench (신규)** — 프로젝트 기반 orchestration 셸. ProjectProfile CRUD(기존 wsl-desktop·life-log 저장소 흡수),
  Git/WSL/포트/서비스 사전 점검, Run Manager 서비스·WSL Desktop layout·Code Pad workspace 시작,
  idempotent 실행 기록과 `Stop What I Started`(Workbench가 시작한 자원만 정리).
- **webhook-lab (신규)** — 로컬 웹훅/콜백 서버(inbound HTTP). method/path별 request history, 응답 rule·지연·오류 재현,
  `Authorization`·`Cookie`·API key 헤더 masking, body/history 상한,
  기본 bind 127.0.0.1 + LAN 공개는 명시적 설정.
- **repo-manager (신규)** — Git repository 탐색·브랜치/dirty/ahead-behind/worktree 상태 목록, worktree 생성,
  Code Pad·WSL Desktop·Workbench로 열기. force delete·reset·clean 기본 동작 없음, worktree remove 전
  uncommitted/untracked 검사.
- **devbox-manager** — 환경 진단(dev environment doctor) 탭: WSL/git/node/pnpm/rustc/cargo/devbox-data/catalog-ids 점검.
- **crates** — `crates/wsl`, `crates/search`, `crates/integration`, `crates/secrets`가 공용 크레이트로 추출되었다.
- **packages** — `packages/tokens`, `packages/editor`, `packages/diff-view`가 공용 React 패키지로 추출되었다.
- **배포 정비** — 릴리스 워크플로가 `apps/catalog.json`에서 빌드 대상을 읽고, portable·installer를 staging해
  `release-manifest.json`(asset 명칭·SHA-256)과 함께 게시하며 verify 단계가 asset을 대조한다.
- **CSP 기준선** — 신규 앱 3종에도 `default-src 'self'; ...; connect-src 'self' ipc: http://ipc.localhost` 기준선 적용.

### Changed

- 앱 카탈로그가 13개로 확장 (`apps/catalog.json`).
- GitHub Releases 산출물 명칭 통일: 휴대용 `<app-id>.exe`, 설치 `<app-id>_<version>_x64-setup.exe`.
- 앱 버전을 기능 추가·수정에 맞게 개별 갱신 (버전은 각 앱이 독립적으로 가져간다):
  - 0.2.0 → **0.3.0**: api-playground(컬렉션·환경·시크릿), everything-plus(watcher·결과 액션),
    knowledge-base(CodeMirror·watcher·snapshot), devbox-manager(카탈로그·manifest·원자 설치·환경 진단)
  - 0.2.2 → **0.3.0**: life-log(활동 추적 흡수·idle·privacy·자동 시작·프로젝트 귀속), wsl-desktop(wsl-dashboard 흡수·탭)
  - 0.3.0 → **0.3.1**: code-pad(복구·problems·탐색 이력), run-manager(관찰성·export/import)
  - 0.2.0 → **0.2.1**: port-manager, developer-toolbox (identifier 이관 등 내부 정비)
  - 0.1.0 → **0.1.1**: repo-manager (git·앱 실행 안정화)

### Fixed

- **repo-manager·workbench·life-log** — Windows에서 `git` 하위 프로세스가 실패해 브랜치 `?`/`n/a`,
  커밋 수 0으로 표시되던 문제 수정 (`crates/git`).
- **repo-manager·workbench** — 설치된 앱 실행(`open_in`·Start Workspace)이 잘못된 exe명으로 실패하던 문제 수정 (`crates/launch`).
- **devbox-manager** — GitHub release asset redirect 대상 변경(`release-assets.githubusercontent.com`) 미반영 수정.
- **devbox-manager** — 환경 진단 WSL 버전 UTF-16LE 깨짐 수정.
- **life-log** — 실행 파일 중복 실행 방지(단일 인스턴스 + 기존 트레이 포커스).
- **wsl-desktop** — grid 행 높이 불균형·팬 간 이동 불가 수정 (Alt+Arrow).
- **code-pad** — 창 축소 시 하단 잘림 수정 (`.content-area` 높이 제약).
- **workbench** — Windows에서 `wsl.exe -l -v` 출력을 UTF-16LE로 디코딩하지 않아 프로젝트 사전 점검(project health)의
  WSL 배포판 확인이 항상 "distro 없음"으로 표시되던 문제 수정. devbox-manager(#183)와 같은 원인으로, 공용
  `crates/wsl` 디코더를 재사용했다 (#192).
- **repo-manager** — `scan_root`가 탐색 깊이 제한·제외 규칙 없이 전체 파일시스템을 재귀 탐색해 `node_modules`·
  `target`·`AppData` 등까지 들어가고 Windows junction 순환에 취약하던 문제 수정. 비-repo 디렉터리 가지치기와
  탐색 깊이·방문 디렉터리 상한을 추가했다. 상한에 걸리면 `scan_root`가 `truncated` 플래그를 반환하고 화면에
  배너로 알린다 (#193).

### 알려진 문제

- **wsl-desktop 터미널 출력 손상.** PTY 읽기 경계에 걸친 멀티바이트 문자(한글·박스드로잉)가 손상돼 화면이
  간헐적으로 깨지고, `htop`/`vim`/`lazygit` 같은 TUI의 프레임이 어긋난다. 긴 줄이 있는 상태에서 창 크기를
  바꾸면 기존 출력이 망가지는 문제도 함께 있다.
  설계·수정 계획: [`docs/superpowers/specs/2026-08-17-wsl-desktop-terminal-design.md`](docs/superpowers/specs/2026-08-17-wsl-desktop-terminal-design.md) §2
- **앱 간 "다른 앱으로 열기"가 경로를 전달하지 못한다.** repo-manager의 Code Pad/WSL Desktop/Workbench 열기와
  workbench의 Start Workspace는 대상 앱을 실행하지만, 대상 앱이 명령줄 인자를 읽지 않아 빈 상태로 열린다.
  설계·수정 계획: [`docs/superpowers/specs/2026-08-17-app-interop-design.md`](docs/superpowers/specs/2026-08-17-app-interop-design.md) §5.1

## [v0.3.0] - 2026-08-13

기능 추가 릴리스. 신규 앱 2종(Code Pad, Run Manager)이 추가되어 총 12개 앱이 되었다.

### Added

- **code-pad (신규)** — CodeMirror 6 기반 경량 코드 에디터. 문법 하이라이팅, 탭·분할 2뷰, 찾기/바꾸기(정규식),
  인코딩/줄바꿈(CRLF/LF) 감지·변환, 큰 파일 가드, `.md`/`.mmd` 프리뷰. 언어 중립 LSP 클라이언트와 Windows 로컬 stdio
  서버 관리(진단·자동완성·hover·정의·참조·이름 변경·포맷, 재시작 백오프)를 제공하며, rust-analyzer·typescript-language-server·
  basedpyright·vscode-langservers-extracted를 검증된 고정 버전으로 설치한다.
- **run-manager (신규)** — 예약 실행(크론 잡)과 상시 실행(서비스)을 한곳에서 관리. 1초 스케줄러, occurrence 원자적 claim,
  중복 실행 정책(skip/queue/kill-previous), Windows(Job Object)·WSL(session/group) 실행 어댑터, DPAPI 환경변수 보호,
  stdout/stderr 회전 로그 tail, 실패 Windows toast 알림. 서비스는 start/stop/restart·자동 시작·재시작 정책(never/on-failure/always)·
  백오프·프로세스 생존/로컬 TCP 헬스체크를 지원한다.
- **crates** — `crates/filesystem`(제한 순회 API), `crates/markdown`, `crates/process`가 공용 크레이트로 추출되었다.

### Changed

- code-pad·run-manager 버전을 0.3.0으로 설정.

### Fixed

- (v0.2.x에서 수정된 항목 유지)

## [v0.2.2] - 2026-08-11

### Fixed

- **wsl-desktop** — \`+ Terminal\` 클릭 시 \`wsl.exe\`가 \`0xc0000142\`(DLL initialization failed)로 실패하고 터미널이 열리지 않던 문제 수정. portable-pty의 ConPTY(HPCON)를 보관하는 master가 세션 종료 시 함께 닫히면서, 아직 시작 중인 wsl.exe가 잘못된 pseudoconsole 핸들로 초기화를 시도했던 것. master를 세션 핸들에 보관해 ConPTY 수명을 유지하도록 함.
- **life-log** — 주간/월간 조회 시 "No activity in this period"만 표시되던 문제 수정. 기본 활동 데이터 소스 경로가 activity-timeline의 실제 저장 경로(\`%LOCALAPPDATA%\\com.workbench.activitytimeline\\data.db\`)와 달랐던 것. 문서(CONVENTIONS·windows-guide)의 데이터 위치도 실제 identifier 기준 경로로 정정.

## [v0.2.1] - 2026-08-11

### Fixed

- **wsl-desktop** — Windows에서 터미널 창이 열리지만 shell이 로딩되지 않던 문제 수정. portable-pty 0.9의 ConPTY 시작 교착(PSEUDOCONSOLE_INHERIT_CURSOR가 커서 위치 조회 후 응답을 기다려 자식 프로세스를 정지)을 해소하기 위해 세션 시작 직후 `ESC[1;1R`을 입력 파이프로 전송.

## [v0.2.0] - 2026-08-11

기능 추가 릴리스. 신규 앱 2종(WSL Desktop, Devbox Manager)이 추가되어 총 10개 앱이 되었다.

### Added

- **wsl-desktop (신규)** — 임베디드 터미널(xterm.js + PTY). WSL 배포판 선택·지정 경로로 터미널 열기, 격자/세로/가로 분할, 여러 터미널에 동시 명령(broadcast).
- **devbox-manager (신규)** — devbox 앱 버전 체크·설치·업데이트·실행. 휴대용 exe 또는 설치 패키지 방식 선택.
- **life-log** — 캘린더 날짜 선택, 로딩 표시, 주간/월간 조회(일별 사용량 차트), 지난 날짜 세션 캐시.
- **api-playground** — 현재 요청을 curl 명령으로 변환·복사.
- **activity-timeline** — 30초 자동 새로고침.
- **everything-plus** — re-index 진행률 표시, 정규식 검색 모드, 텍스트 내용 검색(확장자 선택·루트별 옵션).
- **wsl-dashboard** — Docker 미설치 안내 배너, 프로젝트 경로 입력 수정·localStorage 저장.

### Changed

- 모든 앱 버전을 0.2.0으로 통일.

### Fixed

- (v0.1.1에서 수정된 항목 유지)

## [v0.1.1] - 2026-08-11

Windows에서 발견된 버그 수정 릴리스. port-manager / wsl-dashboard / knowledge-base / life-log의 버전을 0.1.1로 올렸다.

### Fixed

- **port-manager** — 한국어 Windows에서 `netstat` 출력(OEM 코드페이지, 예: CP949)이 UTF-8 디코딩을 실패하던 문제 수정. 자식 프로세스의 콘솔 창 깜빡임도 제거.
- **wsl-dashboard** — 파이프로 실행된 `wsl.exe -l -v`가 UTF-16LE(NUL 포함)로 출력돼 "null byte found in provided data" 오류가 나던 문제 수정 (UTF-16 디코딩 추가). wsl/git 자식 프로세스의 콘솔 창 깜빡임도 제거.
- **knowledge-base** — Windows 절대경로 처리 버그로 파일 작업·데일리 노트에서 "경로가 루트 밖을 벗어납니다"가 나던 문제 수정.
- **life-log** — git 커밋 집계 자식 프로세스의 콘솔 창 깜빡임 제거.

## [v0.1.0] - 2026-08-11

최초 릴리스: 8개 데스크톱 앱 (Tauri v2, Rust + React).

### Added

- **port-manager** — 포트/프로세스 조회·검색·필터, 프로세스 종료, localhost 열기
- **developer-toolbox** — 14종 개발 도구 (JSON/Base64/URL/타임스탬프/Case/Hash/UUID/Regex/Diff/JWT)
- **wsl-dashboard** — WSL 배포판·Docker·git 상태 대시보드, 컨테이너 start/stop/restart
- **api-playground** — REST 요청 빌더, CORS 없는 응답 확인, 요청 history
- **activity-timeline** — 포그라운드 창 기반 사용 기록, 하루 타임라인·앱 통계, 트레이 상시 실행
- **everything-plus** — 파일명 FTS5 인덱스·검색, 루트 관리, 백그라운드 재인덱스
- **knowledge-base** — 마크다운 저장소, 태그, 본문 검색, 데일리 노트
- **life-log** — 활동·git 집계 일일 요약, 데이터 소스 설정

### Changed

- 없음 (최초 릴리스)

### Fixed

- 없음 (최초 릴리스)

### Known issues

- 개인 빌드라 코드 서명이 없어 설치 시 SmartScreen 경고가 표시된다 (`추가 정보 → 실행`).
- activity-timeline 포그라운드 추적, wsl-dashboard 등 **Windows 전용 기능은 Windows에서만 동작**한다.
- everything-plus 내용(body) 검색은 v2 예정. 현재는 파일명 검색만 지원.
