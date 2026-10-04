# 감사·추가 개선·검증 추적표

2026-10-03 실행 중. R01–R14의 주요 수정과 좁은 로컬 회귀를 아래 작업별 근거에 연결했다. Windows packaged 사용자 여정 runner와 전체 gate는 R15에서 연결했으며, 후보 실행 전 상태는 **NOT_RUN**이다. 구현·로컬 PASS를 출시 수용 PASS로 해석하지 않는다. 최종 PR·candidate·release 결과는 같은 통합 PR 본문과 Actions artifact에 추가한다.

코드 경로는 현재 `/home/jihoon/projects/devbox` 기준이다. 다른 checkout에서는 저장소 상대 위치를 그대로 사용한다. 상세 변경 파일과 assertion은 소유 작업 문서에 있다.

### 후보 검증 후속 보정

통합 PR #616 이후 출시 차단 수정은 최소 보정 PR에 묶는다. #617은 Windows 파일명 충돌,
#618은 commit/passive-effect 사이 작업 시작과 Transform 명시 저장의 수명 경계를 수정했다.
후보 `37126573659`는 네 제품과 7개 자산 조립, product-shells·Knowledge·cross-product·WSL2를
통과했지만 API 응답 선택과 installer 준비에서 실패하여 승격 대상이 아니다.

- API: Windows 진단 `37132833899`에서 `.response`와 `.response-panel` 높이 0,
  본문 높이 28px(패딩만 남음) 및 SSE 영역에 가려지는 화면을 확인했다. 응답 영역이 축소되지 않도록
  수정하고 버튼 줄바꿈을 적용했다. 같은 개발용 Chromium 요청의 수정 전 0px → 수정 후 응답 320px 이상,
  본문 160px 이상을 확인했으며 720×480에서도 본문 선택·포인터 접근 및 헤더 탭을 확인했다.
  이는 browser 레이아웃 근거이고 수정된 Windows 설치본 수용 완료를 뜻하지 않는다.
- Installer: 같은 Actions step 안에서 새 `GITHUB_ENV` 값을 읽어 WSL fixture 소유권 검사에
  실패했다. 생산자와 소비자 step을 분리했다. 이 실패 이전에는 40개 설치 UI 여정이 실행되지 않았다.
- 설치 UI 진단 `37134283547`·`37134661732`에서는 Welcome 창의 활성 `다음 >` 컨트롤(id 1)을
  관찰했지만 UI Automation이 Button 클래스를 Pane(50033)·Invoke 미지원으로 읽었다. 숫자 식별자
  비교만으로는 해결되지 않았다. .NET Framework 기본 proxy 초기화가 PowerShell 동적 호출에서
  `NullReferenceException`을 일으킴을 별도 재현했고, typed C# 진입점으로 기본 provider를 초기화한다.
  Windows의 별도 raw Win32 Button fixture에서 기존 Pane/Invoke 미지원 재현과 수정 후 Button(50000)
  인식·실제 Invoke·소유 marker·정상 종료를 확인했다. 등록 전 실패가 owner receipt 오류에 가려지지
  않도록 원래 오류·단계·소유 창 구조를 보존한다. 설치 완료 근거는 후속 실제 여정에서 확인하며
  초기화 및 fixture 성공만으로 설치 PASS를 기록하지 않는다.
- 후속 진단 `37170395096`은 실제 NSIS Welcome·경로 선택·설치·Finish와 Control Center 연결까지
  진행했다. 당시 일반 CDP 오류로 중단됐으며 `37170824962`에서 첫 화면 캡처는 성공하고 스크롤 뒤
  Workspace 열기의 좌표 hit-test가 실패함을 확인했다. 이후 활성화·40개 여정은 완료되지 않았다.
  GUI 프로세스 호출 뒤 미설정 `LASTEXITCODE`를 읽던 정리 코드도 발견해 정확한 자식 프로세스의
  종료를 기다리도록 보정했고 `37170824962`의 실제 정리는 통과했다.
  실제 Chromium에서도 root scroll 뒤 동일한 hit-test 실패를 재현했다. 검증 driver는 화면 기준
  pointer 좌표와 페이지 기준 DOM hit-test 좌표를 구분하며, 실제 pointer 클릭과 덮인 요소 거절을
  확인했다. 설치 단계 목록 번호가 다음 항목에 붙어 보이는 간격도 기존 토큰을 유지해 수정했다.
- 진단 `37171221250`에서는 실제 Workspace 열기 클릭 뒤 제품 오류가 표시됐다. native가 보관한
  `\\?\D:\…` 형식의 canonical root를 외부 입력 검사에 다시 넘겨 정상 설치를 거절하는 결함을
  확인했다. CapturedScope에서 얻은 로컬 디스크 경로만 일반 형식으로 변환하고 동일 filesystem
  identity를 확인한다. 외부 입력의 device·UNC·verbatim 경로 거절은 유지한다. 순수 Rust 경로 회귀와
  Windows 파일 identity 확인은 통과했으며 실제 수정 바이너리의 설치 실행은 새 후보에서 검증한다.
- 새 설치의 Inventory·복구 이력보다 준비 안내와 다음 동작을 먼저 배치했다. 실제 컴포넌트·CSS와
  합성 native 응답을 사용한 브라우저에서 1024×720·720×480 모두 제품 열기와 준비 후 다음 단계를
  첫 화면에서 확인했다. 명시 확인 전 실행 차단과 복원·재설치·업데이트 조건은 유지한다.
- API 진단 `37170520484`는 요청·응답 선택·Transform 전달·재시작을 통과했다. 진단용 CSS가 fixture의
  페이지 새로고침에서 제거되는 문제를 수정하고 문서별 적용 단계·계산값을 기록했다. 이전 payload에
  현 CSS를 적용한 진단이며 새 배포 bytes 자체의 통과나 최종 후보 수용으로 취급하지 않는다.
- 보관 후보를 재사용한 진단은 runner와 payload source를 구분하고 별도 diagnostic artifact에만
  기록한다. 최종 exact-main 후보의 설치/UI 수용과 승격 근거를 대체하지 않는다.
- #619의 PR CI `37172279567`과 exact-main CI `37172648247`은 모두 통과했다. 새 후보
  `37173572971`(`c91d0326`)은 네 제품·7개 자산, 네 native scope 및 WSL2/Docker를 통과했고,
  실제 설치 뒤 `open_setup_product` 성공으로 경로 수정도 확인했다. 그러나 Control Center를
  정상 종료·재개하는 검증의 창 선택이 실패해 설치 여정과 최종 seal은 실패했다. 출시 가능한 후보가 아니다.
  보관 진단 `37177932549`는 같은 PID 아래 `Tauri Window` 외에 `Tao Thread Event Target`와
  설치 identifier의 `-sic` 제어 창도 native/UIA에서 표시 중으로 보고되는 것을 확인했다.
  단순 visible 필터로 해결되지 않으므로 정확한 제품·보조 창 식별을 보정한다. 실제 두 앱 창이나
  modal의 모호성은 계속 거절하고, 보관 진단의 결과를 새 후보의 승격 근거로 재사용하지 않는다.
- 보관 진단 `37178443869`에서 정확한 보조 창 제외 후 Control Center 종료·재개와 네 제품 실행은
  통과했다. 다음 Workspace 복구 화면에서 요구한 기본 client 크기와 실제 치수가 달라 중단됐다.
  일회성 hosted Windows에만 지원 display mode를 임시 적용하고, 크기·화면·소유 창 측정은
  assertion 전에 보존하도록 보완했다. 기본 1180×780·최소 720×480 수용 기준은 유지한다.
  남은 Workspace 종료 driver 세 곳도 접근성 dialog 이름으로 종료 검토를 식별하도록 맞췄다.

최종 보정 PR·새 후보·공개 결과는 같은 PR 본문과 Actions artifact·Release notes에 기록한다.

## 1. 기존 감사 23건의 누락 없는 배정

| ID | 우선 | 재현 조건·문제 | 핵심 위치 | 작업 | 검증 ID·종료 조건 |
|---|---|---|---|---|---|
| S1 | P1 | import 배너가 추가되면 네 앱 본문 200px로 이동 | `/home/jihoon/projects/devbox/packages/product-shell/src/index.tsx:93`, `styles.css:32` | R06 | UI-01: 모든 delivery 상태의 본문/primary 가시성 |
| W0 | P1 | setupOnly store selected 뒤 금지 registry snapshot→실패/재생성 반복 | `/home/jihoon/projects/devbox/apps/devbox-workspace/src/RegistryGate.tsx:109` | R07 | INSTALL-01/02: selected에 snapshot/start_empty 재호출 없음 |
| W1 | P1 | Files edit→X/restart, recovery writer 미연결로 초안 소실 | `/home/jihoon/projects/devbox/apps/devbox-workspace/src-tauri/src/component.rs:349`, `/home/jihoon/projects/devbox/packages/workspace-features/src/files/App.tsx:1438` | R02 | WORK-01: 실제 편집→기록→종료/재열기, 실패 시 창 보존 |
| W2 | P1 | Source dirty→Agents 다른 worktree 검토, guard 우회 | `/home/jihoon/projects/devbox/apps/devbox-workspace/src/Workspace.tsx:288` | R02 | WORK-02: native select 전 차단, 취소 후 초안/context 보존 |
| W3 | P2 | 새 agent worktree 등록 후 parent registry stale | `/home/jihoon/projects/devbox/apps/devbox-workspace/src/agents/agentFlow.ts:87` | R03 | WORK-03: base-only 초기 상태에서 즉시 Source/Dependencies 일치 |
| AS-01 | P1 | JSON/raw→None 후 이전 body native 송신 | `/home/jihoon/projects/devbox/crates/http-client-engine/src/commands/request.rs:1252` | R04 | HTTP-01: 실제 echo body=0, cURL/browser/native 일치 |
| AS-02 | P1 | query/form 예약 문자 비인코딩→의미 변조 | `/home/jihoon/projects/devbox/crates/http-client-engine/src/commands/request.rs:3121` | R04 | HTTP-02: decode pairs/중복/순서/한글 보존 |
| AS-03 | P1 | Webhook 외부 POST 후 history 화면 미갱신 | `/home/jihoon/projects/devbox/packages/api-studio-features/src/webhooks/App.tsx:313` | R09 | WEB-01/02: idle arrival/route 복귀/실패 후 읽기 회복 |
| AS-04 | P1 | var2 하나 import→+변수→기존 값 삭제 | `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/components/RequestSidebar.tsx:521` | R08 | ENV-01: sparse/secret 충돌 시 기존 값 보존 |
| AS-05 | P2 | 환경 변수 이름 입력/수정·개별 삭제 불가 | 같은 RequestSidebar.tsx:456 | R08 | ENV-01: baseUrl/token을 UI로 만들고 요청까지 사용 |
| AS-06 | P2 | 새 환경 생성 뒤 첫 기존 환경 선택 | `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/hooks/useEnvironmentPersistence.ts:149` | R08 | ENV-01: 생성 id 선택/실패 시 이전 선택 유지 |
| AS-07 | P2 | auth 변경 뒤 비활성 field 변수 때문에 send 차단 | `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/lib/runner.ts:191` | R04 | HTTP-01/03: 활성 field만 검사·resolve |
| AX-01 | P1 | MCP 연결 A에서 B/None 선택, 호출은 A 자격 | `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/ProtocolLab.tsx:989`, native `mcp.rs:560` | R05 | AUTH-01/02: active grant와 UI 일치, mutation 1회 |
| AX-02 | P2 | pipeline 편집이 신규 id 생성, 20개 포화 후 저장 회복 불가 | `/home/jihoon/projects/devbox/packages/api-studio-features/src/transforms/workflows/SmartWorkflowPanel.tsx:259` | R11 | TRANSFORM-01: full library 편집/삭제/새 저장/재열기 |
| AX-03 | P2 | gRPC 2개 message 뒤 error→응답0으로 소실 | `/home/jihoon/projects/devbox/crates/http-client-engine/src/commands/grpc.rs:1003` | R10 | GRPC-01: 부분2개+terminal error와 history 일치 |
| K1 | P1 | 노트 A→B→Undo→B에 A 내용 autosave | `/home/jihoon/projects/devbox/packages/knowledge-features/src/notes/components/MarkdownEditor.tsx:220` | R01 | DOC-01: 실제 B disk에 A sentinel 없음 |
| K2 | P2 | 반복 observe 뒤 idle 종료, 마지막 입력 후298초 집계 | `/home/jihoon/projects/devbox/crates/activity-engine/src/core/sessionizer.rs:55` | R13 | ACTIVITY-01: idle-tail 구간 제외, pause/clock 경계 보존 |
| K3 | P2 | regex 기호/한글 제거한 FTS가 정상 후보 제외 | `/home/jihoon/projects/devbox/packages/knowledge-features/src/search/App.tsx:453` | R12 | SEARCH-01: candidate→worker 통합 정확성·한도 partial |
| K4 | P2 | Settings의 history 재생성 항상 disabled/현재 digest 오용 | `/home/jihoon/projects/devbox/packages/knowledge-features/src/activity/App.tsx:590` | R13 | ACTIVITY-02: 과거 entry 기간/timezone으로 재생성 |
| K5 | P2 | 원본 삭제 뒤 journal 열기 실패, UI 복원/본문 확인 불가 | `/home/jihoon/projects/devbox/packages/knowledge-features/src/notes/components/RecoveryControls.tsx:92` | R01 | DOC-02: 원본 없이 preview/copy, 명시 CAS 복구 |
| K6 | P2 | installed QuitGuard가 실제로 유지되는 수집을 중지한다고 안내 | `/home/jihoon/projects/devbox/apps/devbox-knowledge/src/QuitGuard.tsx:97` | R13 | ACTIVITY-03/AGENT-04: owner별 문구와 실제 process 일치 |
| K7 | P2 | 최소720×480 기본 노트 편집폭33px | `/home/jihoon/projects/devbox/packages/knowledge-features/src/notes/App.css:35/354`, Knowledge tauri.conf.json:20 | R06 | UI-01/02: 최소 창 편집폭≥320px, toolbar 도달 |
| C1 | P1 | products 화면 recordSuiteHealth가 route admission에서 거절 | `/home/jihoon/projects/devbox/apps/devbox-control-center/src/Health.tsx:76`, native `ipc/delivery.rs:131` | R07 | INSTALL-01: 실제 화면 route=header, 다음 단계 완료 |

원 감사 근거 구분: S1/K1/K7 브라우저 UI 재현; W0 실제 권한을 반영한 RED 2개; AS-01/02 순수 Rust 분기 probe; AS-04/06/07 TypeScript 순수 함수 probe; K2 실제 sessionizer 실행; 나머지는 생산 코드 연결 확인. 이번 계획 작성에서 이들 시험을 다시 돌리지 않았다.

## 2. 계획 단계에서 추가 확인한 코드 결함

| ID | 분류/근거 | 문제 | 작업 | 종료 조건 |
|---|---|---|---|---|
| N01 | P2 · 코드 확인 | `IncomingCommands.tsx`/`SuiteConnection.tsx`의 refresh 성공이 이전 조회 issue를 해제하지 않아 복구 후에도 실패 표시가 남음. write issue와 섞지 않고 해제 필요 | R06 | 읽기 실패→최신 성공 후 해당 경고 소멸, late failure 무시; unrelated write 실패는 유지 |
| N02 | P2 · 코드 확인 | Requests App의 warning이 HTTP 전용 조건 안에 있어 Protocols에서 환경 save 실패 이유가 숨겨짐 | R08 | ENV-02: Protocols/History/Requests 모두 오류 표시와 write lock 설명 |
| N03 | P2 · 코드 확인 | Activity shift는 day/week/month만 변경; timeline 화살표는 날짜를 바꾸지 않음 | R13 | ACTIVITY-02: timeline 날짜±1일·native 기간 이동 |
| N04 | P2 · 코드 확인 | Activity idle threshold의 UI void 호출/native set_setting 오류 무시→거짓 성공 | R13 | ACTIVITY-03: read-only DB 오류 전달, 이전 ack값 보존, 재시작 readback |

N01 근거: `/home/jihoon/projects/devbox/packages/product-shell/src/IncomingCommands.tsx` refresh.then은 setReviews만, SuiteConnection.tsx refresh.then은 setStatus만 호출한다. 각 catch는 setIssue를 호출한다. 성공적 refresh가 error를 해제하는 경로가 없다.

N02 근거: `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/App.tsx:1532`의 `workspace !== 'protocol'` 조건 내부에 persistenceWarning. sidebar의 환경 UI와 persistenceReady=false 잠금은 protocol에서도 사용된다.

N03/N04 근거: `/home/jihoon/projects/devbox/packages/knowledge-features/src/activity/App.tsx:757`, `components/ActivitySettings.tsx:208`, `/home/jihoon/projects/devbox/crates/activity-engine/src/commands/tracking.rs:313`, `core/db.rs:249`. DB에는 이미 fallible `try_set_setting()`이 있으므로 이를 활용한다.

이 4건은 추가 코드 검토의 확정 경로이며 실제 Windows 재현을 이번에 실행한 것은 아니다. 기존 감사23건에 몰래 합쳐 재현 수를 부풀리지 않는다.

## 3. 고장으로 단정하지 않는 추가 개선

| ID | 왜 필요한가/현재 근거 | 구체 범위 | 작업·검증 |
|---|---|---|---|
| I01 | 설치 UI가 여러 제품과 helper 단계 조합을 요구 | journal 원장의 연속 안내·즉시 상태 갱신·중단 재개·다음 행동 | R07 INSTALL-01/02/03 |
| I02 | 일반 기능 불가 상태에서도 Agent/incoming 조회와 재시도 UI 노출 | availability 유도, 예상 대기/실제 실패 구분 | R06 UI-01, R07 |
| I03 | 같은 제품 안의 고정 panel과 toolbar가 작은 창에 경쟁 | 모든 route 대표 최소창·접근성·IME/초점·오류/빈상태 계약 | R06 UI-01/02 |
| I04 | Knowledge ‘버리고 종료’의 저널 보존 의도가 불분명 | keep-close와 명시적 영구 폐기 분리·drain·첫 offline 로컬 복구 | R01 DOC-02/03 |
| I05 | API 이전 응답을 전송/취소 중 유지하고 일부 행 이름이 모호 | 이전 응답 표시·요청 상태·행 레이블/포커스 | R04 HTTP-03/UI-02 |
| I06 | 환경 CRUD의 보존/실패/이름 충돌·secret 제약이 화면에 부족 | key CRUD·collision·ack·모든 route 오류, secret 재노출 없음 | R08 ENV-01/02 |
| I07 | 주기 갱신을 넣으면 rule draft/선택/stale 응답과 충돌 가능 | single-flight·hidden pause·부분 실패·마지막 정상 결과 유지 | R09 WEB-01/02 |
| I08 | workflow 저장완료/dirty/포화 상태 구분 필요 | ID 보존·save-as 분리·삭제/실패복구·입출력 비영속 유지 | R11 TRANSFORM-01 |
| I09 | 저장 검색 의미가 현재 source/regex 상태에 의존하고 index 상태 갱신이 제한적 | 현 저장 포맷 의미 명시·원자 load·partial/offline/expiry 상태 | R12 SEARCH-01/02 |
| I10 | privacy/동의/일시정지/창 종료/자동시작의 의미가 분산 | actual owner/status 표시·설정 ack·범위 설명 일치 | R13 ACTIVITY-03, R15 AGENT-03/04 |
| I11 | Tasks/Terminal 실행 응답 유실 때 실패/결과 불확실/재실행 혼동 | receipt 조회·read-only retry·대상 이동·provider 재검토 | R14 RUNTIME-01/02/DEPS-01 |
| I12 | 이전 installer 시험은 silent+직접 API/helper, foreground 수집은 사용자 확인 대기 | owned UI 입력·disk/wire/OS 관찰·source/digest matrix·fail closed | R00/R15/R16 전체 |

Source 초안의 crash 후 영구 복원, 모든 검색 모드 저장 포맷 확대, gRPC incremental streaming UI, 새로운 provider/프로토콜/앱은 이번 범위 밖이다. 필수 사용 흐름을 완성하는 위 항목은 단순 권장 사항으로 남기지 않고 소유 작업의 수용 조건에 포함한다.

## 4. 구현 이후 기록 형식

아래 한 행을 실제 결과로 작성하고 증거가 없으면 NOT_RUN으로 둔다. 이번 문서에는 가짜 PR 번호나 PASS를 채우지 않는다.

```text
ID / owner work item / single PR URL / tested source SHA / L1-L4 또는 L5 scenario / run artifact / PASS|FAIL|NOT_RUN / 남은 제한
```

R00 matrix 검사로 기존23·추가4·개선12의 소유 작업 누락과 scenario 없는 항목을 거부한다. R15/R16은 해당 source의 필수 scenario가 모두 채워졌는지 확인한다. 결함 수정 commit만 존재하고 실제 사용자 여정이 미실행이면 완료로 닫지 않는다.


## 5. 구현과 로컬 확인 근거

아래 commit은 작업별 변경 이력이며 최종 배포 source는 통합 PR의 main 머지 SHA다. 모든 작업의 Windows UI 수용은 후보 실행 전 NOT_RUN이다. 같은 테스트의 통과를 여러 번 합산하지 않는다.

| 작업 | 구현 commit | 원인·직접 영향 범위의 로컬 확인 | 남은 수용 |
|---|---|---|---|
| R00 | `302361bb` | 실제 입력 driver, matrix·identity·미실행 거절 validator 회귀 | Windows 입력/전체 gate 연결 |
| R01 | `600a9b06` | 문서 generation·Undo, journal/autosave/recovery·Quit·Startup, native journal 및 exporter | DOC-01–03 |
| R02 | `50131024`, `dfc56ecf` | Files 42, Workspace 초안/종료/전이 guard, native recovery metadata·files host·close admission | WORK-01/02 |
| R03 | `d1e87f6c` | registry projection·Agent 완료 후 갱신, typed app 검사 | WORK-03 |
| R04 | `2fcddb4d` | HTTP native 66 및 활성 필드·browser/cURL/codegen 직접 영향 회귀 | HTTP-01–03 |
| R05 | `6a8a29e5` | 실제 native 연결 grant snapshot, ProtocolLab 인증 잠금·불일치 거절 | AUTH-01/02 |
| R06 | `efa8c4a4`, `a3a1895d` | shell·Agent·Incoming·SuiteConnection; Chromium Notes 720×480 편집폭 468px/1180×780 704px, API 720×480 가로 잘림 없음 | 네 제품·전체 delivery 상태/keyboard/IME UI-01/02 |
| R07 | `50131024`, `c23f8218`, `a3a1895d`, `327e05af` | 재설치·업데이트·복원의 health 진입 회귀 RED→GREEN; setupOnly RED 2건 포함 RegistryGate; Control Center setup/Health/Restore 7, native delivery 검증 2, exporter·tsc | 실제 installer부터 INSTALL-01–03 |
| R08 | `3e0c60b1` | sparse/rename/delete·선택·저장 실패 UI 19 | ENV-01/02 |
| R09 | `d2221313` | Webhooks single-flight·active/hidden·읽기 회복 및 Studio 재진입 | WEB-01/02 |
| R10 | `5ff0dd7f` | 두 stream message 뒤 INTERNAL trailer 실제 tonic 회귀와 UI/history | GRPC-01 |
| R11 | `dbed4bdb` | full library 수정·삭제·용량 복원·지연 저장 UI/store | TRANSFORM-01 |
| R12 | `c978c3f2` | Search UI 30/API 3, content metadata 2/Notes metadata 1, native exporter | SEARCH-01/02 |
| R13 | `4345e80a`, `fd49b549` | idle sessionizer·실패 ack·DST 과거 기록, 날짜·privacy·owner UI, native tracking·exporter·tsc | ACTIVITY-01–03 |
| R14 | `5b314ca7`, `a6a41fe5` | Runtime recovery/Terminal/Dependencies/Files 영향 및 tsc; 응답 유실 뒤 실제 상태 조회로 실행 여부 조정, 부작용 중복 실행 차단 | RUNTIME-01/02, LSP-01, DEPS-01 |
| R15 | `efa4c861`, `8dc2b443`, `3ce46aac`, `8a51160d`, `33f031a3`, `f3be5eb1`, `df2fd78d` | 실제 입력·앱별 여정·handoff·설치 단계 관찰 runner 연결, exact-source/digest/전체 ID/스크린샷 gate; 로컬 runner 계약 검사 | 전체 packaged UI 여정은 후보 실행 전 NOT_RUN |
| R16 | `b2443b99`, `ee687389`, `eac4b572` | 현재 문서·닫힌 원장·철회 이력 정합성, 실제 철회본 7개 파일 digest, 업데이트·설치 실패 runner 준비 | PR CI→main CI→candidate→tag/release |

추가 구현 검토에서 설치 준비 중의 제품 연결과 Agent 재연결을 구분했다. 제품 버스는 준비 상태 확인에 필요하므로 복구 단계에도 명시적으로 연결할 수 있고, 일반 작업·Agent 재연결·수신 작업은 계속 차단한다. API의 좁은 창에서 숨겨지던 코드/OpenAPI/캡처 조작도 브라우저 측정으로 확인해 줄바꿈을 적용했다.

R15 여정 연결에서 추가 발견한 결함도 수정했다. Workspace 빠른 열기의 한글 조합 Enter 보호(`f58a7199`), API 버튼의 실제 접근성 이름(`733d5b0b`), 대상 ES 버전에서 지원하지 않는 Array.at 사용(`3070818a`), Windows 입력 tick 순환(`fd49b549`)을 직접 영향 범위로 확인했다. 설치 수용은 동일 설치 key와 후보 digest를 추적하며, v0.8.1에서 전환하는 별도 소유 fixture만 부모 설치 key를 명시하는 예외를 둔다. 예전 공개본의 성공을 새 후보에 승계하지 않는다.

기존 후보 artifact `11265063384`(run `37095144741`)의 철회본은 새 후보가 아니라 동일 버전 교체 검사의 고정 입력이다. manifest SHA-256 `27a3dd7b2dab585fa6a930d3a1e64bd4f1f3166a141c926080f005bc16114b4b` 및 7개 파일의 폐쇄 목록·크기·digest를 확인한다. 입력이 없거나 바뀌면 새 빌드로 대신하지 않는다. 별도 v0.8.1 전환과 실제 철회본→수정본 경로의 데이터 보존은 각각 구분해 기록한다.

최종 로컬 통합에서 기존 크기 상한을 넘은 컴포넌트는 앱 내부 helper로 분리했다(`726cfdf8`, `9762e3d0`, `cdd34186`). Knowledge의 초기 route 처리는 복구 가능한 지연 로딩으로 옮겨 초기 JS 278,731 bytes(상한 280,000)를 확인했다. 타입·테스트 계약 누락과 훅 의존성, Clippy 지적을 보완했고, 최초 전체 검사의 실패/미실행 범위만 이어서 확인했다. 상세 결과와 최종 source는 단일 PR 본문에 기록한다.
