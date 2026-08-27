# Roadmap

13개 앱을 순차적으로 완성하고, 공통 코드가 실제로 필요해지면 그때 `crates/`·`packages/`로 추출한다.
처음 8개 앱(port-manager~devbox-manager)을 완성한 뒤 에디터(code-pad)·예약 실행·서비스 관리자(run-manager),
그리고 Stage 4·5 앱(workbench·webhook-lab·repo-manager)을 추가했다.

## Phase 1 — Tauri 기본기 ✅
- [x] **port-manager** — IPC, Rust 기초, netstat 파싱, 포트/프로세스 관리
- [x] **developer-toolbox** — 사이드바 UI, 오프라인 소형 개발 도구 모음 (hash/uuid/HMAC/regex/diff는 Rust)

## Phase 2 — 시스템/네트워크 ✅
- [x] **api-playground** — HTTP(reqwest), 요청 빌더, 응답 뷰어, history
- [x] **everything-plus** — FTS5 이름 인덱스/검색, 루트 관리, 백그라운드 재인덱스

## Phase 3 — 개인 데이터 플랫폼 ✅
- [x] **knowledge-base** — frontmatter/태그, 파일 저장소, FTS5 검색, 데일리 노트
- [x] **life-log** — 활동·git 집계 허브 (activity-timeline 흡수)

## 추가 앱 ✅
- [x] **wsl-desktop** — 임베디드 WSL 터미널 (분할 레이아웃, 동시 명령, wsl-dashboard 흡수)
- [x] **devbox-manager** — 앱 버전 체크·설치·업데이트·실행
- [x] **code-pad** — CodeMirror 6 경량 코드 에디터. 언어 중립 LSP 클라이언트와 Windows 로컬 stdio 서버 관리
  (진단·자동완성·hover·정의·참조·이름 변경·포맷, 재시작 백오프)
- [x] **run-manager** — 예약 실행(크론 잡)과 상시 실행(서비스)을 한곳에서 관리. Windows/WSL 실행 어댑터,
  DPAPI 환경변수 보호, 회전 로그 tail, 실패 알림, 서비스 재시작 정책·헬스체크

## Stage 4 — Workbench ✅
- [x] **workbench** — 프로젝트 기반 orchestration 셸. ProjectProfile(기존 두 저장소 흡수), Git/WSL/포트/서비스 사전 점검,
  Run Manager·WSL Desktop·Code Pad 시작, idempotent 실행 기록, `Stop What I Started`

## Stage 5 — 신규 앱 ✅
- [x] **webhook-lab** — 로컬 웹훅/콜백 서버 (inbound HTTP). request history, 응답 rule·delay·오류 재현,
  민감 헤더 masking, LAN 공개 기본 차단, bounded masked JSON fixture 저장 (#314; API Playground handoff는 #315)
- [x] **dev environment doctor** — devbox-manager의 환경 진단 탭 (WSL/git/node/pnpm/rustc/cargo/devbox-data/catalog-ids)
- [x] **repo-manager** — Git repository 탐색·브랜치/worktree/상태 목록, worktree 생성, Code Pad·WSL Desktop·Workbench로 열기
  (파괴적 기본 동작 없음, remove 전 uncommitted/untracked 검사)

## 다음 단계 — v0.5.0 확정 계획

`docs/product-opportunities.md` §17(PR 1~39 + Stage 4/5)은 **전부 완료**됐다. 이후 작업은
2026-08-22에 확정한 네이티브 우선 계획과 하위 설계 문서를 따른다.

| 문서 | 범위 |
|---|---|
| [v0.5.0 네이티브 우선 계획](./superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md) | P1·P2·선택 P3 전체 범위, 외부 도구 원칙, 신규 앱, PR·테스트·릴리스 gate의 단일 기준 |
| [wsl-desktop 터미널 설계](./superpowers/specs/2026-08-17-wsl-desktop-terminal-design.md) | PTY 전송 결함, 클립보드·단축키, 레이아웃 복원, 멀티플렉서 opt-in |
| [앱 간 연동 설계](./superpowers/specs/2026-08-17-app-interop-design.md) | argv 계약, 카탈로그 capability, 스냅샷 버스 정리 |
| [UX 개선 설계](./superpowers/specs/2026-08-15-ux-improvements-design.md) | 컨텍스트 메뉴 13개 앱, toolbox 도구, 앱별 항목, 실사용 피드백 |

### v0.4.1 — 핫픽스 (결함만, 기능 추가 없음; 안정판 배포 완료)

1. **wsl-desktop 터미널 출력·세션 실행 결함** — v0.4.0에서 `terminal.rs`가 PTY 읽기마다
   `String::from_utf8_lossy`를 호출해 읽기 경계의 한글·박스드로잉을 U+FFFD로 치환했고,
   `windowsPty` 미설정·팬 1×2 붕괴·영구 resize desync와 사용자 제보의 `bash -lc "cd ... && exec bash"`
   단일 argv 문제가 관찰됐다. v0.4.1에서 UTF-8 carry, `windowsPty`/resize, `wsl.exe ... --cd <cwd> --`
   분리 argv를 적용해 해결했다.
2. **작동하지 않는 앱 간 링크** — v0.4.0에서는 repo-manager와 workbench가 다른 앱에 인자를 넘겨도
   **argv를 읽는 앱이 없어** 빈 앱이 열렸다. v0.4.1에서 `crates/applink`와 Code Pad/WSL Desktop/
   Workbench의 수신 및 대상 매핑을 구현해 복구했다.

3. **run-manager 시작 시 panic** — RC2 Windows acceptance에서
   `apps/run-manager/src-tauri/src/lifecycle.rs:144`의 scheduler `tokio::spawn` 호출이 Tauri
   `setup` 경계의 runtime 부재로 panic하고 프로세스가 즉시 종료되는 현상을 직접 관찰했다. 후속
   코드 검토와 회귀 테스트에서 maintenance task에도 같은 setup-runtime 결함이 있음을 확인했다.
   v0.4.1-rc3에서 두 lifecycle task를 Tauri가 구성한 async runtime에서 시작하도록 수정하고,
   동기 `setup` 경계의 시작·종료 회귀 테스트를 추가했다.

4. **identifier 변경 뒤 앱 로컬 데이터 이관** — RC3 packaged Developer Toolbox에서 WebView/Tauri가
   setup 전에 `com.devbox.developertoolbox/EBWebView`를 만들어 setup 시점 destination-exists
   guard가 구 데이터 이관을 건너뛰는 결함을 직접 관찰했다. RC4 후보에서는 공용 whole-directory
   rename을 `tauri::Builder::default()` 전에 수행하고, 현재 디렉터리가 있으면 덮어쓰지 않으며,
   실패는 로그를 남기고 다음 실행에서 재시도한다.

v0.4.1은 안정판 핫픽스로 배포됐다. 자동화된 migration 사례와 10개 앱의
`tauri::Builder::default()` 이전 호출 위치는 검증했지만, 사용 가능한 Windows 장비에서 legacy path가
이미 제거되어 Windows C1/C2를 안전하게 재현하지 못했다. 이는 packaged-runtime 검증이 아니며, 남은
Windows acceptance는 [issue #176](https://github.com/jihoon22-lee/devbox/issues/176)에서 post-release로 계속 관리한다.

### v0.4.2 — API Playground 보안 핫픽스 (안정판 배포 완료)

v0.4.1에도 존재하는 resolved secret persistence 결함을 v0.5.0까지 미루지 않고 P1-02 전체
범위로 선행 수정했다. API Playground 0.3.2는 secret을 Rust 전송 경계에서만 해석하고,
History·Collection v2 fail-closed migration, masked cURL, 응답·오류·redirect redaction과
cross-origin credential·body stripping 및 민감한 redirect destination 연결 전 차단을 적용한다.
코드와 Linux/Windows CI는 main에 반영됐으며, 이 선행 완료는 v0.5.0 범위를 삭제하거나
축소하지 않고 같은 P1-02 회귀 기준으로 이어진다.

v0.4.2-rc1은 다음 단계까지 완료했다.

- `v0.4.2-rc1` annotated tag는 commit `371c404`에 고정됐다.
- 공식 Windows release workflow
  [32693958102](https://github.com/jihoon22-lee/devbox/actions/runs/32693958102)의
  Build Windows installers, Publish release, Verify release assets 세 job이 모두 성공했다.
- 13개 portable + 13개 NSIS installer + `release-manifest.json`으로 구성된 정확한 27 assets,
  26 binaries를 별도 다운로드·size·SHA-256 대조로 독립 검증했다. API Playground portable
  asset도 검증을 통과했다.
- Windows packaged H1에서 DPAPI sealing, backend-only resolve, History v1 fail-closed 삭제,
  cURL·응답·오류 redaction, cross-origin redirect credential/body 차단 및 민감한 redirect
  destination 연결 전 차단을 확인했다.

그러나 RC1 H1의 Collection v1→v2 변환에서 `requiresSecretReview` boolean metadata가
backend sanitizer에 의해 문자열 `[REDACTED]`로 바뀌어 schema parse가 실패했고, raw v1이
logical storage에 남았다. 이 실패와 cleanup 결과는 [issue #176 comment](https://github.com/jihoon22-lee/devbox/issues/176#issuecomment-5391635404)에
기록했으며, stable gate는 의도적으로 차단된 상태다. 이 결함을 수정한 PR
[#231](https://github.com/jihoon22-lee/devbox/pull/231)은 commit `be2c64e`로 main에
병합됐고, 정확한 boolean metadata 보존·non-boolean 즉시 redaction, History/Collection wire
shape와 실제 sensitive field 회귀 테스트를 포함한다. API Rust 테스트는 16개로 늘었고
전체 CI도 통과했다.

RC1은 수정·삭제하지 않는 immutable failed-H1 historical candidate로 보존한다. schema fix가
반영된 RC2는 다음 gate를 모두 통과했다.

- annotated `v0.4.2-rc2`는 source commit
  `8bcde4271778f83c23b7b1049634a65656662e89`에 고정됐다. 공식
  [release workflow 32700441413](https://github.com/jihoon22-lee/devbox/actions/runs/32700441413)의
  Build, Publish, Verify 세 job이 성공했고, [공개 prerelease](https://github.com/jihoon22-lee/devbox/releases/tag/v0.4.2-rc2)는
  정확한 27 assets(13 portable + 13 NSIS installer + manifest), 26 binaries를 가진다.
- 별도 download에서 모든 size·SHA-256, missing 0, undeclared 0을 독립 검증했다. API Playground
  portable SHA-256은 `bfec1475c87173515c6c6a21fb6f10a145090c070ed51d40d03e9167d874c053`이고
  package/Cargo/Tauri version은 모두 0.3.2다.
- Windows `10.0.26200`, PowerShell `5.1.26100.9168`, WebView2 `151.0.4129.101`에서
  accepted packaged H1-A~D를 수행했다. UI secret sealing과 backend-only resolve,
  History v1 fail-closed removal, Collection v2 conversion과 boolean review metadata,
  reference 보존, cURL·response·error redaction, 307/308 body·entity-header 억제, 민감한
  redirect destination 연결 전 차단, generic transport-failure/timeout 메시지와 logical
  localStorage 평문 부재가 모두 통과했다.
- 이 host firewall은 unbound IPv4/IPv6 loopback을 `ConnectionRefused` 대신 timeout으로
  처리했다. non-timeout generic transport-failure branch는 accept-and-reset server로,
  timeout branch는 지연 loopback server로 분리해 검증했으며 두 오류 모두 URL·port·token·
  내부 오류를 노출하지 않았다. exact multi-format 복구를 보장할 수 없어 clipboard는
  건드리지 않았다.
- cleanup 뒤 API Playground process 0, 격리 app-data 부재, backup residue 0과 test server·
  새 WebView2 descendant 종료를 독립 재확인했다. secret 원문·sealed blob 없는 상세 결과는
  [issue #176 PASS evidence](https://github.com/jihoon22-lee/devbox/issues/176#issuecomment-5392680030)에
  기록했다.

stable preparation PR [#233](https://github.com/jihoon22-lee/devbox/pull/233)은 required CI 뒤
commit `c9a320ef52ac2d6abe30d9f6e5364a09780b54c4`에 병합됐다. 같은 commit의 annotated
`v0.4.2` tag에서 [stable workflow 32708402180](https://github.com/jihoon22-lee/devbox/actions/runs/32708402180)의
Build, Publish, Verify 세 job이 성공했고, [stable release](https://github.com/jihoon22-lee/devbox/releases/tag/v0.4.2)는
`draft=false`, `prerelease=false`, GitHub Latest다. 13 portable + 13 NSIS installer + manifest의
정확한 27 assets/26 binaries는 별도 download에서 모든 size·SHA-256, missing 0, undeclared 0을
다시 통과했다.

Stable API Playground portable SHA-256은
`c7927b833633d5abf038eca6adda726d9d5ea2a5929b4b1649e777de207d6a10`이다. stable binary가
RC2와 byte-identical하다고 가정하지 않고 exact stable asset에서 accepted H1-A~D를 다시 수행해
logical localStorage plaintext 0과 cleanup(API process 0, app-data absent, backup residue 0)을
확인했다. 상세 evidence는 [issue #176 stable PASS comment](https://github.com/jihoon22-lee/devbox/issues/176#issuecomment-5393695037)와
[release plan](./superpowers/plans/2026-08-24-v0.4.2-release.md)에 기록한다. v0.4.2 선행 완료는
v0.5.0 P1-02의 회귀 기준으로 유지하며 P1·P2·선택 P3, Devbox Launcher·Log Lens 범위를
삭제하거나 축소하지 않는다.

### v0.5.0

v0.5.0은 외부 도구 설치 허브가 아니라 **오프라인 native 기능과 앱 간 직접 전달**을 강화하는
release다. 현재 13개 앱을 강화하고 `devbox-launcher`, `log-lens`를 추가해 목표 15개 앱으로
확장한다. 아래 P3도 검토 후 선택된 release 범위이며 임의로 탈락시키지 않는다.

#### P1 — 선행 필수

1. 네이티브 우선·외부 도구 보완 원칙과 bundled dependency/license 지침.
2. API Playground의 resolved secret History·Collection·cURL·응답/오류 경계 수정(v0.4.2
   선행 hotfix), backend-only resolve와 localStorage 기반 구버전 History fail-closed migration.
3. catalog schema v2, runtime catalog, installed target discovery, 하드코딩 allowlist 제거.
4. Knowledge·Everything+·Repo Manager inbound/single-instance 수신 확대.
5. Life Log/Knowledge/WSL snapshot producer·consumer 정리와 자동 발견, direct DB read 제거.
6. `packages/context-menu`와 기존 13개 앱의 도메인별 메뉴.
7. WSL Desktop clipboard·shortcut·title/cwd/link/search/font·resize 안전성.
8. WSL native workspace/profile/layout와 action palette, optional tmux/zellij adapter.
9. Toolbox JSON↔YAML/Base64 계열/JSON→TypeScript, API header/cookie/multipart, Knowledge
   wikilink/backlink/rename preview, Manager batch, Code Pad Quick Open/LSP UX, Workbench
   services/ports·WSL 제안, Webhook label/example curl. 기존 실사용 backlog의 WSL Docker compact,
   Code Pad preview·LSP panel·Quick Open tree, Manager install path 표시, API response header/cookie도 포함.

#### P2 — 순서가 유동적인 필수 후속

1. `OpenTarget::Handoff` protocol v2와 atomic one-time handoff: P2 Webhook→API,
   Life Log→Knowledge. P3 Launcher/Log Lens bootstrap 이후 Toolbox→API와 Run/WSL→Log Lens를
   연결한다.
   - 2026-08-27 core implementation: 기존 v1 argv/path 호환성을 유지하면서 opaque
     `kind`/128-bit `id`만 argv로 전달하고, 10분 TTL·10MiB 상한·target/kind/schema 검증,
     create-new publication, token 기반 claim/ack/restore, 60초 bounded lease와 crash recovery를
     공용 `crates/applink`에 구현했다. raw credential과 상대/symlink payload path는 publication과
     claim 양쪽에서 거부한다. producer/consumer 화면과 clipboard fallback은 후속 issue 범위다.
2. Port Manager command line·WSL identity-safe kill.
   - 2026-08-26 draft: native/WSL/container listener rows, bounded full command/path,
     Windows creation FILETIME (decimal-string wire value) and WSL proc start tick identity,
     endpoint+identity
     revalidation, fixed-argv signal/terminate boundary, and WSL Desktop container
     stop handoff descriptor are implemented in the dedicated feature worktree.
   - The handoff descriptor is deliberately the seam to applink protocol v2; Port
     Manager does not duplicate the one-time store or call the container engine's
     process API. Windows W2 packaged smoke and cross-app consumer verification remain
     release-gate work after the P2-01 contract lands.
   - auto-refresh, diff, favorite/provenance, established-connection termination, and
     arbitrary PID kill remain outside this issue.
3. Toolbox UUID v7/ULID/HTML/URL/HMAC/JWT verify/Lorem/Markdown table/내장 QR.
4. API Playground OpenAPI 3.x import, GraphQL, SSE, WebSocket.
   - GraphQL #294 draft: 기존 native HTTP/auth/environment/history 경계 위에 bounded GET/POST
     query·variables·operationName, canonical GraphQL JSON body, HTTP-vs-GraphQL response
     projection, cancellation과 browser parity를 구현했다. URL/userinfo/credential query,
     document/variables/header/body/response bounds 및 fixed error/redaction을 native와
     frontend에서 mirror한다. multi-operation `operationName` 선택과 request-ID-routed
     in-flight connect/body cancellation까지 루트 review fixture로 고정했고 frontend 전체
     test/build를 통과했다. persisted query/introspection/schema cache/subscription/codegen/replay는
     제외하며 Windows W2는 PR gate에서 확인한다.
   - OpenAPI 첫 독립 범위는 로컬 파일과 HTTP(S) URL의 JSON/YAML 3.0/3.1 bounded import 및
   operation preview/apply다. 로컬 경로는 완전 오프라인이고 URL 선택 때만 제한된 native fetch를
   수행한다. Swagger UI bundle·자동 전송·secret 주입은 제외한다. `$ref`·unsupported auth/method·unsafe graph는
   operation 단위로 격리하며, 기존 Collection overwrite 없이 현재 draft 또는 새 항목으로만
   명시적으로 적용한다.
   - WebSocket #296 implementation: native `tokio-tungstenite` ws/wss transport와 existing
     header/cookie/auth/environment/redaction 경계를 재사용해 explicit Connect/Send/Ping/Close/
     Disconnect, text/binary preview/save, close state와 bounded 10,000-message/20 MiB retention을
     구현했다. TLS certificate verification을 유지하고 transport 파생 header·credential query·
     unsafe path/raw error를 차단하며 opaque session event만 webview에 보낸다. Browser preview는
     standard WebSocket 제한(custom header/auth/cookie/direct ping)을 표시하고 request timeout도
     적용한다. terminal listener/network cleanup 뒤에도 bounded binary save handle은 다음 연결까지
     유지하며, native DTO preview bounds와 atomic file save를 fail-closed로 고정했다. Native loopback/
     masking/eviction/accessibility/lifecycle fixtures와 app-only test/build/typecheck를 포함한다.
     이미 구현된 SSE #295와 OpenAPI #293의 동작은 변경하지 않고 함께 동작하도록 통합했으며,
     GraphQL subscription은 제외한다. Windows W2 packaged smoke는 release gate에서 확인한다.
5. Everything+ text/code/Markdown 및 PDF/DOCX/XLS(X)/ODS content index.
6. Knowledge global quick capture·image asset, Life Log Markdown/JSON/CSV export·규칙 기반 요약.
   Life Log export는 #305 범위의 native-first date-range artifact와 browser-preview 경계를
   포함하며, exact `[start,end)`/DST boundaries·bounded Git·snapshot provenance·privacy/
   deterministic rendering을 선행 계약으로 삼는다.
7. Manager custom install root (#308)·데이터 보존형 안전 제거 (#309).
8. Code Pad LSP cache/local archive, Run Manager log search, Workbench project environment,
   Webhook API handoff (#315; captured fixture storage #314 완료), Repo Manager history/diff/stage/commit/fetch/FF-only pull/push.

   - Code Pad LSP offline path draft (#310): reviewed catalog의 exact archive를 app-owned
     SHA-256 cache에서 재사용하고, native archive 또는 Node reviewed dependency closure `.tgz`
     archive set의 명시적 local import와 source/last-verification 상태를 제공한다. 선택 set은
     검증된 cache와 결합할 수 있으며, 실패 시 editor·save·preview·Quick Open은 계속 사용할 수 있다.
   - Run Manager log search draft (#311): 기존 retained stdout/stderr만 대상으로 literal
     우선·명시적 regex·level/source/time filter, bounded non-blocking scan, line/stream
     navigation과 `log-source/v1` opaque source validation을 구현 중이다. Log Lens 연결,
     원격 로그와 permanent archive는 이 작업에서 제외하고 별도 P3 integration으로 남긴다.

   - 2026-08-27 `#297` BASE 구현: Everything+의 내용 검색은 content-enabled root에서
     명시적 source/Markdown/plain-text 후보만 선택해 UTF-8 및 UTF-16 LE/BE를 strict
     decode하고, 파일 20 MiB·보관 text 2,000,000 Unicode scalar characters·후보 처리
     10초 상한을 적용한다. UTF-8 BOM, UTF-16 BOM, empty/Korean/English fixture와
     unsupported binary/encoding, oversized file, file-change race를 모두 deterministic
     상태로 기록하며, Office/OCR/semantic search는 다음 별도 feature로 남긴다.
   - 각 `file_content` row는 `content_status`, `extractor_version=text-v1`, `truncated`,
     `indexed_at`, `error_code`, `encoding`, `text_chars`를 보유한다. 실패 row의 FTS body는
     비워 filename search를 계속 제공하고, sensitive filename은 읽기 전에
     `skipped_sensitive`로 격리하며 content snippet은 common credential/private-key와
     provider token·AWS access key·JWT pattern redaction, 4,096자 output cap 뒤에만 UI로
     전달한다. full/incremental index는 250-file batch와 cooperative cancel을 사용하고
     watcher도 같은 extractor와 크기·mtime race check를 재사용한다. 기존 regex 파일명
     prefilter는 최대 2,000개를 유지하고 content result만 최대 200개로 제한한다.
   - schema v2 migration은 roots를 보존하고 files/content 파생 index만 재생성한다. `Cancel`
     중 이미 커밋된 부분 결과는 안전한 상태로 남고 `Re-index`로 수렴한다. Rust unit/
     integration fixture와 frontend stale-search·input bound·cancel/a11y fixture를
     focused gate로 검증했으며, packaged Windows W2와 전체 release gate는 아직 남아 있다.
   - 2026-08-27 `#298` PDF extractor 구현: MIT `lopdf`로 text object만 bounded offline
     추출하고 PDF 전용 `pdf-v1` metadata를 기록한다. 20 MiB file/16 MiB decompressed
     page/object stream/100,000 parsed objects/10,000 pages/2,000,000 character/10초 상한을
     적용하며, object/page 구조 상한 초과는 `extract_error`와 `resource_limit`으로,
     image-only scan·encrypted·corrupt 입력은 각각 `no_text`·`unsupported_encrypted`·
     `extract_error`로 격리한다. `meta.pdf_extractor_version` marker가 첫 설치와 parser
     version 전환을 보장하고 성공한 full/PDF scan 뒤 갱신된다. PDF extractor version 변경
     시 PDF row만 format-specific reindex해 text/source/Markdown row를 보존하되, PDF-only
     reindex 중 큐 요청은 `All`로 승격해 새 root/index 요청을 놓치지 않는다. Office/OCR/
     image/format extraction은 후속 별도 issue로 남긴다.
   - 2026-08-27 `#299` XLS extractor 구현: MIT `calamine::Xls`로 legacy `.xls`의
     worksheet 셀 값만 bounded offline 추출하고 `xls-v1` metadata를 기록한다. 파일 20 MiB,
     text 2,000,000 Unicode scalar characters, candidate 10초 상한과 256 sheet/4,000,000
     logical-cell 방어선을 적용한다. fail-closed BIFF preflight는 record/formula/metadata,
     shared-string 원본과 반복 clone 확장량, 256 MiB 추정 parser memory도 parser 진입 전에
     제한한다. formula evaluation·VBA/macro·image/style/외부 resource는 사용하지 않는다.
     encrypted/corrupt/논리 resource-limit workbook은 각각
     `unsupported_encrypted`/`extract_error`/`resource_limit`로 격리한다.
     `meta.xls_extractor_version` marker가 첫 설치와 parser version 전환을 보장하고 성공한
     full/XLS scan 뒤 갱신된다. XLS row만 format-specific reindex해 text/PDF/다른 문서 인덱스를
     보존하며, queued request는 `All`로 승격한다.
   - 2026-08-27 `#300` XLSX extractor 통합: MIT pure-Rust `calamine::Xlsx`의 streaming cell
     API로 cached worksheet 값만 오프라인 추출하고 `xlsx-v1`을 기록한다. parser보다 먼저
     EOCD/ZIP64의 선언 entry 수와 실제 central directory를 각각 4,096개로 제한하고, entry
     32 MiB·전체 uncompressed 64 MiB, canonical package/workbook relationship, unsafe/중복
     path, encryption/external target/DTD, XML depth/event, shared string 및 4,000,000 logical/
     visited cell 상한을 fail-closed로 검증한다. dense worksheet range, formula evaluation,
     macro/image/style/network는 사용하지 않는다.
   - 2026-08-27 `#301` ODS extractor 통합: MIT pure-Rust `calamine::Ods`로 cached cell 값만
     추출하고 `ods-v1`을 기록한다. 같은 ZIP envelope/entry/uncompressed/XML/sheet/row/column/
     cell 경계를 적용하며 manifest encryption과 DTD를 차단한다. dense range parser 진입 전에
     row/column repeat, non-empty value/formula clone 16,000,000자, 기존/신규 Data·formula
     vector가 겹치는 256 MiB peak memory를 계산해 작은 XML의 반복 증폭을 거부한다.
   - `#299`·`#300`·`#301`은 하나의 spreadsheet content-index PR로 검증하되 각각 독립
     extractor version·acceptance를 유지한다. compact `FormatSet`이 stale format 조합을 처리하고
     성공한 full/format-only scan만 marker를 기록한다. partial/cancel scan은 재시도를 보장하며
     XLS/XLSX/ODS-only rebuild는 서로 및 text/PDF row를 보존한다. DOCX는 독립 parser 경계로
     이 묶음에 포함하지 않았다.
   - 2026-08-27 `#302` DOCX extractor 구현: `.docx`만 case-insensitive candidate로 추가하고
     MIT `zip` + `quick-xml` streaming scan으로 `word/document.xml`의 text와 paragraph/tab/
     line-break를 오프라인 추출해 `docx-v1`을 기록한다. raw ZIP envelope와 실제 archive에서
     entry 4,096개, entry 32 MiB, 전체 uncompressed 64 MiB, unsafe/중복 path와 encryption을
     제한한다. canonical content type/package relationship/main part, XML depth 128/event
     1,000,000개/text Unicode scalar+raw attribute byte 합산 8,000,000 budget/relationship
     4,096개를 fail-closed로 검사한다.
     CFB `EncryptedPackage`와 encrypted entry는 `unsupported_encrypted`, 빈 본문은 `no_text`,
     손상/limit/DTD/macro-enabled package는 raw detail 없는 고정 실패 metadata로 격리한다.
     성공한 full/DOCX-only 전체-root scan만 독립 marker를 갱신하고 DOCX-only reindex는
     text/PDF/XLS/XLSX/ODS hit를 보존한다. legacy DOC/DOCM, non-main part, OCR/semantic search,
     image/style/macro/embedded object는 이 기능에 포함하지 않는다.

#### P3 — 선택 확정

1. 신규 **Devbox Launcher 0.1.0** — devbox 앱·profile·repo·job·saved query 전용 launcher와
   current clipboard 일회성 routing.
2. 신규 **Log Lens 0.1.0** — local/Run/WSL/container log tail·merge·filter·export.
3. 전 앱 monitor/DPI-safe window state.
4. Port auto-refresh/diff/favorite/source provenance, Toolbox detection/pipeline/recent/favorite,
   WSL resource/broadcast 안전, API collection import/export/history/binary, Everything filter/saved
   query, Knowledge template, Life Log source explanation/Knowledge draft 상태.
5. Manager read-only Data Inspector·redacted support bundle, Code Pad multi-file rename,
   Run history filter/task import, Workbench template/dependency health/retry, Webhook replay/sequence,
   Repo merged/stale cleanup.
6. Manager의 제한된 Related Tools 감지·사용자 확인 설치·실행. 외부 도구는 모든 native
   action의 secondary 보완재로만 제공한다.

지원 형식, buffer·파일·시간 상한, secret/privacy, destructive safety, public schema,
앱별 목표 버전, PR 지도와 acceptance는 [상세 계획](./superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md)을
축약 없이 기준으로 삼는다.

2026-08-26 현재 7번은 #262, 8번은 #263으로 CI 통과·머지됐다. stable pane key 기반 last
layout/app-local profile, cold/hot profile open, native action palette와 명시적 target broadcast
safety가 들어갔고 tmux/zellij 부재 시에도 native workspace가 완전하게 동작한다. 9번의 첫
독립 기능 #264 Developer Toolbox JSON↔YAML은 PR #396, #265 UTF-8/Hex/Base64/Base64URL byte
codec은 PR #397, #266 bounded radix converter는 PR #398, #267 JSON→TypeScript는 PR #399로
CI 통과·머지됐다. #268은 API Playground request header table에서 duplicate 순서와 enabled
상태를 History/Collection까지 보존하고, 현재 환경의 봉인된 secret 이름을 backend-only reference로
삽입·해제하는 경계를 구현해 PR #400으로 CI 통과·머지됐다. #269는 domain cookie jar가 아닌
request `Cookie` header editor, 기본 비공개 값 입력, 단일 secret reference만 보존하는 persistence
masking, raw Cookie header 충돌과 문법 오류의 fail-closed native 전송 경계를 구현해 PR #401로
CI 통과·머지됐다. #270은 text/file multipart part, part별 Content-Type, runtime-only file
picker 경로, 파일당 25 MiB·전체 50 MiB 제한, missing-file 안전 오류와 History·Collection 경로
제거를 구현해 PR #402로 CI 통과·머지됐다. #271은 Body/Headers/Cookies 응답 탭, 기본
Set-Cookie masking, current-response-only bounded backend raw vault와 확인 후 원문 Headers/Cookies
복사를 구현해 PR #403으로 CI 통과·머지됐다. raw response header는 frontend state·History·
Collection·로그에 저장하지 않고 stale ID·100행/64 KiB 초과·비텍스트 값은 fail-closed한다.
이어 #272는 Knowledge `[[target]]`/alias parser, indexed-note 자동완성, missing/ambiguous/invalid
표시와 backlink source line·column 이동을 구현해 PR #404로 CI 통과·머지됐다. link target은 raw
path로 열지 않고 backend가 유일하게 resolve한 root-relative `.md` path를 canonical open
boundary에서 다시 검증한다. #273은 파일·폴더 이름 변경 전에 이동 경로와 깨질 위키링크 diff를
전체 승인 UI로 표시하고, opaque one-shot plan과 SHA-256 스냅샷 재검증을 거쳐 파일별 atomic
rewrite·filesystem rename·SQLite FTS/link transaction을 적용한다. 기존 key로 계속 유일하게
resolve되는 링크는 쓰지 않고 alias는 보존하며, 충돌·stale snapshot·부분 실패는 전체 중단 또는
rollback하도록 구현해 PR #405로 CI 통과·머지됐다. #274는 Devbox Manager의 catalog 기반 다중 선택,
단일 manifest 조회·순차 실행, 앱별 성공/실패 결과와 실패 항목 exact-mode 재시도를 구현한다.
portable registry commit 실패는 기존 current를 복구하고 setup batch는 여러 installer 마법사 실행을
명시적으로 확인한다. backend SemVer gate는 동일·더 최신 installed version의 stale selection을 no-op로
만들어 batch downgrade를 막아 PR #406으로 CI 통과·머지됐다. #275는 versioned locator와 selected
catalog revision, canonical source manifest와 portable exact executable을 검증해 app별 executable/root/
source manifest를 읽기 전용 패널에 표시한다. installer는 실제 wizard 완료 위치를 추측하지 않고
source manifest provenance만 표시한다. #276은 WSL Desktop의 Docker table을 260px 대응 disclosure
list로 바꾸고 name/state/축약 port를 우선하며, 펼친 detail에 Docker의 ID/image/status/ports 원문을
표시한다. backend는 COMMAND를 포함한 기본 table을 추측하지 않고 `--format`으로 다섯 필드만
조회하고, summary 파생값은 원문이나 storage를 변경하지 않는다. Docker engine 관리·resource
summary는 제외했다.

#277은 기존 workspace snapshot을 다시 걷거나 Git grep/LSP에 의존하지 않고 최대 200개 결과를
파일명·상대 경로 fuzzy score로 정렬한 뒤 디렉터리 tree로 묶는다. 긴 결과는 파일명과 전체 부모
경로를 줄바꿈 가능한 별도 영역에 표시하고, tree의 실제 표시 순서와 `↑/↓`·`Home/End`·`Enter`
선택 순서를 일치시켰다. `Ctrl/⌘+P`는 복원된 workspace도 최신 handler로 읽으며 modal input에
focus를 고정하고 닫힐 때 이전 focus를 복원한다. 파일 열기는 backend가 이미 제한·canonicalize한
absolute path를 그대로 사용하고 새 filesystem walk, storage, network, process 변경은 추가하지
않는다.

#287은 Developer Toolbox Encoding group에 HTML Entity Encode/Decode와 URL Component Encode/Decode를
추가한다. 기존 URL 변환을 `encodeURIComponent`/`decodeURIComponent` 기반의 bounded component
codec으로 교체하고, HTML parser를 내재하지 않는 pinned `entities@8.0.0` 표준 named-entity codec과
직접 검증한 decimal/hex numeric decoder를 추가한다. 두 codec은 UTF-8 input 1,000,000바이트·output 4,000,000바이트·16배
expansion 상한을 공유하고, HTML entity는 token 32자·numeric digit 7자·100,000개 상한을,
URL은 malformed percent와 invalid UTF-8/lone surrogate 검사를 적용한다. output 사전 예상,
entity 누적 상한, fixed error/empty output, raw credential·path·parser 오류 비반향을 순수 fixture로
고정했다. UI는 기존 offline input/output surface를 재사용하되 input 변경 시 이전 output을 비우고
stale sequence 결과를 폐기하며 `aria-busy`/live status, 명시적 clipboard/save action, native
cut/copy/paste·IME·keyboard 동작을 유지한다. HTML parser, 외부 converter, fetch,
자동 저장/전송은 포함하지 않는다. 이미 lock에 있던 BSD-2-Clause codec의 direct edge와 notice
digest를 dependency gate로 검증한다. Windows W2 packaged smoke evidence는 P2 checkpoint에서
수행한다.

**2026-08-27 #290/#291 구현 상태.** Developer Toolbox Text 그룹에 Lorem Generator와 Markdown
Table Formatter를 하나의 cohesive text-transform 사용자 경계로 추가했다. 두 기능 모두 외부
generator/formatter·network·filesystem read·random source 없이 앱에 번들된 deterministic
TypeScript 경로를 사용한다. Lorem은 고정 5문장 corpus에서 문단·문장·단어를 생성하고 수량
1–100, count paste UTF-8 3바이트, 결과 65,536바이트 상한을 적용한다. Markdown formatter는
pipe 행을 원본 순서대로 파싱해 불균일 행을 빈 셀로 보정하고 선택적 `---`/`:---`/`---:`/`:---:`
정렬을 보존한다. 입력 1,000,000바이트·1,000행·100열·셀 4,096 code point·출력 4,000,000바이트
상한과 제어문자·lone surrogate·malformed row/separator fixed error를 적용한다.

공용 Toolbox context surface는 기존 API를 깨지 않는 선택적 `AbortSignal`, bounded explicit
Paste, output action busy callback과 fixed error를 지원한다. Formatter는 다음 event-loop task에
예약하고 superseded queued task를 취소하며, 시작된 bounded core 결과는 sequence·unmount guard로
폐기한다. copy/save와 output context menu는
하나의 in-flight action만 허용한다. 두 UI는 accessible Input/Output label, `aria-busy`, live
status, fixed `role=alert`를 제공하고 native cut/copy/paste·IME keyboard를 가로채지 않는다.
입력·결과를 자동 저장하거나 전송하지 않으며 clipboard read, copy, 고정 plain-text 파일 저장은
사용자가 명시적으로 요청한 경우에만 수행한다. 순수·통합 fixture와 README, v0.5.0 계획,
workthrough를 갱신했으며 신규 의존성·IPC·Rust command는 없다. 필수 cargo/frontend 전체 gate와
Windows W2 packaged offline smoke는 root release checkpoint에서 수행한다.

#278은 기존 언어 서버 status에 retry 실패 횟수·남은 backoff·열린 circuit을 표시하고, 같은 카드에서
수동 `다시 시도`를 실행하도록 확장한다. 관리형 server ref는 설치 index 검증 결과와 결합해 cache
사용 가능/재설치 필요/미설치 상태만 보여 주며 설치 경로는 frontend DTO에 추가하지 않는다. lifecycle
event와 stderr는 앱 실행 중 memory의 언어별 200-entry ring에만 두고, stderr chunk를 native에서 line으로
조립한 뒤 절대 경로·URL·credential 패턴과 oversized line을 제거한다. raw protocol/config/install 오류도
관리 UI에 반향하지 않는다. status/log polling은 generation으로 stale 응답을 버리고, 900px 이내 panel의
본문 하나만 scroll하도록 status/installer nested scroll을 제거한다. LSP server 기능과 managed runtime
설치 계약 자체는 변경하지 않는다. 다음 P1-09 작업은 #279 Code Pad editor·preview 구분이다.

#279는 기존 `previewOpen` state와 `PreviewPane` renderer를 그대로 두고, 프리뷰가 열린 경우에만
editor와 preview surface의 배경·2px 경계·header tone을 분리한다. 활성 editor는
`focus-within` 경계로, 선택된 문서 tab은 기존 selected state와 새 `focus-visible` ring으로
키보드 위치를 드러낸다. preview body 하나만 세로 scroll을 소유하고 일반 본문·긴 inline
code/path는 줄바꿈하되 `pre` code block은 panel 내부 가로 scroll을 사용한다. image·SVG·video·
canvas는 panel width를 넘지 않는다. renderer, Mermaid security mode, state, IPC, storage,
원문/path 전달과 외부 상태는 변경하지 않는다. 다음 P1-09 작업은 #280 Workbench services·ports
입력이다.

**2026-08-27 #288 PR 준비 상태.** Developer Toolbox HMAC는 외부 서비스나 별도 설치 없이
`hmac 0.13.0`·기존 `sha2 0.11.0`의 표준 RustCrypto primitive와 Web Crypto browser preview를
사용한다. `sha256`·`sha384`·`sha512`만 허용하고 key/message 입력은 `utf8`, `hex`, padded
Base64, unpadded Base64URL, 결과는 lowercase hex·padded Base64·unpadded Base64URL로 고정한다.
각 decoded key/message는 1,000,000바이트, encoded field는 2,100,000바이트, tag는 128자로
제한하며 key는 비어 있을 수 없다. Base64 alphabet/padding/pad bit의 canonical 여부와 hex
문자를 native/browser가 동일하게 확인하고, malformed/oversized/unknown 값은 입력·플랫폼
세부사항 없는 고정 오류로 거부한다. Rust `verify_slice`와 Web Crypto `verify`를 통해
constant-time 검증을 수행하고 verify 명령은 boolean만 반환한다.

작업은 `hmac_generate`·`hmac_verify` 두 explicit IPC 명령과 순수 `core/hmac.rs` 경계로
나뉜다. key/message는 history/localStorage/로그/네트워크에 저장하지 않고, 화면 결과의
copy/save만 사용자 명시 action으로 남긴다. 요청 진행 중 algorithm/encoding/input을 잠그고
IME 입력·double action·stale response·unmount를 request sequence로 차단하며, 접근 가능한
label/status/alert를 제공한다. JWT verify, secret persistence, pipeline/handoff, 외부
generator는 이 PR에 넣지 않는다. 새 dependency의 lockfile·license·checksum·전이
`cmov`/`ctutils` notices, 전체 Rust/frontend workspace와 dependency/catalog regression gate는
PR 전 로컬 검증을 통과했다. Windows W2 packaged smoke와 CI dependency audit은 PR/release
checkpoint에서 이어간다.

#280은 `ProjectProfile`을 직접 편집하던 화면을 원문 포트 문자열과 stable-key 서비스 행을 소유하는
`ProfileDraft`로 분리한다. 포트는 쉼표 입력을 저장 직전까지 보존하면서 1~65535, 중복·빈 토큰,
8KiB/128개 상한을 검증하고, Run Manager 서비스 ID는 128자/128개 상한 안에서 행 단위 추가·수정·
삭제와 순서를 보존한다. 이름·profile/service ID·Windows/WSL/Git 경로·WSL distro·포트는 frontend와
Rust IPC/storage 경계에서 다시 검증하고 DTO만 저장한다. 저장소는 missing만 empty로 처리하며 corrupt,
지원하지 않는 version, unknown field, unsafe link/path, 중복 ID/identity와 4MiB 초과를 fail-closed한다.
CRUD는 process-local writer lock 안에서 load/전체 validate/candidate replace/raw-byte CAS/atomic write를
수행해 collision·동시 수정 실패가 기존 파일을 지우지 않게 한다. Run Manager snapshot은 누락과 손상을
구분하고 `activeServices` 전체를 bounded schema로 확인한 뒤에만 health에 사용한다. 편집기는 Enter submit,
IME-safe Escape, autofocus, label/fieldset/aria error, save 중 중복 동작 차단을 제공하며 list/health sequence가
stale 응답을 버린다. raw backend/path/credential/subprocess 오류는 UI에 반향하지 않는다. 서비스 lifecycle,
project environment preflight, template wizard와 runtime 자동 반영은 포함하지 않는다. 다음 dependency
chain은 #410 WSL Desktop runtime snapshot producer 후 #281 Workbench WSL runtime 제안이며, 독립적인
#282 Webhook Lab rule 설명과 #283 example curl은 병렬로 진행한다.

**2026-08-26 #305 구현 상태.** Life Log export는 요청 날짜를 inclusive로, `endMs`를
exclusive로 해석하고 frontend가 계산한 local civil-day `dayBoundaries`를 그대로 보존한다.
날짜 수는 366일, session은 bounded SQL 50,000건, 결과는 4MiB로 제한하며 app/title/privacy와
obvious credential marker를 export 직전에 다시 적용한다. Git은 safe absolute project path만
identity 기준으로 최대 64개 unique 처리하고, `crates/git` bounded runner의 fixed argv·null
stdin·2초 timeout·256KiB stdout·폐기 stderr 및 stable error code를 사용한다. Windows native
save는 사용자가 확정한 matching extension의 absolute path에서만 artifact를 재검증하고
atomic write하며, 취소·잘못된 경로·손상된 output은 대상 파일을 만들지 않는다.

Markdown/JSON/CSV는 같은 source 순서와 deterministic 정렬을 공유하고, Markdown table cell은
pipe·역슬래시·backtick·개행을 escape하며 CSV는 24열 RFC 4180/CRLF를 사용한다. Run Manager의
duplicate service ID와 Knowledge activity identifier·view·freshness·최신 snapshot identity를
검증하고, 요청 범위 밖 snapshot 수치는 summary에 섞지 않는다. 브라우저 preview는 native
DB/Git/snapshot을 읽지 않고 네 source를 `browser_preview_only`로 표시한 bounded range artifact만
다운로드한다. UI는 context menu와 range modal에 하나의 busy guard/request token을 공유하며,
stale·unmount·double-submit을 버리고 initial focus·Escape·Tab trap·focus restore와 고정 오류
안내를 제공한다. Rust export 17개와 Life Log frontend 31개 fixture, app build/clippy/fmt를
focused gate로 확인했으며 Windows packaged W2와 전체 workspace gate는 PR 직전에 수행한다.

**2026-08-26 #410 구현 상태.** WSL Desktop이 Workbench #281과 Life Log가 읽을 수 있는
`wsl-desktop/runtime/v1` snapshot producer를 맡도록 연결했다. 기존
`crates/integration::Envelope::with_views`와 `write_atomic`을 사용해
`%LOCALAPPDATA%\\devbox\\integration\\wsl-desktop\\v1\\summary.json` 하나를 atomic replace하며,
`apps/catalog.json` revision 6에 `snapshot:wsl-desktop/runtime/v1` capability를 선언했다.

producer는 `wsl.exe --list --running --quiet`의 이미 실행 중인 distro만 대상으로 하고, 각
distro에 `wsl.exe -d <validated-distro> -- docker ps -a --no-trunc --format
{{.ID}}\\t{{.Names}}\\t{{.State}}\\t{{.Ports}}` 고정 argv를 순차 실행한다. stopped distro를
조회 때문에 시작하지 않으며 shell/사용자 command/환경 확장을 사용하지 않는다. Docker raw
image/status/ports/COMMAND/labels/env, terminal session id·pane key·cwd·title·profile command는
공개 경계에서 제외한다. container는 bounded hex ID/name과 정규화 state만, port는 검증된
`published`, `target`, `protocol` tuple만 내보내고 IPv4/IPv6 binding을 deterministic dedupe한다.

상한은 distro 64개·이름 128 bytes, container 256개/distro·512개 전체, ID 64 bytes·name 256
bytes, mapping 32개/container·1,024개 전체, terminal 256개/distro, stdout 4MiB·stderr 64KiB·
line 16KiB·child timeout 5초다. 성공한 빈 Docker 목록은 `available`, exit 127은 `missing`,
기타 non-zero는 `error`로 구분한다. malformed row, duplicate/unsafe identity, timeout, I/O 또는
overflow는 빈 결과로 덮어쓰지 않고 기존 last-good snapshot을 보존한다. 앱 setup의 60초 주기와
dashboard 성공 refresh/terminal start·close·cleanup의 250ms debounce는 producer당 단일
coordinator worker로 합친다. 관련 순수 fixture는 parser, exact argv, state/port normalization,
bounds/privacy, atomic replacement, last-good 보존과 temp residue를 검증한다. Workbench 파일,
자동 Docker/WSL mutation, resource summary, Run Manager 추론과 Log Lens는 이 PR에 포함하지 않는다.
상세 wire contract와 W1 packaged 검증 항목은 [v0.5.0 네이티브 우선 계획](./superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md)의
P1-05 `#410` 절을 따른다.

**2026-08-26 #281 구현 상태.** Workbench가 `wsl-desktop/runtime/v1`을 read-only로 소비해
published TCP host port를 편집 중인 profile draft에 제안한다. consumer는 envelope/view와 distro,
container, mapping 전체 bounds/identity를 검증하고 port 숫자순·source provenance순으로 dedupe한다.
producer의 60초 주기에 맞춰 2분 이하는 fresh, 15분 이하는 stale, 그 이후는 expired로 분류하며
stale 반영은 확인하고 expired·missing·corrupt는 차단한다. 반영 직전에 snapshot을 다시 읽어 후보
소실과 freshness 변경을 검사하고, 기존 expectedPorts 순서를 보존한 채 선택된 새 port만 append한다.
preview/accept는 profile store 저장, WSL/Docker command, resource start, producer write를 수행하지
않으며 snapshot 절대 경로·container ID·raw Docker detail은 frontend로 전달하지 않는다. W1에서는
packaged fresh/stale/expired 전환, producer missing/corrupt, accept 후 Save 전 저장소 불변을 확인한다.

**2026-08-27 #308 구현·감사 완료 (PR 전).** Devbox Manager의 custom install root는 기존 설치를
이동하는 마법사가 아니라, 사용자가 선택한 이미 존재하는 canonical 빈 디렉터리를 다음 portable
설치에 연결하는 native preview/confirm 흐름이다. `preview_install_root`는 4,096-byte 경로,
root/home/workspace·환경변수·symlink/reparse·canonical alias·기존 항목·쓰기 권한·최소 128 MiB
free-space를 검사하고 파일을 변경하지 않는다. active manifest 또는 root artifact가 있으면
`existing-install`로 fail-closed한다.

`apply_install_root`는 preview의 `registryRevision`을 CAS로 재검사한 뒤 후보에 `apps/`와 빈
`registry.json`만 만들고 versioned locator를 atomic replace한다. locator/manifest의 strict schema와
16 KiB/1 MiB·256 row bounds, catalog provenance, stale/overflow, rollback residue를 Rust fixture로
검증하며, 기존 root·binary·partial·user data는 이동·삭제하지 않는다. 적용 후 Manager의 install,
launch, rollback, install path 조회는 locator가 지정한 active root를 사용하고, 설치 directory와
download target도 각 component 및 `.partial` sibling의 symlink/reparse를 다시 검사한다.

locator가 없는 v0.4.x 상태만 legacy fallback으로 사용하며, corrupt/oversized locator 또는
symlink/reparse parent는 default root로 복구하지 않는다. active portable record는 exact layout와
intermediate component identity를 다시 확인하고, 기존 `.partial`은 `create_new`로 덮어쓰지 않는다.
WSL focused Rust 75/23 tests, manager/launch check·clippy·fmt, frontend 19 tests와 build를 통과했다.
PR 전 전체 Rust workspace test·check·clippy(`-D warnings`)·fmt과 17개 frontend workspace build도
통과했다. Windows packaged acceptance는 CI와 W2에서 검증한다.
custom root removal, 기존 설치 migration/병합, root reset과 user-data 삭제는 #308에서 자동
이동·삭제로 우회하지 않고 #309의 별도 안전 경계로 분리했다.

**2026-08-27 #309 구현 상태 (PR 전).** Devbox Manager의 portable 제거를
`preview_remove_app` → 별도 확인 → `remove_portable_app`의 두 단계로 연결했다. preview와
remove 모두 현재 catalog-visible app, active locator provenance, manifest digest와 canonical
`apps/<app>/versions/<version>/<app>.exe` layout을 native에서 재검증한다. 삭제는 app-owned
`current.json`·versions tree를 bounded preflight한 뒤 exact regular file/directory 목록만 깊은
순서로 처리하고 symlink/reparse, special, foreign entry, traversal과 arbitrary path를 거부한다.
기본 root와 custom root를 구분하지 않으며, installer의 wizard 위치·uninstaller와 앱 사용자
data는 제거 대상에서 제외한다.

manifest를 먼저 CAS claim하고 실패·부분 제거 시 동일 digest일 때 원래 bytes를 복원한다. 이미
삭제된 exact final executable은 recovery에서만 허용해 interrupted removal을 `partial`/`missing`
상태로 재시도할 수 있고, 경쟁 writer가 있으면 덮어쓰지 않는다. frontend는 target/count/size와
user-data 보존을 preview panel에 표시하며 stale preview, pending duplicate action, unmount 뒤
늦은 응답을 폐기한다. Manager 81개 focused Rust test와 21개 frontend test를 통과시키고,
Windows junction/ACL/packaged W2 및 전체 workspace gate는 CI/W2에서 확인한다.

**2026-08-27 #314 구현 상태.** Webhook Lab은 opaque history ID로 읽은 masked request만
앱 전용 `%LOCALAPPDATA%\com.devbox.webhooklab\fixtures.json`에 저장한다. schema v1은 fixture
200개·파일 8 MiB·bounded method/target/header/body/timestamp를 적용하고, Authorization/Cookie/
token/secret/password/auth 계열과 known credential marker를 `[REDACTED]`로 치환한다. unsafe
absolute/path-traversal target은 고정 marker가 되며 frontend가 경로나 raw body를 제공하지 않는다.
corrupt·oversized·symlink 파일은 자동 복구하지 않고 fixed error로 보존하며, atomic replace와
raw-byte CAS/process-local lock으로 partial write와 concurrent overwrite를 방지한다. 목록은
timestamp 내림차순+ID tie-break로 결정적이며 UI는 fixture 저장·삭제·전체 삭제·로컬 response-rule
초안 action을 하나의 busy guard와 접근 가능한 label로 보호한다. #315 API handoff와 #362 replay/
sequence는 구현하지 않는다.

#293 API Playground OpenAPI import는 로컬 파일과 HTTP(S) URL의 JSON/YAML 3.0/3.1 문서를 대상으로
bounded source/parser 경계와 operation preview를 고정한다. 로컬 file read는 완전 오프라인이고 URL은
2,048자, connect 5초/수동 redirect를 포함한 전체 15초, 같은 host·유효 port redirect 3회, decoded response 4 MiB 상한의 native fetch만
사용하며 gzip/deflate/brotli/zstd는 해제 후 상한을 적용한다. userinfo/credential-shaped query/fragment 및 원문 URL 반향을 거부한다. server/path/method/parameter/body/auth metadata를 deterministic하게
미리 보고 한 operation을 현재 draft에 적용하거나 체크한 operation을 새 Collection에 추가하며,
기존 Collection overwrite와 자동 전송은 없다. JSON/YAML unsafe graph·prototype key·`$ref`·지원하지
않는 auth/method와 표현할 수 없는 path/operation-level server override는 고정 오류와 operation 단위
격리로 처리한다. 생성 request의 parameter/header/cookie row는 각각 100개, 구조화 body는 512 KiB로 제한하며
environment reference도 secret처럼 비워 둔다. Swagger UI bundle, code generation, secret 주입은 범위에서 제외한다.

**2026-08-27 #295 구현 상태.** API Playground에 REST request draft를 그대로 사용하는
native-first SSE streaming을 추가했다. GET/POST, 기존 auth/environment/header/Cookie/params와
JSON/form/raw/multipart text body를 지원하고, native는 Rust request resolver에서 secret을
전송 직전에만 해제한다. browser preview는 secret environment와 file multipart를 차단하고
CORS/forbidden Cookie header·redirect 차이를 표시한다.

native/browser parser는 UTF-8 incremental chunk, 최초 BOM, CR/LF/CRLF, comment, `event`, multiline
`data`, `id`, empty id, decimal `retry`와 EOF flush를 동일하게 처리한다. malformed UTF-8/NUL,
malformed retry와 line/field/data/name/id overflow는 raw source나 runtime 오류를 노출하지 않는
fixed error로 실패한다. decoded/retained stream은 각각 20 MiB, 10,000 event 또는 20 MiB,
line/field 64 KiB, name/id 256 bytes, data 1 MiB, retry 0–60 s로 제한하고, UI는 최근 1,000개와
oldest-first eviction 수만 표시한다. pause는 render만 멈추며 bounded history는 계속 유지한다.
environment는 최대 100개, key 128 bytes, value 64 KiB로 먼저 검증해 browser/native 입력
경계를 일치시킨다.

transport는 opaque session 하나와 abortable task를 사용하며 connect/idle/total timeout을
100–30,000 ms/100–300,000 ms/1–3,600 s로 제한한다. native redirect는 최대 10회, browser
redirect는 차단한다. cross-origin redirect에는 sensitive header/auth/body를 보내지 않고
credential-bearing destination은 차단하며 Accept는 `text/event-stream`, user `Last-Event-ID`는
무시한다. reconnect는 기본 off, opt-in 때만 server retry를 250 ms–60 s로 clamp해 최대 5회로
제한한다. raw URL/path/chunk/credential/network/parser stderr는 DTO·DOM·log·history·telemetry에
반향하지 않으며 event는 자동 저장하지 않는다. 사용자가 누른 `Copy masked events`만 현재
메모리 표시 범위를 clipboard로 보낸다.

Rust/TypeScript pure fixture는 chunk split/BOM/newline/multiline/empty id/retry, invalid UTF-8·NUL,
bounds·eviction과 safe DTO/session filtering을 고정한다. PR 직전 검토에서 lone CR의 이중 line
종료, terminal update 뒤 listener 미해제, browser multipart file/part Content-Type의 늦거나
silent 처리, GET multipart body 누락을 수정하고 Rust/TypeScript/App 회귀 fixture를 추가했다.
`tokio`는 이미 lock/direct dependency에 있는 runtime의 `time` 기능만 사용하고 새
parser/transport library나 lockfile 변경은 추가하지 않았다. 최종 Linux 검증은 Rust 69 test,
all-target check/Clippy/fmt, frontend 20 files/160 tests와 production build, dependency/notices와
catalog gate를 통과했다. Windows W2에서는 loopback GET/POST stream, native/browser parity,
cancel/reconnect, CORS/redirect, redaction, no-persistence, bounded failure와 keyboard/IME/focus를
packaged smoke로 확인한다.

```
Stage -1   결정을 문서에 고정 (PR 1)                                  ✅
Stage 0a   통폐합·네이밍 (PR 2~4) — identifier com.devbox.*          ✅
Stage 0b   배포 정상화 (PR 5~13) — 버전 단일 원본, 카탈로그, manifest  ✅
Stage 0.5  공용 프리미티브 (PR 14~17) — crates/wsl·search, packages/tokens, CSP ✅
Stage 1    정확성·privacy (PR 18~25)                                  ✅
Stage 2    앱 간 연동 (PR 26~30) — integration snapshot, ProjectProfile ✅
Stage 3    기존 앱 깊이 (PR 31~39) — Run Manager 관찰성, Code Pad 복구 ✅
Stage 4    Workbench — ProjectProfile 기반 orchestration 앱          ✅
Stage 5    Webhook Lab, Dev Environment Doctor, Repo Manager          ✅
v0.4.1     핫픽스 — 터미널 PTY·끊긴 앱 간 링크·Run Manager 시작 panic·identifier 이관 수정  ✅ (C1/C2 Windows 수동 acceptance는 issue #176에서 post-release 관리)
v0.4.2     API Playground secret persistence 보안 핫픽스 — stable 27 assets·packaged H1  ✅
v0.5.0     네이티브 기능 강화 + handoff + Devbox Launcher·Log Lens (목표 15개 앱)  ◻
```

## 현재 상태
- 13개 앱 모두 WSL에서 구현 완료 (Rust 유닛 테스트 + clippy + 프론트 빌드 통과)
- 각 앱은 기능 단위 PR로 main에 머지됨
- v0.4.0 정식 배포 완료 (13개 앱)
- v0.4.1 안정판 핫픽스 배포 완료; C1/C2는 legacy path 제거로 재현하지 못했으므로 Windows packaged-runtime
  검증과 구분한다.
- v0.4.2 안정판 보안 핫픽스 배포 완료; exact stable asset의 manifest·size·SHA-256과 packaged
  H1-A~D·cleanup을 통과했다.
- [통합 Windows 검증 체크리스트](https://github.com/jihoon22-lee/devbox/issues/176) — 남은 Windows 실기·패키지·프로토콜·경로·시각
  acceptance를 post-release 수동 체크리스트로 관리한다.
