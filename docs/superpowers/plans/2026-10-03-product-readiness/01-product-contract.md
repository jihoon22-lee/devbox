# Devbox 재출시 제품 계약

**상태:** 2026-10-03 사용자 요청에 따른 구현 전 계획. 현재 기능을 안정적으로 완성하는 범위다. 이 문서 작성은 구현·머지·배포 완료를 뜻하지 않는다.

**Goal:** 새 사용자가 설치부터 첫 작업까지 안내만 따라 완료하고, 기존 사용자는 데이터와 실행 중인 작업을 보존하면서 네 제품을 반복 사용할 수 있게 한다.

**Architecture:** Windows 11·Tauri v2·React 19·TypeScript·Rust 구조와 네 제품/Agent 소유권을 유지한다. renderer는 화면·편집 초안을, native는 권한·설치 journal·저장 revision·프로세스와 외부 송신을 소유한다. 상태 모델과 검증은 실제 사용 흐름을 따라 연결하되 권한 경계를 합치지 않는다.

**Tech Stack:** pnpm 9, Vite, 순수 CSS, `packages/tokens`, CodeMirror, 기존 Cargo/typed IPC 및 Windows WebView2/CDP/UI Automation.

**Spec inputs:** 사용자 지시(감사 후 개선 계획), [23건 추적표](08-traceability.md), [원 감사 보고서](/home/jihoon/projects/devbox-audit-2026-10-03/report.md), AGENTS.md·CONVENTIONS.md. 로컬 감사 파일이 없는 환경에서도 08의 재현 조건과 코드 위치로 과제를 수행할 수 있어야 한다.

## 선택한 접근

| 접근 | 이점 | 남는 문제/판단 |
|---|---|---|
| 23건만 각각 패치 | 변경량이 작음 | 첫 설치 조합·상태 전이·사용자 관점의 검증 누락이 남음 |
| **현재 구조 안에서 결함과 연결된 사용 흐름을 함께 완성** | 데이터/권한 경계를 유지하며 문제를 재현 가능한 단위로 해결 | 이 계획의 선택. 17개 작업 묶음으로 위험 경계를 나누고 통합 브랜치의 최종 PR 1개로 제출 |
| 앱/설치 구조 전면 재작성 | 자유로운 재설계 | 저장소 migration과 배포 경로를 다시 위험에 노출. 이번 범위에서 제외 |

## Global Constraints

- 대상은 Windows 11, Tauri v2·React 19·TypeScript·Rust, 패키지 매니저는 **pnpm 9**다.
- UI는 순수 CSS와 기존 `packages/tokens`를 사용한다. 새 UI 프레임워크·상태관리 라이브러리를 기본 전제로 추가하지 않는다.
- WSL Rust 실행 전 `source ~/.cargo/env`; 실제 앱 실행·배포 빌드는 Windows에서만 한다.
- 기존 네 제품/identifier/설치 namespace를 유지한다. 순수 로직은 core, OS 처리는 command/platform에 둔다.
- 실제 두 번째 소비자 이전에는 새 공용 crate/package를 만들지 않는다. 이번 계획의 상태 도우미는 소유 제품 내부에 둔다.
- 사용자 데이터·secret을 fixture로 쓰지 않는다. migration은 원본 보존·WAL consistent snapshot·destination namespace·재개/복구 경계를 지킨다.
- UI route 이동은 권한 부여가 아니다. 테스트나 UI 편의를 위해 native admission을 전역 완화하지 않는다.
- 로컬 기존 서비스·Docker·방화벽·공유 네트워크를 변경하지 않는다. 해당 검사는 일회성 hosted runner/검증된 독립 VM에서만 수행한다.
- 결함별 최소 회귀와 필요한 로컬 검사를 우선한다. 모든 작업·문서를 통합한 뒤 최종 검증과 PR 1개를 진행한다. 작업마다 전체 검증·CI를 반복하지 않으며 메모리·디스크·에이전트 제한은 00 §4–5를 따른다. 재출시 대상은 v0.9.0이다.
- closed #580을 다시 열거나 새 실기 요청 이슈를 만들지 않는다. 기록은 계획의 추적표와 PR 본문, Actions artifact에 남긴다.
- 제품 검증을 사용자에게 맡기는 항목을 완료 조건으로 두지 않는다. 환경 제약은 담당자가 해결하거나 미실행으로 남기며 그 상태에서 공개하지 않는다.

## Review Focus

1. 편집 도중 다른 문서/프로젝트/앱/프로세스로 전환되어도 미저장 내용과 대상 identity가 혼동되지 않는다. R01/R02/R03의 전환·종료·crash 시험으로 고정한다.
2. UI가 비활성으로 표시한 본문/인증은 실제로 송신되지 않고, 선택한 인증·URL/form 값이 수신 결과와 일치한다. R04/R05/R08/R10의 loopback 시험으로 고정한다.
3. 정상·오류·취소·연결 끊김·재시작을 거쳐도 표시가 실제 상태로 복귀하고, 불확실한 mutation을 자동 재전송하지 않는다. R07/R09/R14/R15에서 고정한다.
4. 빈 데이터와 복원/삭제/외부 변경이 있어도 사용자가 실패 원인을 이해하고 안전한 다음 행동을 할 수 있다. R01/R07/R11/R12/R13에서 고정한다.
5. 최소 지원 창·확대 배율·키보드·한국어 IME에서도 핵심 행동이 보이고 작동한다. R06의 전 제품 매트릭스와 각 기능의 Windows 수용에서 고정한다.

## 1. 설치·활성화 계약

- 신규 설치 끝에서 Control Center의 설치 안내로 이어진다. 선택한 설치의 제품만 다룬다.
- 사용자에게 보여줄 단계는 **저장소 준비 → 준비 검토 → 활성화 → 실행 확인 → 사용 준비 완료**다. 각 단계에는 현재 상태, 필요한 작업, 실패 이유, 다음 행동이 있다.
- native journal/manifest/owner evidence가 원장이다. localStorage나 renderer가 별도 설치 진행률/성공 여부를 영속화하지 않는다.
- 제품별 저장소 선택이 필요한 경우 해당 제품으로 안전하게 이동하고 완료 후 Control Center에 반영한다. 이미 준비된 store에 재생성 요청을 보내지 않는다.
- 상태 기록 성공 직후 상위 설치 목록을 다시 읽는다. 사용자가 다른 ‘새로고침’ 버튼을 찾아야 다음 단계로 넘어가는 흐름을 없앤다.
- import/health 중에는 비허용 일반 기능을 실행하지 않는다. 자동 조회·연결 재시도도 availability를 따른다. 대기 상태를 연결 실패라고 표시하지 않는다.
- helper 전환은 기존 exact identity/정상 종료/receipt 확인을 유지한다. 다른 제품에 미저장 자료가 있으면 닫기를 취소하고 원래 화면을 유지한다. 강제 종료로 설치를 진행하지 않는다.
- 중단·재실행은 journal에서 이어간다. 활성화 완료 전 ‘완료’ 표시나 일반 작업 허용은 없다. 단계 실패 뒤 재시도는 동일 operation receipt를 확인하고 중복 실행을 피한다.
- installer interactive UI 경로와 silent/native 경계 검사를 각각 유지한다. 하나의 성공으로 다른 쪽을 대체하지 않는다.

## 2. UI/UX 계약

- 네 앱은 공통 navigation, page heading, primary action, 상태/오류 표시, 빈 상태, 작업 결과, 취소·재시도, focus 복귀 규칙을 공유한다. 기능별 상세 화면은 해당 앱이 소유한다.
- 모든 `tauri.conf.json`의 현 최소 720×480, 900×640, 기본 1180×780, 1440×900에서 실제 route를 검사한다. 최소 크기를 올려 결함을 숨기지 않는다.
- 100/125/150/200% Windows 배율과 200% 텍스트 확대를 구분해 기록한다. 화면 크기 전체 조합 대신 최소·기본 창과 대표 편집/표/모달에 pairwise 적용한다.
- 좁은 창에서는 부가 sidebar/backlink/detail panel을 접거나 drawer로 옮긴다. 열린 문서·dirty state·keyboard focus는 보존한다. 본문을 33px로 압축하거나 control을 잘라내지 않는다.
- 편집기 작업 폭은 720px 창에서 최소 320 CSS px를 목표로 한다. 이 값과 control 가시성은 R06의 신규 수용 기준이며 현재 통과값이 아니다.
- 표는 필요한 경우 명확한 내부 가로 스크롤을 제공한다. 페이지 전체 overflow로 버튼이 화면 밖에 사라지게 하지 않는다.
- error는 현재 operation과 연결하고 성공적인 최신 refresh가 해당 조회 오류를 지운다. 별개의 write 실패 경고를 조회 성공이 지우지 않도록 분리한다.
- loading/empty/disconnected/unsupported/permission-denied는 구분한다. disabled control에는 원인과 해결 행동이 있다. 실제 동작을 할 수 없는 mock 값을 성공으로 보이지 않게 한다.
- 키보드 Tab/Shift+Tab, modal focus trap·복귀, Escape, visible focus, IME 조합 중 Enter/Ctrl 단축키, forced colors/reduced motion을 검사한다.
- 화면 문구는 작업·대상·결과 중심이다. 내부 route/journal/generation 같은 용어는 진단 세부정보에만 둔다.

## 3. 문서·초안·복구 계약

- 문서 identity는 저장소/프로젝트 context, 상대 경로, native revision, 열기 세대를 함께 사용한다. 단순 현재 path만으로 늦은 응답을 적용하지 않는다.
- 노트 전환은 이전 문서의 Undo가 새 문서로 넘어가지 않는다. 같은 문서의 정상 편집 Undo는 유지한다.
- Files는 dirty 본문·encoding을 revision/context와 함께 recovery에 기록한다. 정상 종료는 save/discard/cancel 검토 후, 선택한 작업이 성공했을 때만 진행한다.
- crash 복구는 마지막 성공 기록의 시점을 표시한다. 기록 실패를 ‘저장됨’으로 표시하지 않는다. 용량 초과·권한 거절·중단된 쓰기는 초안을 메모리에서 제거하지 않는다.
- 외부 수정/삭제·revision 충돌은 명시적으로 비교한다. 삭제된 원본의 저널도 읽기/복사/다른 이름 저장/검토 후 재생성이 가능하다.
- 사용자가 명시적으로 버린 저널의 삭제 범위와 자동 보존 범위를 정의한다. 버리기 실패 시 성공으로 종료하지 않는다. 원본 파일·다른 문서의 복구 자료는 건드리지 않는다.
- AgentHub/Registry/외부 handoff의 context 전환은 하나의 guard를 거친다. cancel은 native context 변경과 화면 해제를 모두 막는다.

## 4. 요청·인증·외부 작업 계약

- 편집용 draft를 유지해도 송신 모델에는 활성 kind의 본문/auth만 들어간다. missing-variable 검사·native 실행·cURL/code preview의 의미를 일치시킨다.
- query/form은 예약 문자·Unicode·중복 키·순서를 보존한다. 최종 URI fragment는 서버에 보내지 않으며 기존 query와 올바르게 병합한다.
- 환경 생성/변수 추가는 기존 값과 secret 속성을 파괴하지 않는다. rename/delete는 명시적이고 이름 충돌을 안내한다. 저장 실패는 모든 관련 route에서 보인다.
- MCP 연결에 적용된 인증과 다음 연결용 설정을 분리한다. 연결 중 인증 변경 시 도구 실행을 막고 재연결해야 한다. 자동 tool mutation 재전송은 없다.
- gRPC는 이미 받은 부분 메시지와 마지막 오류 상태를 함께 표시·보존한다. 크기 제한과 redaction은 유지한다.
- Webhook은 외부 도착을 사용자가 다른 버튼을 누르지 않아도 볼 수 있다. 비활성 화면은 polling을 줄이고 재진입 시 갱신한다. 이벤트 폭주·늦은 응답을 제어한다.
- Tasks/세션/프로토콜은 ‘실패’와 ‘결과 확인 필요’를 구분한다. 기존 operation ID/receipt로 조회한 뒤 사용자가 재실행을 결정한다.

## 5. 검색·활동·백그라운드 계약

- 정규식 후보 검색은 유효한 결과를 조용히 누락하지 않는다. 한도에 도달하면 ‘전체 결과’로 표현하지 않는다. invalid regex와 0건을 구분한다.
- 검색 source/모드/저장 query를 복원할 때 서로 다른 검색 의미를 섞지 않는다. index 준비/중단/권한 실패/0건 상태와 복구 행동을 제공한다.
- idle 제외는 UI 설정과 실제 집계가 일치한다. 임계값 도달 이전 observe 기록을 포함하는 통합 시험을 둔다.
- 과거 handoff 재생성은 그 기록의 기간/timezone으로 새 digest를 만든다. 현재 보고 있던 기간을 잘못 쓰지 않는다.
- installed UI 종료와 Agent 수집 종료는 다르다. 동의·일시정지·자동 시작·UI 종료·tray 종료의 의미를 같은 문구/동작으로 제공한다.
- Agent crash 재연결, explicit stop latch, cold launch, update shutdown, installed owner 유지, portable owner 종료를 실제 프로세스 경계에서 검증한다.

## 6. 범위 밖

새 제품, 새 클라우드 동기화/계정 시스템, 전체 IDE·DB GUI, 기능 수 확대, 새 스킨/디자인 시스템 교체, 근거 없는 성능 전면 최적화, 저장소 포맷 전면 변경은 제외한다. 기존 계약을 고치는 데 필요한 작은 타입/상태/모듈 분리는 각 작업에 포함한다.

추가 문제는 `확정 결함`, `개선 제안`, `실기 미검증`으로 구별하여 추적한다. 이 계획의 완료를 코드 전수 안전 보증으로 표현하지 않는다.
