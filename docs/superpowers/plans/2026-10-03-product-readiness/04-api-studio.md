# API Studio 후속 작업계획 — 구현 전 검토안

> **For agentic workers:** [00 실행 규칙](00-roadmap.md)·[01 제품 계약](01-product-contract.md)·[06 수용 매트릭스](06-acceptance-release.md)를 함께 적용한다. 선행/파일 배정 충돌 시 00의 통합 표가 우선한다.

**Goal:** API Studio의 송신·인증·환경·수신·변환 의미를 실제 사용자 행동과 일치시킨다.

**Architecture:** 기존 제품/native 소유권·revision·typed IPC를 유지하고 실제 UI 전이를 검증한다. 신규 제안 파일/테스트는 아래 과제에서 생성한다.

**Tech Stack:** 현 Rust·React 19·TypeScript·pnpm 9·CSS·Windows WebView2.

**Spec:** 01. **Global Constraints / Review Focus:** 01의 전체 제약과 5개 검토 관점을 상속한다.

**과제 실행 순서:** 각 하위 과제에서 (1) 명시한 assertion의 RED 작성, (2) 실제 실패 원인 확인, (3) 아래 계약에 맞춘 최소 구현, (4) 동일 좁은 검사 GREEN, (5) 과제 커밋. 명령은 실행 계획이며 현재 PASS 기록이 아니다. 직접 pnpm/cargo 명령은 반드시 `python3 .github/scripts/verify-resources.py --` 아래에서 실행한다. 통합 완료 후 최종 검증은 00을 따른다. 작업마다 전체 검증·CI를 반복하지 않는다.


작성일: 2026-10-03. 기준 작업 트리: `/home/jihoon/projects/devbox`, `fix/suite/first-run-activation`.
이번 계획 작성에서는 제품·테스트 파일 수정, 테스트·컴파일 실행, 서비스 조작을 하지 않았다. 계획 문서만 작성했다. 기존 감사는 [API 감사](/home/jihoon/projects/devbox-audit-2026-10-03/devbox-audit-api-studio.md), [추가 감사](/home/jihoon/projects/devbox-audit-2026-10-03/devbox-audit-api-extra.md)를 사용했다. 아래 assertion과 명령은 **향후 구현할 회귀 검사 명세이며 실행 결과가 아니다**.

## 범위와 작업 배분

원래 네 사용자 흐름 묶음을 통합 계획의 위험·rollback 경계에 따라 다음 여섯 작업 묶음에 배분한다. 기존 감사 10건을 빠짐없이 대응한다.

| 작업 | 사용자에게 달라지는 동작 | 기존 감사 | 추가 범위 |
|---|---|---|---|
| R04 | 선택한 Body/Auth와 실제 전송을 일치시키고 query/form 값을 보존 | AS01, AS02, AS07 | 전송·취소·이전 응답 상태와 요청 행 접근성 |
| R05 | MCP 연결에 적용된 인증을 정확하게 표시·고정 | AX01 | 인증 변경·실패 후 재연결 안내 |
| R08 | 환경을 만들고 이름 있는 변수를 안전하게 추가·수정·삭제 | AS04, AS05, AS06 | 신규 N02 및 환경 빈상태·저장실패 안내 |
| R09 | Webhook 외부 수신을 화면에 반영하고 실패 후 다시 읽기 | AS03 | 수신/정지/조회실패 구별, 선택·편집 유지 |
| R10 | gRPC 오류 종료 이전에 수신한 메시지를 보존 | AX03 | 부분 응답과 종료 상태를 함께 표시 |
| R11 | 저장된 Transform 파이프라인 수정과 용량 회복 가능 | AX02 | 저장 대기·완료·실패, 빈상태·단계 접근성 |

선행은 00의 의존 표를 따른다. **R08은 R04/R05/R06 변경의 통합 후 시작**한다. R04와 R08 모두 `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/App.tsx`와 새 실행 흐름 회귀 파일을 만지기 때문이다. R05/R09/R10/R11은 R04와 별도 파일을 중심으로 진행할 수 있다. R05의 `ProtocolLab.tsx`와 R10의 `GrpcLab.tsx`는 별개다. R11 handoff 검증이 R04 Requests 회귀에 의존하게 되는 경우 해당 통합 검사만 R04 수용 후 실시한다. 공통 CSS/README 수정은 소유 작업에 한정해 불필요한 겹침을 만들지 않는다. 단순 동일 패키지라는 이유로 모든 작업에 기능 선행을 만들지는 않는다.

각 작업 내부는 아래 과제를 실패 회귀 → 수정 → 직접 영향 검사 → 과제 커밋 순으로 수행한다. 전체 상세 검증·push/CI는 모든 작업과 문서를 통합한 최종 PR 1개에서만 수행한다. 작업 커밋을 되돌릴 때 데이터 스키마/문서 namespace 변경이 없도록 기존 포맷과 native dispatch를 유지한다. Suite 첫 실행/route admission의 별도 수정은 feature 코드 선행이 아니라 **Windows 통합 수용을 가능하게 하는 선행 조건**으로 다룬다.

## 추가 확인 결함과 개선 제안의 구분

### N02 — Protocols에서 환경 저장 실패 안내가 숨겨진다 [P2, 코드 확인]

- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/App.tsx:1532`의 `workspace !== "protocol" && section !== "history"` 조건 안에 `migrationNotice`와 `persistenceWarning`(1534–1535)이 있다.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/components/RequestSidebar.tsx:139`는 History에서만 sidebar를 숨긴다. Protocols에서도 환경 편집을 사용할 수 있다.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/hooks/useEnvironmentPersistence.ts:69`–72는 native save 실패 시 `persistenceReady=false`로 잠그고, 93–96은 경고를 설정한다.
- 트리거: Protocols에서 환경 값 수정 → native document save 거절(충돌/IO failure) → 환경 controls는 잠기지만 이유가 표시되지 않는다. Requests로 이동해야 경고가 나온다. 코드상 경로 확인이며 실제 native 실패 실험은 하지 않았다.
- R08에서 공통 저장/마이그레이션 안내를 route 독립 영역으로 옮긴다. 쓰기 잠금은 유지한다. 저장 실패를 정상 준비 중으로 표현하지 않고 재시작/재읽기 등 지원되는 복구 경로를 안내한다.

아래 U1–U6은 별도 고장 재현을 주장하는 결함이 아니라 **확인한 코드에서 도출한 현재 기능 완결 제안**이다. 10건의 필수 수정과 함께 작은 범위에서 시행할 수 있으며, 통과 여부는 명시한 사용자 동작으로 판단한다.

- **U1/R04 요청 접근성:** `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/components/KeyValueEditor.tsx:19`–32는 key/value placeholder와 이름 없는 `✕`를 사용한다. 행별 key/value 레이블과 삭제 대상 이름, 추가·삭제 후 포커스를 제공한다. `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/App.tsx:1543` 부근 URL도 명시적 accessible name을 제공한다.
- **U2/R04 전송 상태:** 같은 App의 786–791은 sending을 켜면서 이전 응답을 유지하고, 866–872는 cancel 시 이전 응답을 유지한다. 이전 응답인지 현재 전송 결과인지 표시하고 전송/취소 상태를 짧게 알린다. 이 상태는 서버 작업 rollback을 보증하지 않는다. 재시도는 사용자가 명시적으로 보내기를 누르는 기존 동작을 유지한다. ResponseViewer의 기존 tablist·화살표/Home/End 접근성은 이미 구현되어 있어 다시 만들지 않는다.
- **U3/R05 인증 상태:** `ProtocolLab.tsx:1007`–1017의 선택 grant와 실제 연결 grant를 혼동하지 않게 한다. 연결 중 편집 잠금 + 적용 grant 표시가 가장 작은 수정이다. 인증 실패·grant 취소 후 다시 연결할 다음 동작을 설명한다.
- **U4/R08 환경:** 변수 없음에 `baseUrl` 같은 이름을 직접 추가하는 짧은 안내, 이름 충돌을 해당 row와 연결한 오류, 저장 실패 안내를 모든 route에 보여준다. 자동 template 전체 치환이나 새 credential manager는 추가하지 않는다.
- **U5/R09 Webhooks:** 최초 미시작/수신 대기/조회 실패를 구별한다. 실패 시 마지막 기록과 rule draft를 유지하고 '새로 고침'을 제공한다. 수신 건수 변화만 live status로 알리고 payload 전체를 읽어주지 않는다.
- **U6/R11 Transforms:** 현재 편집 중인 pipeline ID, 변경사항, 저장 상태, 20개 한도를 표시한다. full 상태에서 기존 항목 편집과 삭제로 회복할 수 있게 한다. 입력/출력은 저장하지 않는 기존 정책을 유지한다. 기존 `ToolOutput`/handoff dialog는 이미 미리보기·취소·초점 복구가 있어 새 전달 UI를 만들지 않는다.

## R04 — 요청 편집 상태와 실제 HTTP 전송 일치

### 과제와 정확한 파일

1. AS01: None은 native에서 반드시 body 없음. 편집 초안은 보존해도 실행 payload에서 제외한다. unknown body kind를 catch-all로 보내지 않는다. None의 숨은 body 변수도 미해결 검사에서 제외한다.
2. AS02: 일반 query 및 form을 표준 serializer로 인코딩한다. 중복 key/순서/빈 값/Unicode/기존 query를 유지하고 fragment는 URL의 fragment 위치로 남긴다. 공유 helper를 쓰는 SSE도 확인한다.
3. AS07: active auth kind에 필요한 필드만 미해결 변수 검사·native secret resolve에 포함한다. None/Basic/Bearer/API key/OAuth2 전환 표를 frontend/native 양쪽에서 검증한다. 비활성 draft를 강제로 지워 해결하지 않는다.
4. U1/U2: 접근 가능한 요청 입력과 명확한 전송/취소/이전 응답 상태. 기존 request sequence stale-result 차단을 유지한다.

수정:
- `/home/jihoon/projects/devbox/crates/http-client-engine/src/commands/request.rs` — apply_body 1237–1254, resolve_template 1515 이후, referenced_variable_names 1788 이후, append_query/encode_form 3121–3154 및 같은 파일 test module.
- `/home/jihoon/projects/devbox/crates/http-client-engine/src/commands/sse.rs` — 공유 query helper signature 변경이 필요한 경우 호출부 378/654와 회귀 fixture. protocol 전체 재작성 불필요.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/lib/runner.ts`, `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/lib/runner.test.ts` — active-field 검사.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/components/RequestParameters.tsx` — None 초안 정책/필요한 안내.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/components/KeyValueEditor.tsx` — 레이블/포커스.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/App.tsx` — URL 이름/전송 상태 영역, response provenance props.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/ResponseViewer.tsx`, `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/ResponseViewer.test.tsx` — 이전 응답 표시. raw response 권한은 기존대로 native에서 판정하며 새 임의 우회 불필요.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/api.browser.test.ts`, `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/lib/codegen.test.ts` — browser/code preview와 None·encoding 의미 일치.
신규:
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/App.execution.test.tsx` — 실제 편집→보내기→취소→늦은 응답 전이.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/components/KeyValueEditor.test.tsx` — 키보드/레이블/초점 회귀.

새 회귀 파일과 test prefix는 아래 명칭으로 생성한다. 화면 이름은 아래 accessible name 계약을 사용한다. 송신 분기 변경은 기존 apply_body/append_query/encode_form/missingVariables 함수 안에 둔다.

### 실패 회귀와 assertion 예

- JSON draft 입력→None→Send를 loopback receiver로 받아 `assert!(captured_body.is_empty())`. editor에서 JSON으로 돌아가면 원 draft가 남는다. cURL에도 `--data`가 없다. None+`{{missing}}`는 send를 막지 않는다.
- query q=`a&admin=true`, x=`a#b`, plus=`a+b`, Unicode, duplicate q와 빈 값을 수신 parser로 읽어 `assert_eq!(pairs, expected_pairs)`; 요청에 임의 `admin` key가 없다. form은 `url::form_urlencoded::parse`한 결과가 입력 pair와 정확히 같다. 문자열 escape 형식만 assert하지 않는다.
- `expect(missingVariables({...request, auth:{...basicWithMissingPassword, kind:"none"}}, new Set())).toEqual([])`; Basic으로 다시 바꾸면 `toEqual(["hiddenPassword"])`. native 같은 전환에서 inactive secret unseal 미호출, active secret만 처리한다.
- 이전 응답 A→새 send B deferred→취소→B late resolve: `expect(screen.getByRole("status").textContent).toContain("취소")`, `expect(screen.queryByText("B unique response")).toBeNull()`, `expect(sendMock).toHaveBeenCalledTimes(2)`. 상태 문구가 이전 A를 새 응답으로 표시하지 않아야 한다.
- keyboard만으로 `getByRole("textbox", {name:"쿼리 1 이름"})`, `getByRole("button", {name:"쿼리 q 삭제"})` 접근. 삭제 후 `expect(document.activeElement).toBe(nextEditableField)`.

향후 좁은 명령(각 과제 관련 선택만 실행):
```bash
source ~/.cargo/env
python3 .github/scripts/verify-resources.py -- cargo test -p devbox-http-client-engine --lib commands::request::tests::none_body
python3 .github/scripts/verify-resources.py -- cargo test -p devbox-http-client-engine --lib commands::request::tests::query_form
python3 .github/scripts/verify-resources.py -- cargo test -p devbox-http-client-engine --lib commands::request::tests::active_auth
python3 .github/scripts/verify-resources.py -- pnpm --filter @devbox/api-studio-features exec vitest run src/requests/lib/runner.test.ts src/requests/App.execution.test.tsx src/requests/components/KeyValueEditor.test.tsx src/requests/ResponseViewer.test.tsx --maxWorkers=2
python3 .github/scripts/verify-resources.py -- pnpm --filter @devbox/api-studio-features exec vitest run src/requests/api.browser.test.ts src/requests/lib/codegen.test.ts --maxWorkers=2
```
Rust filter는 신규 회귀 test name prefix로 사용할 제안이다. 구현 후 실제 등록된 이름과 일치시켜 0 tests PASS를 허용하지 않는다. SSE helper 변경 시 `commands::sse::tests::query_encoding` 신규 회귀도 같은 방식으로 실행한다.

수용: Windows packaged API Studio→synthetic loopback echo에서 None wire-body 0, 예약문자/중복 key 왕복 보존, Auth 전환 후 활성 필드만 검증. 지연 응답 취소 후 새 요청이 정상 동작하고 늦은 결과가 덮어쓰지 않음. native raw/cURL/browser 의미 일치. 기존 서비스/Docker/방화벽 변경 없이 임시 fixture listener만 사용. 현재 native 실기 미실행.

## R05 — MCP 연결 인증의 표시·실행 일치

과제: AX01의 최소 수정으로 connected 상태의 grant 선택·authorize를 잠그고 실제 연결 시 snapshot grant를 표시한다. **dropdown만 잠그고 끝내지 않는다.** `ProtocolLab.tsx:497` authorize 완료와 520–540 grant refresh도 선택을 바꿀 수 있으므로 refresh가 active grant를 조용히 바꾸지 않게 한다. active grant revoke/실효는 연결의 인증을 조용히 B로 fallback하지 않으며 명시적 연결 종료/재연결 안내로 처리한다. 오류 후 도구 호출 자동 재시도 금지.

수정:
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/ProtocolLab.tsx` — 연결 snapshot 297–308, authorize/refresh/revoke, busy 610–611, grant selector 989–1017, connection card 1057–1071.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/ProtocolLab.test.tsx` — A/B/None 선택·refresh·revoke race를 UI로 검사.
- `/home/jihoon/projects/devbox/crates/http-client-engine/src/commands/mcp.rs` — 기존 connection snapshot→bearer 552–565 invariant를 same-file 테스트로 보강. native production profile wire shape 확대는 필수 아님.

실패 회귀:
1. A로 Connect 후 `expect((screen.getByLabelText("OAuth grant") as HTMLSelectElement).disabled).toBe(true)`와 연결 정보의 A clientId 표시를 assert. None/B 변경이 반영되지 않으며 tool invoke는 동일 connection ID를 사용한다.
2. 연결 중 grant refresh 결과 `[B]` 또는 A revoke 반환→'A 권한 확인 필요/재연결 필요' 또는 disconnect; `expect(invokeMock).not.toHaveBeenCalled()`(새 호출 시도 전 reset), 정상 연결처럼 B 표시 금지. authorize 응답이 이전 연결보다 늦게 도착해도 연결 인증 표시를 덮어쓰지 않는다.
3. Disconnect→B 선택→Connect→`expect(mocks.connect.mock.calls.at(-1)[0]).toMatchObject({oauthGrantId: grantB.grantId})`와 actual receiver의 Authorization B. 실제 connect API wrapper 인자 위치에 맞춰 assertion을 작성한다.
4. native fixture에서 A bearer와 B bearer를 구별하고 연결 A의 `tools/call` 수신 헤더가 A임을 assert. secret은 UI/timeline/export에 노출되지 않는다.

향후 명령:
```bash
python3 .github/scripts/verify-resources.py -- pnpm --filter @devbox/api-studio-features exec vitest run src/requests/ProtocolLab.test.tsx src/requests/mcpApi.test.ts --maxWorkers=2
source ~/.cargo/env
python3 .github/scripts/verify-resources.py -- cargo test -p devbox-http-client-engine --lib commands::mcp::tests::connection_grant
```
수용: Windows 소유 OAuth/MCP fixture로 A 연결→선택 잠금→grant refresh/revoke→명시 재연결 B 전체 확인. 공개 계정이나 실제 side effect tool 사용 안 함. stdio MCP 연결/해제도 smoke하여 HTTP 변경이 공통 버튼 상태를 깨지 않는지 확인. R04/R08과 기능 선행 없음.

## R08 — 환경 이름·값 보존과 저장 실패 복구 안내

과제:
1. AS04: Add가 existing key를 upsert하지 않도록 생성/추가와 수정 동작을 분리. `var2`만 존재하는 sparse/import 환경에서도 기존 값·secret flag 보존.
2. AS05: key 직접 입력/rename/delete. 빈 이름·template 문법에 맞지 않는 이름·중복 이름을 저장 전에 row 오류로 표시. rename은 sealed value를 복호화하지 않고 그대로 보존한다. 삭제 대상 이름을 보여주며 확인/취소를 제공한다. request template 참조를 임의 일괄 변경하지 않는다.
3. AS06: 생성한 environment ID를 정확히 선택한다. 이름 중복이어도 ID 기준. 저장 실패이면 선택 성공/완료처럼 보이지 않는다.
4. N02/U4: 저장/마이그레이션 실패를 HTTP/Protocols/History 공통 영역에 표시. 실패 뒤 fail-closed 쓰기 barrier 유지. 지원되지 않는 '다시 시도'로 stale revision 덮어쓰기를 강행하지 않는다. 현 세션 안전 재읽기를 설계하지 않으면 '편집 내용은 미저장, 앱을 다시 열어 저장 상태 확인'을 안내하고 재시작 수용을 검증한다.

수정:
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/components/RequestSidebar.tsx` — environment section 400–526.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/lib/environments.ts` 및 `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/lib/environments.test.ts` — collision-safe add/rename/remove 순수 조작.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/hooks/useEnvironmentPersistence.ts` — created ID 선택 142–150, serialized write와 seal race 유지.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/App.tsx` — warning 배치 1532–1535를 route 밖으로 이동하고 environment actions 연결.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/App.execution.test.tsx` — R04의 renderer fixture를 재사용하여 Protocols save-failure route 검사.
신규:
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/components/RequestSidebar.environment.test.tsx` — UI 행 추가/rename/delete/focus와 collision.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/hooks/useEnvironmentPersistence.test.tsx` — created ID/실패/늦은 seal 결과.
환경 UI를 새 package로 추출하지 않는다. 컴포넌트 분리는 필요하더라도 이 작업 내부 구현 판단이며 계획 완료 조건이 아니다.

실패 회귀:
- imported `var2=KEEP` 한 개→추가: `expect(saved.environments[0].variables).toContainEqual({key:"var2",value:"KEEP",secret:false})`; row 수 증가. secret variant도 sealed blob byte equality.
- `token` rename `accessToken`: `expect(renamed.value).toBe(originalSealedBlob)`; `expect(unsealMock).not.toHaveBeenCalled()`; collision rename 실패 시 `expect(saved).toEqual(before)`.
- Production 존재→Staging 생성: `expect(result.current.currentEnvId).toBe(createdId)` (hook fixture에서 callback로 받은 선택을 expose). save reject이면 selectedId가 변경되지 않으며 alert가 있음.
- Protocols route에서 environment write를 reject→`expect(screen.getByRole("alert").textContent).toContain("Environment")`; Add disabled, 이전 성공 저장본 유지. History 이동 후에도 안내가 존재. 재마운트/load 후 native revision에서 정상 상태 복구.
- 이름·값·삭제 버튼이 고유한 accessible name을 갖고 삭제 취소 시 row와 focus 보존.

향후 명령:
```bash
python3 .github/scripts/verify-resources.py -- pnpm --filter @devbox/api-studio-features exec vitest run src/requests/lib/environments.test.ts src/requests/components/RequestSidebar.environment.test.tsx src/requests/hooks/useEnvironmentPersistence.test.tsx src/requests/App.execution.test.tsx --maxWorkers=2
```
수용: R04/R05/R06 변경 통합 이후 시작. Windows 임시 테스트 문서 namespace에서 imported sparse vars→추가→이름변경→sealed 변수 보존→두 번째 환경 선택→앱 재시작 동일 결과. production secret 대신 테스트 값만 사용. native save 실패는 fixture로 주입하며 사용자 store 권한을 바꾸거나 손상시키지 않는다. native migration 실패/경고 visibility와 기존 export/import의 secret 정책도 영향 검사한다.

## R09 — Webhook 지속 수신과 조회 회복

과제: AS03을 기존 `/home/jihoon/projects/devbox/packages/hooks/src/usePolling.ts`의 single-flight/hidden pause를 소비하여 수정. 활성 route에서만 정해진 간격으로 읽고 복귀 즉시 읽는다. native listener는 route 숨김만으로 정지시키지 않는다. 최초 mount·수동 refresh·mutation 후 refresh가 같은 조정 경로를 사용하게 해 중복 read를 막는다. 서버상태/history polling과 사용자 편집중 rule draft를 분리하고 주기적 읽기가 입력을 초기화하지 않게 한다. 실패 시 마지막 정상 목록·선택은 유지하고 자동 갱신 실패 상태와 수동 재읽기 동작을 제공한다.

수정:
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/webhooks/App.tsx` — refresh 313–346, active prop, 상태/목록/수동 refresh controls.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/webhooks/App.test.tsx` — external-arrival fake-timer fixture, route active 전환, 지연/실패 읽기.
- `/home/jihoon/projects/devbox/apps/devbox-api-studio/src/Studio.test.tsx` — 기존 route suite에 hidden-but-mounted route 복귀와 active 전달 회귀 추가.
공유 usePolling 자체는 이미 overlap/visibility 테스트가 있으므로 원칙적으로 수정하지 않는다. polling이 read API를 쓰므로 webhook native lifecycle 재작성/새 event bus는 불필요.

실패 회귀:
- mount history=[]→별도 mutation 없이 listHistory mock=[newRecord]→timer tick→`expect(screen.getByText("/new-hook")).toBeTruthy()`; `expect(stopServerMock).not.toHaveBeenCalled()`.
- active=false 뒤 timer 여러 회→`expect(listHistoryMock).toHaveBeenCalledTimes(beforeHidden)`; active=true 후 interval을 기다리지 않고 다시 read. listener stop 호출 없음.
- 첫 read deferred 중 timer·refresh 연타→`expect(maxConcurrentReads).toBe(1)`; 먼저 시작한 stale read가 route 복귀 결과를 덮어쓰지 않음.
- rule draft에 미저장 텍스트→외부 arrival→`expect(ruleInput.value).toBe("unsaved draft")`; selection ID가 유효하면 유지. 사라진 selection에만 설명 후 해제.
- listHistory reject→기존 row 유지+실패 alert→수동 refresh 성공→alert 해제. 주기적 failure가 모든 fixture를 빈 목록으로 바꾸지 않도록 부분 성공 정책 검사.

향후 명령:
```bash
python3 .github/scripts/verify-resources.py -- pnpm --filter @devbox/api-studio-features exec vitest run src/webhooks/App.test.tsx --maxWorkers=2
python3 .github/scripts/verify-resources.py -- pnpm --filter devbox-api-studio exec vitest run src/Studio.test.tsx --maxWorkers=2
```
기존 Studio.test.tsx와 앱 package 이름을 현재 소스에서 확인했다. 변경한 경계에 해당하는 테스트만 실행한다.
수용: Windows Webhooks Start→외부 synthetic POST→명시된 갱신 시간 안에 기록 표시→Requests로 이동→추가 POST→복귀 즉시 두 번째 표시. 기록 선택/스크롤·rule draft 유지, 일시 조회 실패 뒤 회복. 기존 listener agent start/stop/port ownership 계약 유지. 서비스 재시작을 강제하지 않는다. 기능 선행 없음.

## R10 — gRPC 오류 종료 이전 메시지 보존

과제: `/home/jihoon/projects/devbox/crates/http-client-engine/src/commands/grpc.rs:1003`–1009의 stream error 시 누적 메시지를 버리는 경로 수정. 정상 transport terminal status와 이미 수신된 messages를 함께 반환한다. local size limit/cancel/권한 실패는 기존 차단 정책을 보존하며 임의 부분 성공으로 바꾸지 않는다. server/bidirectional streaming만 영향; unary/client-streaming 반환을 별도 보존 검증한다.

수정:
- `/home/jihoon/projects/devbox/crates/http-client-engine/src/commands/grpc.rs` — collect_response_stream/status_outcome와 같은 test module의 tonic fixture.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/GrpcLab.tsx` — 결과/summary 453 이후에 '2개 수신 후 INTERNAL 종료'처럼 partial 결과를 설명. 기존 result schema 필드로 가능한 범위.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/GrpcLab.test.tsx` — partial result 렌더링·summary 일치.
신규 프로토콜, live incremental streaming UI, transport 재작성은 범위 밖.

실패 회귀: fixture `Ok(message1), Ok(message2), Err(Status::internal("synthetic"))`→`assert_eq!(outcome.status, "INTERNAL")`, `assert_eq!(outcome.messages.len(), 2)`; native invoke projection에서도 `response_message_count == 2`. UI `expect(screen.getByText(/INTERNAL/)).toBeTruthy()`와 두 메시지 내용/count 표시. 오류 status 원문 secret은 노출하지 않으며 0 message error, all-OK, bounds overflow, cancel도 기존 결과 유지.

향후 명령:
```bash
source ~/.cargo/env
python3 .github/scripts/verify-resources.py -- cargo test -p devbox-http-client-engine --lib commands::grpc::tests::partial_stream
python3 .github/scripts/verify-resources.py -- pnpm --filter @devbox/api-studio-features exec vitest run src/requests/GrpcLab.test.tsx src/requests/grpcApi.test.ts --maxWorkers=2
```
수용: Windows 임시 local fixture에서 server/bidi 각각 2 messages 후 INTERNAL→두 메시지·non-OK 종료 함께 표시하고 summary/export count와 일치. 기존 masking/bounds 확인. R05와 기능/파일 선행 없음.

## R11 — Transforms 파이프라인 편집 identity와 저장 회복

과제:
1. AX02: load한 pipeline 수정 시 ID 보존. 단계 추가/삭제/입력유형 수정이 저장 대상을 조용히 신규로 바꾸지 않는다. 신규 시작은 명시 동작으로 분리하고 현재 ID/dirty 상태 표시.
2. 20개 상태에도 existing pipeline update 허용. delete를 제공하여 새 pipeline 저장 capacity 회복. 삭제 취소/실패면 기존 metadata 보존. 자동 가장 오래된 항목 eviction 금지. 불필요한 이름짓기/폴더/공유 기능 확대 없음.
3. save pending/confirmed/error를 구별하고 확인 전 저장 완료 표시 금지. full 상태 안내에 현재 count/기존 항목 편집·삭제 동작 표시. 실패 후 native revision 재확인 없는 write barrier 해제 금지.
4. U6: 단계 삭제 버튼 대상 이름과 키보드 초점 유지. 결과 handoff는 현재 성공한 output에만 가능하도록 기존 `allowHandoff`/output source metadata 경계 유지. 입력/출력 본문은 metadata에 저장하지 않는다.

수정:
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/transforms/workflows/SmartWorkflowPanel.tsx` — persist 187–205, selectedPipelineId reset 218–270, save 282–295, library 567–587.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/transforms/workflows/workflowStore.ts` — collision-free ID 223–229, existing upsert 231 이후, removePipeline 순수 함수 추가(같은 파일).
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/transforms/workflows/SmartWorkflowPanel.test.tsx` 및 `/home/jihoon/projects/devbox/packages/api-studio-features/src/transforms/workflows/workflowStore.test.ts` — full/edit/delete/restart/failure.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/transforms/tools/ApiHandoffAction.test.tsx` — output 변경 후 stale preview·late completion 경계가 기존 검사에 없다면 보충.
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/transforms/App.applink.test.tsx` — pipeline 결과→API draft route의 명시 동작/자동 send 없음 회귀 보충.
`tools/ApiHandoffAction.tsx`, `tools/common.tsx`, `tools/KnowledgeDraftAction.tsx`는 기존 접근성/출력 정책 재사용 대상으로 검토했다. 이 감사에서 별도 확인 결함은 없으므로 production 수정을 계획에 자동 포함하지 않는다.

실패 회귀:
- metadata에 20개 seed→pipeline-7 load→단계 추가→Save→`expect(saved.pipelines).toHaveLength(20)`; `expect(saved.pipelines.find(p=>p.id==="pipeline-7")?.steps).toEqual(editedSteps)`; 나머지 19개 원본 동일. unmount/remount 뒤 수정본 재로드.
- 한 개 삭제 취소→`expect(after).toEqual(before)`; 삭제 확인·저장→19개→명시 새 pipeline 저장→20개. `expect(savedText).not.toContain("synthetic-private-input")`; input/output 키도 없음.
- save deferred→저장 중 상태; reject→`expect(screen.getByRole("alert").textContent).toContain("저장")`, 성공 문구 없음, 로드한 마지막 committed metadata가 재시작 뒤 남음. UI draft가 실패로 사라지지 않음.
- 실패한 pipeline 실행은 `expect(screen.queryByRole("button", {name:/API.*보내기/})).toBeNull()` 또는 disabled(현재 표시 계약에 맞춤). 성공 output은 명시 preview 승인에서만 handoff publish 1회; 수신 Requests가 자동 send하지 않음. 취소 시 publish 0회.

향후 명령:
```bash
python3 .github/scripts/verify-resources.py -- pnpm --filter @devbox/api-studio-features exec vitest run src/transforms/workflows/SmartWorkflowPanel.test.tsx src/transforms/workflows/workflowStore.test.ts --maxWorkers=2
python3 .github/scripts/verify-resources.py -- pnpm --filter @devbox/api-studio-features exec vitest run src/transforms/tools/ApiHandoffAction.test.tsx src/transforms/App.applink.test.tsx --maxWorkers=2
```
수용: Windows 실제 document namespace에서 20개 임시 metadata 준비→existing edit→restart→delete→new save. 성공 결과를 API/Knowledge로 명시 전달 후 recipient preview/취소/적용 확인; API 자동 send 및 clipboard fallback 없음. 별도 cross-product permission route 확장은 하지 않는다. 기능 선행 없음; Requests recipient 통합은 R04 accepted baseline에서 수행.


## Windows UI 시나리오의 파일·등록 계약

각 작업은 아래 파일과 같은 이름의 `.test.mjs`를 생성한다. 테스트 파일은 fixture ownership·allowed scenario ID·failure cleanup과 응답 형태를 검사한다. Windows 실제 결과는 `run`의 실행 evidence로 따로 기록한다. R00 UiDriver를 받아 visible input으로 제품을 조작하고 native 관찰/loopback fixture는 검증 대상 결과만 읽는다.

| 작업 | 신규 모듈(저장소 `.github/scripts/` 아래) | 반환 ID |
|---|---|---|
| R04 | `windows-api-request-semantics-ui.mjs` | HTTP-01/02/03 |
| R05 | `windows-api-mcp-auth-ui.mjs` | AUTH-01/02 |
| R08 | `windows-api-environment-ui.mjs` | ENV-01/02 |
| R09 | `windows-api-webhooks-ui.mjs` | WEB-01/02 |
| R10 | `windows-api-grpc-results-ui.mjs` | GRPC-01 |
| R11 | `windows-api-transforms-ui.mjs` | TRANSFORM-01 |

인터페이스(신규 각 모듈이 같은 export를 구현):

```ts
export async function run(context: {
  ui: UiDriver;
  fixtureRoot: string;
  sourceSha: string;
  fixtureSha: string;
  artifactDigests: Record<string, string>;
}): Promise<ScenarioResult[]>;
```

`UiDriver`·`ScenarioResult`는 02 R00의 계약이다. 각 모듈이 자신의 합성 echo/OAuth/gRPC fixture를 임시 loopback port에 생성·종료하고 namespace/소유 PID를 검증한다. context에 사용자 token·사용자 store를 전달하지 않는다. R00 matrix의 `module`과 ownerWorkItem에 등록하고 작업 subset runner가 그 모듈을 실제 import·실행하는 config 회귀를 같은 PR에 추가한다. R07의 `windows-suite-user-flow.mjs`가 R15 통합 시 이 모듈들을 호출한다. 기능 작업의 사전 준비 namespace L4는 별도 evidenceKind로 남긴다.

예상 좁은 harness 검사(R04 예):

```bash
node --test .github/scripts/windows-api-request-semantics-ui.test.mjs
node --test .github/scripts/test-suite-user-flow-matrix.mjs
```

그 외 작업은 표의 같은 basename `.test.mjs`를 실행한다. 모듈/시험 파일이 없거나 반환 ID가 누락되면 해당 작업 수용은 미완료다.

## 공통 완료 기준과 아직 검증하지 않은 경계

- 작업별로 최소 회귀만 실행하고 모든 작업의 마지막 변경을 통합한 뒤 최종 검증 1회에 Biome·필요 Windows 수용을 묶는다(00 §4). all 범위를 실행했다면 affected를 다시 실행하지 않는다. 작업별 CI는 만들지 않는다.
- test filter가 실행한 개수를 기록한다. 계획의 신규 Rust prefix/신규 UI file을 구현 전 실행해 0-test 성공으로 기록하지 않는다.
- 공통 자원 제한/실행 lock 준수. 직접 narrow cargo/vitest도 전역 full run과 겹치지 않는다. Rust 사용 전 `source ~/.cargo/env`. 현재 계획 작성 중에는 어떠한 test도 실행하지 않았다.
- Windows 미실행은 미실행으로 남긴다. native request/protocol/broker 검증을 mocked Vitest PASS로 대체하지 않는다. 임시 synthetic fixture와 namespace를 쓰며 사용자 secret/store를 fixture로 쓰지 않는다.
- 새 native invoke/binding이 불가피해질 때만 generated bindings와 route admission contract 검사 범위를 확대한다. 지금 계획은 기존 command의 의미를 고치는 작업으로 설계했다.
- 실제 OAuth refresh/revoke race, gRPC transport terminal status, Windows DPAPI environment roundtrip, Webhook agent 별도 프로세스 lifecycle, packed cross-product handoff는 정적 검토만으로 성공이라 결론 내릴 수 없다. 위 native 수용 전까지 각각 미검증이다.
