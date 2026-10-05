# 실제 사용자 여정·패키지 검증·재출시 계획

> **For agentic workers:** 이 문서의 명령·시나리오는 실행 예정이다. 현재 감사의 테스트 통과를 아래 수용 PASS로 복사하지 않는다. [00](00-roadmap.md), [01](01-product-contract.md)의 제약과 순서를 적용한다.

**Goal:** 사용자가 보는 UI부터 native side effect/저장 결과까지 같은 source와 package에서 증명한다.

**Architecture:** 단위·경계·브라우저·Windows packaged UI·설치/복원 검증을 분리한다. 공통 evidence validator는 필수 시나리오 누락/다른 SHA/다른 bytes/NOT_RUN을 거부한다. 후보는 재빌드 없이 공개한다.

**Tech Stack:** 기존 pnpm/Cargo·Node test runner·CDP·Windows UIA·로컬 WSL/Windows·독립 VM fixture, 최종 후보의 GitHub-hosted Windows/WSL2. Docker/네트워크 변경 시험은 검증된 독립 VM 또는 최종 hosted 환경에 한정한다.

**Spec:** 01의 Review Focus 5개, 모든 제품 계약. 다음 매트릭스와 08 추적표가 요구사항 목록이다.

## 1. 검증 계층과 진입 조건

| 계층 | 입력과 assertion | 의미/한계 |
|---|---|---|
| L1 단위 | 순수 상태·serializer·revision·후보 필터·idle 기록 | 원인 RED→GREEN. 화면/OS PASS 아님 |
| L2 renderer↔native 경계 | native enum/admission에서 가져온 허용/거부표를 fixture에 반영 | 상태 조합 검증. mock이 금지 API를 성공시키면 실패 |
| L3 실제 Chromium UI | 실제 component·CSS·keyboard·합성 데이터, UI state 주입으로 전이 생략 금지 | 레이아웃/접근성/사용 전이. native/Windows PASS 아님 |
| L4 Windows 제품 UI | exact executable·isolated data·WebView 입력→실제 native 결과/disk/wire 관찰 | 각 작업의 기능 수용. 설치 자체의 PASS는 별도 |
| L5 installer/update/recovery | interactive installer→최초 실행→종료/재개/commit, 기존 데이터 보존 | release 후보의 전체 수용 |

R01–R06은 설치 flow 수정 전이므로 L4를 **미리 준비한 소유 fixture namespace**에서도 수행한다. 그 provisioning을 최초 설치 UI PASS로 부르지 않는다. 이 구분으로 R07과 이전 작업의 의존 순환을 피한다. R07 이후 L5는 준비 API/helper 직접 호출 없이 visible UI로 완료한다. L4 native-boundary fixture와 L5는 모두 필요하다.

각 작업은 결함/변경을 입증하는 최소 계층을 선택한다. L1–L5를 모두 반복하는 의무는 없다. 로컬에서 좁은 회귀를 먼저 실행하고 나머지 기능/OS 수용은 최종 통합에서 묶는다. OS 환경 미확보 항목은 NOT_RUN으로 유지하면서 독립 구현은 계속하고, 최종 후보의 필수 L4/L5가 미실행이면 출시하지 않는다. R15와 R16에서 동일 후보의 이미 통과한 결과를 다시 실행하지 않는다.

## 2. 기능·실행 매트릭스

아래 40개 ID는 결과 추적 단위이며 40회 독립 실행이나 40개 CI job을 뜻하지 않는다. 기존 runner에서 설치/소유 프로세스/fixture를 재사용해 다음 여정으로 묶는다. 기능 수정 중에는 영향 항목만 로컬에서 고르고 전체 여정은 통합 후 실행한다.

| 통합 실행 여정 | 함께 관찰할 ID | 주 실행 시점 |
|---|---|---|
| 설치·중단·업데이트·제거 | INSTALL-01–03, DELIVERY-01–02, AGENT-01/04 | 최종 후보의 설치/격리 VM 수용 |
| Workspace 편집·실행·연결 | WORK-01–03, RUNTIME-01–02, LSP-01, DEPS-01 | 로컬 Windows 통합, 후보의 동일 runner |
| API 요청·인증·환경·수신·변환 | HTTP-01–03, AUTH-01–02, ENV-01–02, WEB-01–02, GRPC-01, TRANSFORM-01 | 로컬 Windows 통합, 후보의 동일 runner |
| Knowledge 보존·검색·활동 | DOC-01–03, SEARCH-01–02, ACTIVITY-01–03 | 로컬 Windows 통합, 격리가 필요한 OS 동작은 후보 |
| 앱 간 전달·Agent 실패 복구 | HANDOFF-01–02, AGENT-02–03 | 위 네 제품이 열린 통합 세션 |
| 공통 UI·성능 | UI-01–02, PERF-01 | 위 여정에 관찰 삽입; 별도 전체 시작/설치 반복 금지 |

하나의 여정에서 여러 결과를 반환하고 실패 시 해당 단계와 영향 범위만 다시 실행한다. 로컬 소스 실행은 개발 검증, 후보 실행은 배포 bytes 검증으로 구분한다. same-source/fixture/digest 결과 재사용 시 원 실행을 링크한다. 수용 ID를 합치더라도 실패 원인과 누락을 식별할 assertion은 보존한다.


모든 fixture는 합성 내용과 임시 소유 namespace를 사용한다. 권한 거절·offline·정상 종료·강제 종료를 테스트할 때 대상은 이번 실행이 만든 정확한 process/file/installation뿐이다.

| ID | 소유 작업 | 실제 행동 | 필수 결과 |
|---|---|---|---|
| UI-01 | R06 | 네 앱 direct/committed/import/health/recover 화면, 최소·기본 창 | main/primary action 가시성, banner 겹침/본문 축소 없음 |
| UI-02 | R06 | keyboard-only, 모달 열기/닫기, IME, 배율/텍스트 확대 | focus 순환/복귀, 조합 중 단축키 오동작 없음, 핵심 작업 완료 |
| INSTALL-01 | R07 | 새 설치 interactive installer부터 네 제품 준비·활성화·첫 작업 | 콘솔/API 우회 없이 committed, 재실행 후 정상 |
| INSTALL-02 | R07 | 제품 1개 미준비/불응답, 중간 취소·Control Center 종료·재열기 | 실패 대상/다음 행동 명확, 이미 준비된 store 재생성 없음 |
| INSTALL-03 | R07 | 데이터 편집 중 활성화/업데이트 종료 요청 취소 | 미저장 자료 보존, blocker 유지, 강제 종료 없음 |
| DOC-01 | R01 | A edit→B→Undo/Redo→autosave→재열기 | B disk에 A sentinel 없음, B의 정상 Undo 유지 |
| DOC-02 | R01 | journal 기록→owned process crash→원본 삭제/offline→복구 UI | preview/copy, 명시 recreate/save-as, CAS 충돌 시 원본/저널 보존 |
| DOC-03 | R01 | keep-close/permanent discard 중 delayed save/journal | 선택한 의미대로만 보존/삭제, late timer 재생성 없음 |
| WORK-01 | R02 | Files 편집→recovery 기록→X의 save/discard/cancel→재실행 | 내용·encoding 보존, writer 실패 시 종료 거절 |
| WORK-02 | R02 | Source 초안→Registry/Agents/handoff로 context 변경 | dirty guard, 취소 후 원문·context·실행 세션 유지 |
| WORK-03 | R03 | base-only registry→agent task 생성→Source/Dependencies→cleanup | 수동 새로고침 없이 일치, 같은 mutation 재시도 없음 |
| HTTP-01 | R04 | Body JSON→None→echo 수신·cURL 확인 | wire body 0, inactive variable/secret resolve 없음 |
| HTTP-02 | R04 | 예약문자/한글/중복/빈 query/form 및 SSE | 서버 decode 결과가 입력과 동일, 잘못된 parameter 추가 없음 |
| HTTP-03 | R04 | 지연 요청→취소→새 요청→이전 응답 지연 도착 | 현재 응답 덮어쓰기·자동 재송신 없음, 상태 구분 |
| AUTH-01 | R05 | MCP A grant 연결→B/None 변경 시도→tool mutation | 실제 적용 A 표시/편집 잠금 또는 재연결 전 차단, 1회 side effect |
| AUTH-02 | R05 | grant refresh/revoke/만료·인증 실패·재연결 | 무음 fallback 없음, 사용자 재연결로 복구, token 노출 없음 |
| ENV-01 | R08 | sparse var2 import→추가/rename/delete→둘째 환경 생성 | 기존 값/secret 보존, 새 환경 선택, 충돌/삭제 안내 |
| ENV-02 | R08 | Requests/Protocols에서 환경 저장 거절·재읽기 | 모든 route에 실패 원인, write lock 유지, 임의 초안 폐기 없음 |
| WEB-01 | R09 | listener 시작→외부 POST→대기→route hide/show | 새 기록 자동 표시, inactive polling 제한, 복귀 refresh |
| WEB-02 | R09 | Agent 끊김/조회 실패→복구, rule 초안·선택 유지 | 마지막 기록 보존, retry는 read-only, listener 중복 시작 없음 |
| GRPC-01 | R10 | 2개 정상 stream message→non-OK trailer | messages=2+오류 상태, UI/history 일치 |
| TRANSFORM-01 | R11 | 20개 저장 상태에서 기존 pipeline 수정/삭제/새 저장 | 기존 id 유지·용량 복구, 입력/출력 본문은 저장되지 않음 |
| SEARCH-01 | R12 | foo/bar/axb/한글 파일, regex·모드/source 저장복원 | 누락 없는 매칭, invalid/partial/0건 구분 |
| SEARCH-02 | R12 | index 취소/재시작·root offline·삭제·reference expiry | stale 결과로 쓰기 실행 안 함, 진행/재시도/미완료 표시 |
| ACTIVITY-01 | R13 | owned foreground fixture 입력→idle threshold→재입력 | 기대 last-input 경계로 실제 집계, 현재 설정 readback 일치 |
| ACTIVITY-02 | R13 | Timeline 이동·history 과거기간 재생성 | 선택 날짜 변경, entry 기간/timezone으로 새 digest |
| ACTIVITY-03 | R13 | DB 설정 저장 실패·privacy/consent/pause·installed UI 종료 | 거짓 성공 없음, 동의와 owner에 맞는 수집 상태/문구 |
| RUNTIME-01 | R14 | run/stop 응답 유실→receipt 조회→확인/재시도 | 중복 job/service 없음, 결과 불확실 상태의 안전한 복구 |
| RUNTIME-02 | R14 | Terminal 실패/재연결·Ctrl+C·프로필 변경·세션 재열기 | 자동 명령 재실행 없음, 정확한 대상과 상태 표시 |
| LSP-01 | R14 | 검토된 fixture LSP install/import/cancel/uninstall | partial 정리·digest 검증·정확한 owned directory 제거·이전 설치 보존 |
| DEPS-01 | R14 | lockfile 변경/OSV·deps.dev 실패/취소 후 재분석 | 새 revision 재검토, 이전 preview 승인 재사용 금지 |
| AGENT-01 | R15 | store 없음→제품별 준비→동시 첫 연결 | 준비 후 초기화 재시도, 단일 owner/writer·단일 launch |
| AGENT-02 | R15 | owned Agent crash→조회/명시 재연결→business call | stale connection 폐기, 1회 효과, 미확정 mutation 자동 재전송 없음 |
| AGENT-03 | R15 | tray 명시 종료→기존 제품 polling→수동 재연결 | 자동 재시작 억제, 명시 재연결만 허용 |
| AGENT-04 | R15 | installed 제품 X/portable X/update quiesce | 소유권대로 background 유지/정리, update 중 재기동 없음 |
| HANDOFF-01 | R15 | Workspace 선택→API 변환→Knowledge draft, hot/cold product | 대상/내용 확인, 취소/만료/revision 변경 거절, 중복 소비 없음 |
| HANDOFF-02 | R15 | 다른 installation/연결 off/수신 unavailable→복구 | 원래 권한 경계 유지, 현재 상태 안내, 자동 외부 작업 없음 |
| DELIVERY-01 | R15/R16 | v0.8.1 합성 데이터/WAL→업데이트→등록된 네 제품 바로가기 실행→사용→rollback/restore | 원본 보존, namespace 불혼합, postcommit 새 데이터 보존, 바로가기의 현재 제품 연결 |
| DELIVERY-02 | R15/R16 | 재설치·제거·재설치와 취소/공간·쓰기 실패 | 사용자 자료 보존, owned 파일만 정리, 재개/복구 가능 |
| PERF-01 | R15/R16 | 네 제품 cold/warm/idle, 500-file search, 대표 작업 | 기존 budget 준수, 누락 앱은 NOT_RUN, owner/process 누수 없음 |

실제 provider 외부 서비스는 deterministic fixture로 timeout/성공/거절을 제어한다. 공식 서비스의 존재·호환성에 의존하는 변경이 생기면 구현 담당자가 당시 공식 문서/endpoint 계약을 확인한다. 이 계획의 코드 사실을 외부 최신 정책이라고 주장하지 않는다.

## 3. Agent Activity OS 검증 개선

현재 `/home/jihoon/projects/devbox/.github/scripts/windows-agent-collectors.mjs`의 physical foreground 항목을 사용자 확인 대기로 끝내지 않는다. R13/R15에서 다음 소유 fixture를 구현한다.

- 이번 runner가 띄운 고유 HWND/PID의 synthetic window만 foreground로 설정한다. 인위적 입력도 이 창에만 보낸다.
- Windows `GetLastInputInfo` 관찰값과 Agent가 저장한 구간을 같은 clock 기준으로 비교한다. 타임아웃/foreground 획득 실패는 실패 또는 NOT_RUN으로 기록한다.
- 테스트 전후 unrelated window/process 정보를 캡처하지 않는다. full window title·실제 사용자 입력을 evidence에 넣지 않는다.
- consent off 상태는 이벤트 기록 0, 명시 start/pause/resume은 각 상태를 확인한다. UI X와 tray 종료를 별도로 실행한다.
- watcher/수집 process 손실 뒤 duplicate session이 생기지 않는지 검사한다. native mock collector만으로 실제 OS 수집 PASS를 대신하지 않는다.

## 4. 성능·안정성 범위

새 임의 목표를 통과시키려고 기존 제한을 늘리지 않는다. 현 `/home/jihoon/projects/devbox/.github/scripts/product-foundation-performance.json`의 budget(예: cold renderer 30s, 첫 keyboard event 1s, warm 10s, index500Files 30s, search 1s)을 계속 검사한다. historical v0.7 baseline/app ID 목록은 현 네 제품의 측정 목록과 분리한다. 현재 Workspace/API lifecycle의 측정을 재사용하고 Knowledge/Control Center도 같은 schema로 누락 없이 보고한다.

- 각 제품이 실제 측정됐는지 required product ID로 확인한다. 빈 measurement 배열이 PASS가 되면 validator 실패다.
- 동일 hosted VM/동일 fixture/동일 조건의 비교만 회귀 근거로 사용한다. 서로 다른 runner 시간의 단순 비교로 성능 개선을 주장하지 않는다.
- 대표 route 전환/열기·닫기 뒤 listener/polling/timer/owned process 정리를 확인한다. 반복 횟수 20회를 고정 합격 조건으로 쓰지 않고 누수 징후가 있을 때만 필요한 재현 횟수로 확대한다. 무관한 전역 heap 숫자만으로 leak 판정하지 않는다.
- 장시간 수신/작업은 메시지·메모리·history 상한과 cancel/drain을 검사한다. ‘실시간’ 때문에 무제한 polling/재시도를 추가하지 않는다.
- 로그/지원 번들은 원문 request body, secret, 파일 내용, 사용자 경로를 포함하지 않는 기존 schema를 유지한다.

## R15 — 전체 여정의 연결과 gate

제목: `test(suite): require complete packaged user journeys before release`

선행: R01–R14의 통합과 R16 배포/문서 준비. 기능 작업에서 필요한 assertion을 추가하고 이 작업은 중복 실행을 제거하며 누락·상호작용·최종 gate를 마무리한다. R16 실행 결과가 R15의 입력이어야 한다는 의존 순환을 만들지 않는다.

수정/신규 파일:

- R00의 `/home/jihoon/projects/devbox/.github/scripts/suite-user-flow-evidence.mjs` 및 tests
- R00의 `/home/jihoon/projects/devbox/.github/scripts/suite-user-flow-matrix.json` — 위 required ID, owner, 환경, evidence kind
- R00의 `/home/jihoon/projects/devbox/.github/scripts/test-suite-user-flow-matrix.mjs` — 중복/누락/고아 시나리오, 모든 제품 mapping 검사
- R07의 `windows-suite-user-flow.mjs`와 각 기능 작업의 Windows UI 모듈
- `/home/jihoon/projects/devbox/.github/scripts/windows-agent-{collectors,reconnect,runtime,webhooks}.mjs`
- `/home/jihoon/projects/devbox/.github/scripts/windows-suite-delivery.ps1`, `windows-suite-delivery-native.mjs`, `windows-installer-acceptance.ps1`
- `/home/jihoon/projects/devbox/.github/workflows/product-foundation.yml`, `windows-package-candidate.yml`
- `/home/jihoon/projects/devbox/.github/scripts/build-candidate-metadata.py`, `resolve-release-candidate.py`와 각 기존 test 파일
- performance 기존 scripts/config와 docs/release-policy.md·verification.md

### Task 15.1 — 누락/대체 성공을 막는 집계

- [ ] RED: required ID 삭제·NOT_RUN·source/fixture SHA 불일치·digest 불일치·UI 대신 boundary evidence를 넣은 matrix를 모두 거부한다.
- [ ] RED: candidate metadata의 UI job이 failure/cancelled/skipped이거나 artifact가 없으면 promotion resolver가 거부한다.

```js
assert.equal(summarizeEvidence(requiredMatrix, allPassing).ready, true);
for (const missingId of requiredMatrix.map(row => row.id)) {
  assert.equal(summarizeEvidence(requiredMatrix, allPassing.filter(r => r.id !== missingId)).ready, false);
}
assert.equal(summarizeEvidence(requiredMatrix, withWrongPackageDigest).ready, false);
```

`requiredMatrix`는 R00에서 고정한 JSON, `allPassing`/`withWrongPackageDigest`는 test 파일의 합성 ScenarioResult 목록이다. `summarizeEvidence`는 R00 validator에 추가하는 집계 함수이며 결과는 `{ready:boolean, missing:string[], failures:string[]}`로 고정한다.

- [ ] candidate metadata와 workflow `needs`에 실제 UI 수용 artifact를 연결한다. UI evidence 없는 기존 candidate는 새 정책에서 승격되지 않는다.
- [ ] timeout/불안정 시 첫 실패 evidence를 유지한다. 원인 수정 없는 retry를 안정성 PASS로 바꾸지 않는다.

```bash
node --test .github/scripts/test-suite-user-flow-matrix.mjs .github/scripts/suite-user-flow-evidence.test.mjs
python3 .github/scripts/test-build-candidate-metadata.py
python3 .github/scripts/test-resolve-release-candidate.py
python3 .github/scripts/test-windows-package-candidate-config.py
python3 .github/scripts/test-release-candidate-promotion-config.py
```

### Task 15.2 — actual UI·OS·data 여정 실행

- [ ] matrix를 scenario별 source/환경/관찰 산출물에 연결하고 source-defined native policy를 우회한 UI test가 없는지 리뷰한다.
- [ ] 로컬 Windows/독립 VM에서 실행 가능한 변경 영향 여정만 수행한다. WSL/Docker와 미확보 OS 환경, 최종 package bytes에 대한 전체 수용은 R16의 동일 후보 실행에 합친다. R15는 runner/gate와 로컬 검사를 준비하고 해당 packaged 결과는 NOT_RUN으로 남긴다. 후보가 아직 없다는 이유로 개발을 막거나 중복 전체 실기를 요구하지 않는다. 로컬 차단은 해제하지 않는다.
- [ ] v0.8.1 fixture migration/restore의 원본과 postcommit 합성 자료 hash를 확인한다. 사용자 폴더를 fixture로 복사하지 않는다.
- [ ] 실패는 해당 소유 코드에서 수정하고 그 경계/영향 범위만 보충한다. 동일 코드를 여러 번 전체 실행하여 운 좋게 통과한 결과를 고르지 않는다.
- [ ] 최종 로컬 검사·PR CI·evidence matrix를 단일 PR 본문에 연결한다. 23건 및 N01–N04는 구현/로컬 검증과 packaged 수용 상태를 분리한다. 전자는 테스트 이름·commit·실행 근거를 기록하고 후자는 후보 실행 전 NOT_RUN을 유지한다. R16 뒤 같은 PR 본문에 실제 candidate run을 추가한 뒤 최종 완료로 판정한다.

## R16 — v0.9.0 문서 확정과 동일 후보 재출시

작업 커밋 제목 예: `chore(release): prepare verified v0.9.0 republication`

**두 단계:** 문서/배포 입력 준비는 R00부터 각 구현과 병행해 R15 최종 통합 검사 전에 완료한다. 실제 candidate·tag·공개는 R15의 로컬 검사/runner 준비와 단일 PR의 required CI 수용 후 진행한다. 전체 packaged 수용은 이 단계에서 완료한다. R16용 별도 PR은 만들지 않는다. 이번 계획 편집은 태그/릴리스를 실행했다는 뜻이 아니다.

### Task 16.1 — 전체 문서 정합성과 배포 입력 준비(최종 PR 전)

- [ ] 제품/Agent 버전은 **0.9.0으로 유지**한다. 네 제품 Cargo.toml 버전 원장과 tauri.conf.json·package.json, Agent Cargo.toml·tauri.conf.json, Cargo.lock의 해당 package, catalog/manifest/tag/설치 파일명 일치를 확인한다. 내부 library 버전을 불필요하게 바꾸지 않는다.
- [ ] 기존 v0.9.0 설치 사용자와 v0.8.1 업그레이드 데이터를 합성 fixture로 수용한다. 같은 버전 재설치 시 installer/update가 수정본으로 교체하는지, old/new manifest digest·component generation으로 구분되는지 확인한다. 단순 버전 동일 비교로 교체를 건너뛰면 setup 재설치 경로를 수정하고 안내까지 맞춘다.
- [ ] v0.8.1 updater의 새 구성요소 거절은 숨기지 않고 setup 직접 설치 경로를 안내한다. v0.9.0 철회본 사용자의 수정본 재설치 경로와 데이터 보존 조건도 명시한다.
- [ ] 다음 현재 문서를 구현과 대조해 **단일 PR 안에서 모두 정리**한다.

| 문서 범위 | 일치시킬 내용 |
|---|---|
| AGENTS.md·CONVENTIONS.md·docs/verification.md·codex-setup.md | 단일 통합 PR, 로컬 우선/중복 방지, 자원·정리, 허용 모델 2개, 삭제된 skill 참조 |
| 루트 README·네 앱 README·docs/projects.md·windows-guide.md·관련 현재 설계/ADR | 실제 화면·최초 설치/활성화/재개, 권한·owner, 데이터·종료/복구·제한 |
| CHANGELOG·release-policy.md·release-evidence.md·release notes template | v0.9.0 철회와 재출시 구분, 태그 처리, 배포 파일, 최소 CI/후보 순서, 검증 전 완료 주장 제거 |
| 새 계획 8개·기존 로드맵/계획/수용 문서 | 작업 ID·선행·완료 상태·검증 근거 일치; 과거 기록은 당시 상태로 표시하고 현재 지침과 구분 |
| dependency/notice 관련 문서·THIRD_PARTY_NOTICES.md | 의존성 변경 시 기존 생성기로 갱신, 배포 notice와 실제 포함 component 일치 |

- [ ] `rg`로 저장소의 추적 Markdown/설정 전체를 찾아 버전, 공개 여부, v0.9.1 제안 잔재, 닫힌 #580의 현재 원장 취급, skill 경로, 작업별 PR/CI 의무, 설치 뒤 사용자 확인 의존, 깨진 링크/삭제된 파일 참조를 점검한다. 과거 버전 언급을 일괄 치환하지 않고 역사/fixture/현재 계약을 구분한다.
- [ ] 23개 감사·4개 추가 결함·12개 개선의 구현/문서/검증 연결을 08에 확인한다. 패키지 검증 전 문서는 준비/예정 상태로 써 두고 미래 run ID/PASS를 꾸미지 않는다. 공개 결과는 같은 PR 본문·Release notes·artifact에 추가해 별도 결과 문서 PR을 만들지 않는다.
- [ ] 문서 링크·버전·metadata 확인은 필요한 기존 검사와 경량 검색으로 묶고, 문구 수정마다 전체 build/test를 다시 돌리지 않는다. 최종 영향 검증과 유일한 PR의 required CI를 마친 뒤 main으로 머지한다.

### Task 16.2 — 최종 main·기존 태그 처리·후보(머지 후)

- [ ] 최종 main SHA를 고정하고 `ci.yml` workflow_dispatch를 **1회** 실행한다. PR CI는 squash 전 결과이므로 exact-main 결과로 바꾸어 기록하지 않는다. main CI 외 별도 전체 감사/PF dispatch를 중복 실행하지 않는다.
- [ ] 후보 시작 전에 v0.9.0 Release와 태그가 없는지 확인한다. 철회본 tag object `f31f111977956bd665cd432accdf54677444027f` / peeled commit `e499ac7127269bf67863bf0fdc42eaf53236b9f3`의 이력과 제거 근거는 보존했다. 제거 완료를 이유로 태그를 다시 만들지 않는다. 예상하지 않은 태그나 Release가 존재하면 덮어쓰거나 삭제하지 않고 원인을 확인한다.
- [ ] 원격 삭제는 현재 확인한 **태그 ref object**를 명시적 lease로 고정한다: `git push --force-with-lease=refs/tags/v0.9.0:<확인한-tag-object-SHA> origin :refs/tags/v0.9.0`. peeled commit을 lease 값으로 쓰지 않는다. 다른 태그/릴리스는 변경하지 않는다. 계획 작성 단계에서는 실행하지 않는다.
- [ ] 태그/Release 부재를 확인하고 exact current main SHA·`candidate_tag=v0.9.0`으로 `Windows package candidate`를 실행한다. 기존 후보의 tag-exists 거부 조건을 비활성화하지 않는다.
- [ ] 한 후보의 assembly·native·격리 WSL2/Docker·interactive installer/UI·migration/update/restore/removal·성능 결과를 연결한다. 기존 job/runner에 필요한 assertion을 통합하고 같은 검사를 독립 workflow로 다시 실행하지 않는다. 모든 packaged evidence의 source/fixture/digest가 일치해야 한다.
- [ ] 실패하면 원인을 해결하고 영향 결과를 보충한다. 제품/package 입력 변경 시 새 후보가 필요한 것은 반복 검증과 구분한다. 같은 실패를 재빌드로 덮지 않으며 만료 후보나 임의 다른 build를 공개하지 않는다.

### Task 16.3 — 태그·공개·정리

- [ ] 후보 성공 후 현 7일 유효기간 안에 **같은 main commit에 annotated `v0.9.0`**을 생성한다. 태그 push로 release workflow를 시작하고 동일 후보의 7개 파일을 재빌드 없이 draft로 승격한다.
- [ ] 기존 release workflow의 fresh-download 이름/크기/SHA-256/component 확인→공개→공개 다운로드 및 네 portable 기본 native 실행 결과를 확인한다. 같은 smoke를 별도 수동 workflow로 반복하지 않는다.
- [ ] 동일 이름 태그 재발행 사실·철회본 대상 commit·수정본 대상 commit/digests·재설치 안내·확인된 제한을 Release notes와 PR에 기록한다. #580에 사용자 테스트를 요청하지 않는다.
- [ ] 임시 설치/fixture/중복 package를 정리하고 필수 배포/검증 증거의 보존 위치를 남긴다. 작업 브랜치/worktree는 clean·통합 확인 뒤 00 §5 순서로 정리한다. 현재 root checkout·활성·dirty·미통합 작업은 보존한다. 최종 worktree/브랜치·디스크 상태를 확인한다.

완료 기준: 계획의 개선·문서 정합성 완료, 필수 수용 FAIL/NOT_RUN/누락 0, 미해결 데이터 손실·오전송·설치 차단 0, 동일 source/digest 후보 검증, 새 v0.9.0 annotated tag와 공개 Release, 다운로드 실행 확인, 불필요한 임시 산출물 정리. 검증 횟수나 테스트 수 자체를 품질 근거로 사용하지 않는다.
