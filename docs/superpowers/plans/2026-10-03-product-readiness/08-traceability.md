# 감사·추가 개선·검증 추적표

2026-10-03 계획 기준. 모든 행의 현재 상태는 **계획됨 / 구현 전**이다. 원 감사의 재현 결과는 결함 근거이며 아래 검사의 PASS가 아니다. 완료할 때 각 행에 `단일 PR URL / 작업 ID / 최종 commit / 테스트 이름 / scenario run / 결과`를 추가한다. 미실행과 실패는 구분한다.

코드 경로는 현재 `/home/jihoon/projects/devbox` 기준이다. 다른 checkout에서는 저장소 상대 위치를 그대로 사용한다. 상세 변경 파일과 assertion은 소유 작업 문서에 있다.

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
