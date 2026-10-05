# 감사·추가 개선·검증 추적표

## 현재 상태와 증거 원장

R00–R16 통합 구현은 [PR #616](https://github.com/jihoon22-lee/devbox/pull/616)에 반영했다.
출시 차단 보정은 후속 PR에 모으며 [#633](https://github.com/jihoon22-lee/devbox/pull/633)에서
최종 Astra 병렬 리뷰·수정·문서 정리를 묶었다. 이후 설치형 후보에서 확인된 결함은
최소 보정 PR로 이어간다. 아래는 출시 준비 커밋의 검증 기록이며,
**최종 공개 여부는 v0.9.0 Release와 연결된 최신 보정 PR의 결과를 따른다.**
소스·fixture·실제 실행·결과 집계·공개를 구분하고, 최종 source와 artifact는 PR 본문에 기록한다.

| 근거 | 실제 결과와 제한 |
|---|---|
| 제품 빌드 `5d25aa75`, 후보 `37306773490` | 7개 파일·네 native scope·WSL2 통과. portable 관측기 오류로 전체 수용 실패; 직접 승격 불가 |
| fixture `ab2ee78a`, 재검증 `37322897417` | 동일 설치 UI 실행 40행 PASS 및 owned cleanup 통과. migration의 generation update에서 `checkpoint_expired`; UI 성능 증거의 PNG·일부 성공 failureCode 누락도 집계 전에 발견. seal·게시 미완료 |
| 진단 `37325623409`, 원본 제품 bytes | 전체 migration 통과, 업데이트 준비 30.9초·34.0초. 소유 파일 약 1,081개·61MiB. 앞선 120초 초과의 정확한 runner 원인은 미확정. 진단은 승격 증거가 아님 |
| PR #633 제품 보정 | checkpoint 파일 작업 최대 2개 병렬, 전체 byte 상한 사전 예약. 기존 120초·취소·path/identity/hash·전체 보존·marker 조건 유지. 12개 직접 회귀와 최종 Control Center 64개 테스트 통과; 이후 Windows 결과는 다음 후보 행과 구분 |
| PR #633 패키징·검증 보정 | Tauri `--bins` 산출물인 helper의 중복 컴파일 제거, Product foundation PR 자동 실행 제거, 실제 성능 화면을 캡처하는 증거 생성 및 성공 행 계약 수정. 필수 CI·최종 후보 gate 유지 |
| 최종 전수 리뷰 | 최초 tracked 3,144개를 Workspace/Agent 1,327·API/Knowledge 1,017·공용/배포 709·문서 91개로 배정. 생성 파일·asset 검사와 깊은 소스 검토를 구분하고 추가 변경 파일도 최종 범위에 포함. 발견 결함과 최종 검증은 #633에 통합 기록 |
| 제품 빌드 `45443fd8`, 후보 `37344312789` | PR #633·exact-main CI 통과 후 새 7개 파일 assembly·네 native scope·WSL2 및 초기 설치/활성화 통과. 설치형 UI 여정 실패로 seal 거절, migration 미실행. 터미널 입력 순서 결함을 로컬에서 재현했으므로 fixture-only 재사용 대상이 아님 |

제품 코드·패키징 입력이 바뀌었으므로 `5d25aa75`를 새 제품의 PASS로 바꾸지 않는다.
새 exact-main 후보 수용과 동일 bytes 공개 확인을 완료해야 R16을 닫는다.
Control Center 로컬 영향 검사는 약 31초, peak 1.71GiB·swap 0이었다. 로컬 Clippy의
불필요한 참조를 수정한 뒤 실패·미실행 검사만 보충했고 통과한 check는 반복하지 않았다.
Windows 성능 배수나 전체 시간 절반 단축은 아직 측정·확정하지 않았다.

후보별 반복 진단의 원문은 [이전 기록](../../../history.md)과 #616/#617–#633 PR에 보존한다.
아래 감사 표의 경로·행 번호는 최초 감사 시점의 위치다. 현재 구현은 실제 코드·테스트와
각 제품 작업 문서를 따른다. 역사적 미실행·PASS를 현재 후보 상태로 합산하지 않는다.

## 1. 기존 감사 23건의 누락 없는 배정

| ID | 우선 | 재현 조건·문제 | 핵심 위치 | 작업 | 검증 ID·종료 조건 |
|---|---|---|---|---|---|
| S1 | P1 | import 배너가 추가되면 네 앱 본문 200px로 이동 | `packages/product-shell/src/index.tsx:93`, `styles.css:32` | R06 | UI-01: 모든 delivery 상태의 본문/primary 가시성 |
| W0 | P1 | setupOnly store selected 뒤 금지 registry snapshot→실패/재생성 반복 | `apps/devbox-workspace/src/RegistryGate.tsx:109` | R07 | INSTALL-01/02: selected에 snapshot/start_empty 재호출 없음 |
| W1 | P1 | Files edit→X/restart, recovery writer 미연결로 초안 소실 | `apps/devbox-workspace/src-tauri/src/component.rs:349`, `packages/workspace-features/src/files/App.tsx:1438` | R02 | WORK-01: 실제 편집→기록→종료/재열기, 실패 시 창 보존 |
| W2 | P1 | Source dirty→Agents 다른 worktree 검토, guard 우회 | `apps/devbox-workspace/src/Workspace.tsx:288` | R02 | WORK-02: native select 전 차단, 취소 후 초안/context 보존 |
| W3 | P2 | 새 agent worktree 등록 후 parent registry stale | `apps/devbox-workspace/src/agents/agentFlow.ts:87` | R03 | WORK-03: base-only 초기 상태에서 즉시 Source/Dependencies 일치 |
| AS-01 | P1 | JSON/raw→None 후 이전 body native 송신 | `crates/http-client-engine/src/commands/request.rs:1252` | R04 | HTTP-01: 실제 echo body=0, cURL/browser/native 일치 |
| AS-02 | P1 | query/form 예약 문자 비인코딩→의미 변조 | `crates/http-client-engine/src/commands/request.rs:3121` | R04 | HTTP-02: decode pairs/중복/순서/한글 보존 |
| AS-03 | P1 | Webhook 외부 POST 후 history 화면 미갱신 | `packages/api-studio-features/src/webhooks/App.tsx:313` | R09 | WEB-01/02: idle arrival/route 복귀/실패 후 읽기 회복 |
| AS-04 | P1 | var2 하나 import→+변수→기존 값 삭제 | `packages/api-studio-features/src/requests/components/RequestSidebar.tsx:521` | R08 | ENV-01: sparse/secret 충돌 시 기존 값 보존 |
| AS-05 | P2 | 환경 변수 이름 입력/수정·개별 삭제 불가 | 같은 RequestSidebar.tsx:456 | R08 | ENV-01: baseUrl/token을 UI로 만들고 요청까지 사용 |
| AS-06 | P2 | 새 환경 생성 뒤 첫 기존 환경 선택 | `packages/api-studio-features/src/requests/hooks/useEnvironmentPersistence.ts:149` | R08 | ENV-01: 생성 id 선택/실패 시 이전 선택 유지 |
| AS-07 | P2 | auth 변경 뒤 비활성 field 변수 때문에 send 차단 | `packages/api-studio-features/src/requests/lib/runner.ts:191` | R04 | HTTP-01/03: 활성 field만 검사·resolve |
| AX-01 | P1 | MCP 연결 A에서 B/None 선택, 호출은 A 자격 | `packages/api-studio-features/src/requests/ProtocolLab.tsx:989`, native `mcp.rs:560` | R05 | AUTH-01/02: active grant와 UI 일치, mutation 1회 |
| AX-02 | P2 | pipeline 편집이 신규 id 생성, 20개 포화 후 저장 회복 불가 | `packages/api-studio-features/src/transforms/workflows/SmartWorkflowPanel.tsx:259` | R11 | TRANSFORM-01: full library 편집/삭제/새 저장/재열기 |
| AX-03 | P2 | gRPC 2개 message 뒤 error→응답0으로 소실 | `crates/http-client-engine/src/commands/grpc.rs:1003` | R10 | GRPC-01: 부분2개+terminal error와 history 일치 |
| K1 | P1 | 노트 A→B→Undo→B에 A 내용 autosave | `packages/knowledge-features/src/notes/components/MarkdownEditor.tsx:220` | R01 | DOC-01: 실제 B disk에 A sentinel 없음 |
| K2 | P2 | 반복 observe 뒤 idle 종료, 마지막 입력 후298초 집계 | `crates/activity-engine/src/core/sessionizer.rs:55` | R13 | ACTIVITY-01: idle-tail 구간 제외, pause/clock 경계 보존 |
| K3 | P2 | regex 기호/한글 제거한 FTS가 정상 후보 제외 | `packages/knowledge-features/src/search/App.tsx:453` | R12 | SEARCH-01: candidate→worker 통합 정확성·한도 partial |
| K4 | P2 | Settings의 history 재생성 항상 disabled/현재 digest 오용 | `packages/knowledge-features/src/activity/App.tsx:590` | R13 | ACTIVITY-02: 과거 entry 기간/timezone으로 재생성 |
| K5 | P2 | 원본 삭제 뒤 journal 열기 실패, UI 복원/본문 확인 불가 | `packages/knowledge-features/src/notes/components/RecoveryControls.tsx:92` | R01 | DOC-02: 원본 없이 preview/copy, 명시 CAS 복구 |
| K6 | P2 | installed QuitGuard가 실제로 유지되는 수집을 중지한다고 안내 | `apps/devbox-knowledge/src/QuitGuard.tsx:97` | R13 | ACTIVITY-03/AGENT-04: owner별 문구와 실제 process 일치 |
| K7 | P2 | 최소720×480 기본 노트 편집폭33px | `packages/knowledge-features/src/notes/App.css:35/354`, Knowledge tauri.conf.json:20 | R06 | UI-01/02: 최소 창 편집폭≥320px, toolbar 도달 |
| C1 | P1 | products 화면 recordSuiteHealth가 route admission에서 거절 | `apps/devbox-control-center/src/Health.tsx:76`, native `ipc/delivery.rs:131` | R07 | INSTALL-01: 실제 화면 route=header, 다음 단계 완료 |

원 감사 근거 구분: S1/K1/K7 브라우저 UI 재현; W0 실제 권한을 반영한 RED 2개; AS-01/02 순수 Rust 분기 probe; AS-04/06/07 TypeScript 순수 함수 probe; K2 실제 sessionizer 실행; 나머지는 생산 코드 연결 확인. 이번 계획 작성에서 이들 시험을 다시 돌리지 않았다.

## 2. 계획 단계에서 추가 확인한 코드 결함

| ID | 분류/근거 | 문제 | 작업 | 종료 조건 |
|---|---|---|---|---|
| N01 | P2 · 코드 확인 | `IncomingCommands.tsx`/`SuiteConnection.tsx`의 refresh 성공이 이전 조회 issue를 해제하지 않아 복구 후에도 실패 표시가 남음. write issue와 섞지 않고 해제 필요 | R06 | 읽기 실패→최신 성공 후 해당 경고 소멸, late failure 무시; unrelated write 실패는 유지 |
| N02 | P2 · 코드 확인 | Requests App의 warning이 HTTP 전용 조건 안에 있어 Protocols에서 환경 save 실패 이유가 숨겨짐 | R08 | ENV-02: Protocols/History/Requests 모두 오류 표시와 write lock 설명 |
| N03 | P2 · 코드 확인 | Activity shift는 day/week/month만 변경; timeline 화살표는 날짜를 바꾸지 않음 | R13 | ACTIVITY-02: timeline 날짜±1일·native 기간 이동 |
| N04 | P2 · 코드 확인 | Activity idle threshold의 UI void 호출/native set_setting 오류 무시→거짓 성공 | R13 | ACTIVITY-03: read-only DB 오류 전달, 이전 ack값 보존, 재시작 readback |

N01 근거: `packages/product-shell/src/IncomingCommands.tsx` refresh.then은 setReviews만, SuiteConnection.tsx refresh.then은 setStatus만 호출한다. 각 catch는 setIssue를 호출한다. 성공적 refresh가 error를 해제하는 경로가 없다.

N02 근거: `packages/api-studio-features/src/requests/App.tsx:1532`의 `workspace !== 'protocol'` 조건 내부에 persistenceWarning. sidebar의 환경 UI와 persistenceReady=false 잠금은 protocol에서도 사용된다.

N03/N04 근거: `packages/knowledge-features/src/activity/App.tsx:757`, `components/ActivitySettings.tsx:208`, `crates/activity-engine/src/commands/tracking.rs:313`, `core/db.rs:249`. DB에는 이미 fallible `try_set_setting()`이 있으므로 이를 활용한다.

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

아래는 통합 PR #616을 만들던 시점의 작업별 구현·로컬 증거다. 표의 남은 수용은 그 당시 상태이며, 최신 후보·게시 상태는 이 문서 첫 부분과 PR 본문에서 구분한다. 같은 테스트의 통과를 여러 번 합산하지 않는다.

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

## 6. 최종 Astra 병렬 리뷰 보정

최초 파일 목록 확인 뒤, 구조 검사만 된 제품 구현을 다시 분리해 실제 제어·데이터 흐름을
읽었다. 세 Astra 리뷰어와 주 작업자가 제품·공용 모듈·native 경계·스크립트·workflow를
분담했으며, generated 선언·반복 fixture·asset 형식 검사와 실제 구현 리뷰를 구분한다.
아래 직접 회귀는 로컬 결과이며 새 Windows 배포본의 수용 근거가 아니다.

| 범위 | 확인된 문제와 보정 | 직접 확인 |
|---|---|---|
| Files·편집 | 저장 후 복구 정리 중 새 편집 보존, 정리 실패 표시, 이름 변경·탐색 시 같은 버퍼 선택, clipboard 읽기 전용 전환 보호, LSP 재시작 mirror 직렬화·재생 기한, 정확한 file handle·읽기 상한 | 비동기 지연·거절·재열기·back/forward, LSP replay 4·file 22 |
| Source | stash의 실제 commit 재확인, diff의 `---`/`+++` 본문 처리, unborn branch 이름, detached worktree 파싱, stage 취소 시 commit 초안 보존 | Git 임시 저장소·parser·StageCommitPanel 18 |
| Runtime·Tasks | 서비스 stale 복구 누락 및 실행 전 중단된 claim 정리, 종료 확인 실패 시 차단 유지, 빈 argv 보존, 서비스 상태 갱신·완료 로그 끝까지 조회·안전한 오류 문구 | stale 6·workspace-task 15 및 Tasks UI 회귀 |
| Terminal·Logs·WSL | 여러 pane 동시 종료 시 빈 탭·pane 잔존 방지, 설치형 Logs 선택을 실제 Agent owner로 조회, 비드라이브 mount의 프로젝트 identity 보존 | Terminal UI, 설치 route 계약, WSL path 17 |
| 파일·실행 경계 | shared/AppLink/Logs/Projects의 FIFO 대기와 비정상 파일 거절, 신뢰 실행 파일의 전체 root prefix 확인, LSP stderr 비밀 값 가림 | synthetic FIFO/device·path prefix·redaction 회귀 |
| API Studio | regex assertion 시간 제한, MCP own-property 경계, 참조와 literal이 섞인 credential 가림, UTF-16 pair 검증, 늦은 hash 결과 무시, diff 행·clipboard 실패 처리 | MCP 36·native request 67·Transforms 72 및 copy 12 |
| Knowledge | CRLF frontmatter, 실패한 index transaction rollback, 암호화 XLS 분류, Activity export privacy·실제 일치 alias, 새 template의 현재 날짜 | frontmatter 7·index 19·XLS 17·export 33·attribution 6·template 4 |
| CI·패키징 | lockfile 변경 시 전체 소비자 범위, 선택 진단 자동 실행 제거, 중복 helper/LSP fixture 빌드 제거, 실제 성능 PNG와 결과 계약 | scope·package·foundation·layout 계약 회귀 |
| 문서 | 폐기·대체된 50개 문서를 고정 Git 이력 색인으로 통합, 현재 계획·ADR·제품 안내·출시 증거 원장 연결 | 로컬 링크와 삭제된 경로 참조 대조 |

수정 뒤 통합 검사·필수 PR CI·새 exact-main 후보·같은 bytes의 공개 다운로드 확인은
별도 결과로 기록한다. 코드 리뷰와 좁은 회귀 통과만으로 R16이나 재출시를 완료 처리하지 않는다.

로컬 최종 통합에서는 전체 프런트 빌드·번들·타입 검사와 2,387개 테스트, Rust workspace
check·Clippy·fmt와 2,947개 테스트 및 생성 타입 일치를 확인했다. 컴포넌트 크기·ES 대상
호환성·포맷 실패를 수정한 뒤 실패·미실행 단계만 이어갔다. 마지막 Logs 호환성 보정은
필터 9개와 영향받은 Workspace 타입·빌드·번들만 확인했다. Rust harness의 ignored 3개 중
하나는 통과한 부모 테스트가 실행하는 subprocess fixture이고, 나머지 Windows→WSL 및
설치된 LSP 수용은 새 후보에서 별도로 확인한다. CI·공개 결과는 #633과 Release 원장에 남긴다.

## 7. 후보 `37344312789` 이후 직접 보정

세 Astra 리뷰어가 설치형 실패의 최초 원인과 후속 오류를 분리하고 수정 범위를 서로
교차 검토했다. 전체 감사를 다시 실행하지 않고 직접 회귀와 영향받은 타입·빌드·Rust
검사를 수행한다. 다음 로컬 재현은 새 Windows 설치본의 PASS를 뜻하지 않는다.

| 경계 | 확인·보정 | 검증과 한계 |
|---|---|---|
| Terminal | 비동기 PTY 쓰기의 완료 전에 다음 Enter가 먼저 도달. pane별 제한된 대기열로 실제 쓰기·flush ACK 순서를 보존하고 종료·실패 시 대기 입력 폐기, 재전송 금지 | 지연된 첫 쓰기와 Enter로 RED→GREEN. 초기 명령·broadcast 대상 변경·실패·상한·pane 독립성 포함 |
| Knowledge 시작·Health | 기존 제품 DB에 중단된 rollback journal이 있으면 읽기 전용 metadata 연결이 복구를 거절. 기존 선택 파일만 READ_WRITE로 열어 SQLite 복구를 허용하고 query_only·trusted_schema 제한 유지 | 실제 subprocess 중단 journal로 binding 및 Notes/Activity/Search 준비 실패를 재현. DB 생성·row/schema 변경은 금지. 원본 후보에는 journal 근거가 없어 동일 원인이라고 단정하지 않음 |
| checkpoint | 제품 writer 종료보다 Windows 파일 handle 해제가 늦을 수 있음. 개별 open/read의 OS 32·33만 원래 120초·취소 범위에서 기다리며 나머지 오류는 즉시 거절 | byte/hash/identity·전체 보존 조건 유지, 완료된 read 재실행 없음. 실제 Windows exclusive handle 회귀는 #634 Windows CI에서 통과했으며 로컬 WSL에서는 실행하지 않음. 원본 후보의 정확한 파일·OS 원인은 미확정 |
| 설치 관측 | 종료 dialog 안의 경고를 명시적으로 선택, Knowledge 재시작 뒤 새 CDP로 성능 PNG 캡처, shortcut의 정확한 WebView 자식 종료까지 관측 | WORK 경고 중복과 이전 CDP 재사용은 fixture 결함으로 재현. 후속 LSP/Dependencies의 가림은 앞선 종료 dialog에서 발생 |
| 실패 기록 | 미완료 activation의 Agent 여정은 NOT_RUN으로 기록하고 전체 gate는 실패 유지. 시작 시 Agent가 없던 경우 X가 종료시켰다고 표시하지 않음 | 최초 delivery 실패를 보존하며 성공을 만들기 위한 재시작·상태 변경은 하지 않음 |

이 보정에는 제품 변경이 포함되므로 `45443fd8`의 자산을 승격하거나 새 source로
재표기하지 않는다. 같은 보정 PR에 로컬 결과와 필수 CI를 기록한 뒤 새 exact-main
후보의 설치·복구·전체 여정과 공개본 다운로드를 확인한다.

`e5c8bd19`의 새 후보를 빌드하는 동안 후속 관측기 3곳도 보완했다. Activity 중지는
한 번의 클릭 뒤 native tracking·consent 해제를 기다리고, 첫 HANDOFF 미리보기는
레이블이 아니라 전달할 원문과 정확히 비교한다. delivery 정리 실패는 최초 실패를
덮지 않고 결과에 추가하며, 성공 뒤 정리 실패도 FAIL로 남긴다. 모두 fixture 변경이며
제품·의존성·빌드 입력에는 차이가 없다. 기존 빌드·assembly·native·WSL2 성공이
모두 확보된 뒤 이전 관측을 중단하고 수정된 40개 UI·migration을 같은 7개 파일로
수용하는 경우 [제한적 재검증 정책](../../../release-policy.md#fixture만-바뀐-후보의-제한적-재검증)을 따른다.
어느 필수 독립 job이라도 완료·성공하지 않았으면 재사용하지 않는다.
