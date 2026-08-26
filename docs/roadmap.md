# Roadmap

13개 앱을 순차적으로 완성하고, 공통 코드가 실제로 필요해지면 그때 `crates/`·`packages/`로 추출한다.
처음 8개 앱(port-manager~devbox-manager)을 완성한 뒤 에디터(code-pad)·예약 실행·서비스 관리자(run-manager),
그리고 Stage 4·5 앱(workbench·webhook-lab·repo-manager)을 추가했다.

## Phase 1 — Tauri 기본기 ✅
- [x] **port-manager** — IPC, Rust 기초, netstat 파싱, 포트/프로세스 관리
- [x] **developer-toolbox** — 사이드바 UI, 소형 도구 13종 (hash/uuid/regex/diff는 Rust)

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
  민감 헤더 masking, LAN 공개 기본 차단 (JSON fixture·API Playground 변환은 설계 문서의 향후 항목)
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
2. Port Manager command line·WSL identity-safe kill.
3. Toolbox UUID v7/ULID/HTML/URL/HMAC/JWT verify/Lorem/Markdown table/내장 QR.
4. API Playground OpenAPI 3.x import, GraphQL, SSE, WebSocket.
5. Everything+ text/code/Markdown 및 PDF/DOCX/XLS(X)/ODS content index.
6. Knowledge global quick capture·image asset, Life Log Markdown/JSON/CSV export·규칙 기반 요약.
7. Manager custom install root·데이터 보존형 안전 제거.
8. Code Pad LSP cache/local archive, Run Manager log search, Workbench project environment,
   Webhook fixture/API handoff, Repo Manager history/diff/stage/commit/fetch/FF-only pull/push.

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
source manifest provenance만 표시한다. 다음 P1-09 작업은 #276 WSL Desktop Docker compact 표시다.

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
