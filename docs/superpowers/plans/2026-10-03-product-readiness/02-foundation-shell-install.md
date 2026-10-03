# 검증 기반·공통 UI·설치 완료 구현 계획

> **For agentic workers:** [00 실행 규칙](00-roadmap.md)과 [01 제품 계약](01-product-contract.md)을 함께 적용한다. 아래 명령/코드는 향후 구현 계약과 RED 시험 예시이며 이미 존재하거나 실행됐다는 뜻이 아니다.

**Goal:** 실제 사용자가 보이는 화면으로 설치를 완료하고, 네 제품의 지원 창 크기에서 핵심 작업을 수행하게 한다.

**Architecture:** R00은 현 CDP/UI Automation 기반을 사용자 입력과 evidence 수집에 연결한다. R06은 shell과 앱 내부 panel의 가시성을 정리한다. R07은 기존 native journal/typed delivery를 원장으로 설치 안내를 구성한다.

**Tech Stack:** 기존 Rust/React/CSS/Node test runner/Windows UI Automation. 신규 브라우저 프레임워크 의존성을 전제로 하지 않는다.

**Spec:** 01 §1/2와 Review Focus 전부. 별도 Global Constraints는 01을 그대로 상속한다.

## R00 — 철회 정리와 실제 UI 검증 기반

제목: `test(suite): establish user-flow evidence and retire withdrawn release policy`

### Task 00.1 — 현재 원장과 기존 로컬 변경 정리

수정 대상:

- `/home/jihoon/projects/devbox/AGENTS.md`, `CONVENTIONS.md`
- `/home/jihoon/projects/devbox/docs/release-policy.md`, `release-evidence.md`, `windows-guide.md`, `codex-setup.md`, `verification.md`
- `/home/jihoon/projects/devbox/docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md`, `p3-01-release-v0.9.0.md`
- 네 `/home/jihoon/projects/devbox/apps/devbox-{workspace,api-studio,knowledge,control-center}/README.md`의 현재 원장 링크
- 기존 미커밋 스킬/전용 metadata 검사 제거 파일들(원 diff만 이동)

- [ ] 시작 시 status/worktree/원격 인증과 원 diff를 확인한다. RegistryGate 테스트 변경은 R07에 보존한다. 사용자 파일을 reset하지 않는다.
- [ ] old roadmap 머리에 `v0.9.0 철회 전 실행 이력`과 새 계획 링크를 적고 과거 결과 본문은 보존한다. source 버전과 공개 상태를 구별한다.
- [ ] release policy에서 닫힌 #580을 현재 기록 장소로 쓰거나 기본 UI 검사를 출시 뒤로 미루는 규칙을 제거한다. 단일 PR·로컬 우선 최소 검증·자원 관리·v0.9.0 태그 재생성 순서를 00/06과 맞춘다. 문서 갱신을 끝까지 미루지 않고 작업과 함께 진행한다.
- [ ] skill 삭제 뒤 전용 checker 경로가 남지 않는지 확인하고 기존 scope/resource regression을 실행한다. 역사적 worktree 이름 `devbox-release-v0.9.0`은 스킬 참조와 구분한다.
- [ ] 이 과제의 문서·삭제 변경만 커밋한다. 계획 폴더/나머지 테스트를 암묵적으로 포함하지 않는다.

검증:

```bash
python3 .github/scripts/test-ci-scope.py
python3 .github/scripts/test-ci-scope-runners.py
python3 .github/scripts/test-verification-resources.py
```

수용: 23건 추적표에 담당 작업이 모두 있고, 기록을 쓰기 위해 닫힌 이슈에 메시지를 보낼 필요가 없다. 원격 Release 삭제·기존 태그의 임시 보존과 최종 재생성 예정·미완료 제품 상태가 문서에서 모순되지 않는다.

### Task 00.2 — UI 입력 driver와 증거 계약

신규 파일:

- `/home/jihoon/projects/devbox/.github/scripts/suite-user-flow-driver.mjs`
- `/home/jihoon/projects/devbox/.github/scripts/suite-user-flow-driver.test.mjs`
- `/home/jihoon/projects/devbox/.github/scripts/suite-user-flow-evidence.mjs`
- `/home/jihoon/projects/devbox/.github/scripts/suite-user-flow-evidence.test.mjs`
- `/home/jihoon/projects/devbox/.github/scripts/suite-user-flow-matrix.json`, `test-suite-user-flow-matrix.mjs` — 06의 필수 ID와 소유 작업을 먼저 고정
- `/home/jihoon/projects/devbox/.github/scripts/windows-installer-ui.ps1`
- `/home/jihoon/projects/devbox/.github/scripts/test-user-flow-config.py`

수정/재사용:

- `/home/jihoon/projects/devbox/.github/scripts/workspace-cdp-fixture.mjs`의 exact target/CDP transport
- `/home/jihoon/projects/devbox/.github/scripts/windows-native-file-dialog.ps1`의 소유 chooser 입력 경계
- `/home/jihoon/projects/devbox/.github/scripts/windows-packaged-smoke.mjs`의 process identity/cleanup 도우미
- `/home/jihoon/projects/devbox/.github/workflows/product-foundation.yml`의 기존 수용 entrypoint·artifact 업로드 재사용(작업별 CI trigger 추가 금지)

생산 코드에 test 전용 권한/전역 변수를 넣지 않는다. driver의 DOM evaluate는 관찰·control 위치 확인에만 쓴다. 동작은 CDP Input pointer/keyboard 또는 native UIA로 보내며 `__TAURI_INTERNALS__.invoke`, 내부 method 호출, renderer state 주입으로 성공 상태를 만들지 않는다.

인터페이스(이 과제에서 구현):

```ts
type UiTarget = { role: string; name: string };
type UiDriver = {
  click(target: UiTarget): Promise<void>;
  fill(target: UiTarget, text: string): Promise<void>;
  press(keys: string): Promise<void>;
  text(target: UiTarget): Promise<string>;
  screenshot(name: string): Promise<string>;
  closeOwnedWindow(): Promise<void>;
};
type ScenarioResult = {
  id: string;
  status: 'PASS' | 'FAIL' | 'NOT_RUN';
  sourceSha: string;
  fixtureSha: string;
  artifactDigests: Record<string, string>;
  evidenceKind: 'browser-fixture' | 'native-boundary' | 'packaged-ui';
  assertions: string[];
  screenshotPaths: string[];
  failureCode: string | null;
};
```

- [ ] RED: driver 테스트의 fake CDP에서 `click`은 좌표 입력을 보내고 mutation용 `Runtime.evaluate`를 보내지 않는지 검사한다. 동일 accessible name이 2개이면 모호성 오류로 중단하며 임의 첫 항목을 누르지 않는다.
- [ ] RED: evidence validator가 source 불일치·필수 ID 누락·NOT_RUN·다른 후보 digest·native-boundary를 packaged-ui로 대체한 결과를 모두 거부하도록 시험한다.

```js
assert.throws(() => requireCompleteEvidence({
  expectedSource: 'a'.repeat(40), requiredIds: ['INSTALL-01'],
  results: [{ ...passingFixture, id: 'INSTALL-01', status: 'NOT_RUN' }],
}));
assert.throws(() => requireCompleteEvidence({
  expectedSource: 'a'.repeat(40), requiredIds: ['INSTALL-01'],
  results: [{ ...passingFixture, id: 'INSTALL-01', evidenceKind: 'native-boundary' }],
}));
```

`passingFixture`는 테스트 파일에서 위 ScenarioResult 전 필드를 채운 합성 객체로 정의하고 `sourceSha/fixtureSha`와 SHA-256 digests를 고정한다. `requireCompleteEvidence`는 같은 모듈에서 구현할 validator다.

- [ ] driver·validator를 구현하고 테스트 GREEN을 확인한다. timeout은 단계 실패로 남기며 해당 동작을 자동으로 다시 클릭하지 않는다.
- [ ] 로컬 Windows/독립 Windows VM 세션에서 exact owned Control Center의 비파괴 toolbar 열기/닫기와 screenshot을 수행한다. 소유 창을 찾지 못하거나 interactive desktop이 없으면 NOT_RUN이며 최종 Windows 수용에서 보충한다. 이 환경 제약만으로 다른 로컬 구현을 대기시키지 않는다.
- [ ] installer UIA wrapper는 `/S` 없는 설치의 다음/취소 control을 소유 PID/실행파일로 제한한다. foreground 실패를 global input으로 우회하지 않는다. 실패 시 owned artifact만 정리하고 로그/스크린샷을 보존한다.
- [ ] matrix에는 06의 필수 ID와 소유 작업을 모두 등록한다. 아직 구현 전인 시나리오는 NOT_RUN으로 남기고 제품 ready를 false로 유지한다. R00 개발 완료는 driver/validator의 최소 양·음성 회귀로 판단하며 Windows 입력 smoke는 로컬에서 가능하면 실행하고 불가능하면 최종 격리 수용에 배정한다. 최종 출시 전에 그 결과를 반드시 확보하고, 전체 ready gate는 R15에서 candidate에 연결한다.
- [ ] 변경한 CI 설정의 좁은 config test만 로컬에서 수행한다. 전체 검증은 최종 통합 뒤 한 번 모은다. 다른 작업의 미구현 시나리오를 fake PASS로 등록하지 않는다.

좁은 명령:

```bash
node --test .github/scripts/suite-user-flow-driver.test.mjs .github/scripts/suite-user-flow-evidence.test.mjs
python3 .github/scripts/test-user-flow-config.py
```

matrix row 계약은 `{id, ownerWorkItem, products, evidenceKind, module}`이다. R00에서 모든 ID/owner/module 예정 경로를 선언하고 config 검사는 문법·고유성·소유권만 확인한다. 로컬 작업 실행은 `--owner-work-item R04`처럼 정확한 subset을 선택하여 그 module의 존재/반환 ID/결과를 요구한다. 미구현 후속 module은 앞 작업의 PASS에 합산하지 않는다. release 실행은 `--all-required`로 전체 row를 요구하며 누락/NOT_RUN을 거부한다. subset 결과의 ready는 `scope='work-item:R04'`로 표시하고 `scope='release'`와 혼용하지 않는다. 이 CLI와 schema를 R00 evidence 모듈의 테스트에 고정한다.

R00 완료는 모든 제품의 UX 통과가 아니라 **실제 입력·관찰·실패 기록·정리 능력**의 확보다. 이후 작업은 이 driver에 자신의 시나리오 파일을 추가한다.

## R06 — 공통 shell·지원 창 크기·상태 피드백

제목: `fix(suite): make all product states usable across supported windows`

선행: R01/R02/R04. Notes/Files/Requests의 수명·송신 변경 후 UI refactor가 이 동작을 다시 깨뜨리지 않도록 검증한다.

### Task 06.1 — 명시적 shell 배치와 availability

수정:

- `/home/jihoon/projects/devbox/packages/product-shell/src/index.tsx`, `styles.css`, `api.ts`, `shell.test.tsx`
- `/home/jihoon/projects/devbox/packages/product-shell/src/AgentStatus.tsx`, `AgentStatus.test.tsx`
- `/home/jihoon/projects/devbox/packages/product-shell/src/IncomingCommands.tsx`, `SuiteConnection.tsx`, `SuiteConnection.test.tsx`

신규:

- `/home/jihoon/projects/devbox/packages/product-shell/src/availability.ts`, `availability.test.ts`
- `/home/jihoon/projects/devbox/packages/product-shell/src/IncomingCommands.test.tsx`
- `/home/jihoon/projects/devbox/.github/scripts/browser-product-layout.mjs` 및 `/home/jihoon/projects/devbox/.github/scripts/browser-product-layout.test.mjs`

`availability.ts`의 `deriveAvailability(description: Description)`는 `{ data: boolean, incoming: boolean, reconnect: boolean, setupRequired: boolean }`를 반환한다. native 권한을 새로 부여하지 않고 기존 Description의 supported/delivery 상태에서 UI 허용 여부만 유도한다. 내부 unknown 상태는 enabled로 추정하지 않는다.

- [ ] RED: direct/committed/import/health/recover/unsupported를 네 제품 fixture로 렌더한다. import/health에는 pending 조회와 agent reconnect 호출 0회, setup 안내는 존재해야 한다.
- [ ] RED: 조회 1회 실패→다음 최신 refresh 성공 후 해당 조회 error는 없어지고 최신 데이터가 보인다. 늦은 이전 실패가 현재 성공을 덮지 못한다. unrelated write 실패는 남는다.
- [ ] 명시적 grid 영역 `header/notice/nav/main`을 적용한다. notice가 없을 때도 main/nav 배치가 동일해야 한다. `<p>`의 자동 배치 순서에 의존하지 않는다.
- [ ] Agent 상태에서 starting/restarting/paused-by-setup/unavailable을 다르게 표시하고 상태 회복 시 읽기 오류를 해제한다. 비허용 단계에 재연결 버튼을 남기지 않는다.
- [ ] 브라우저에서 각 main의 위치·폭과 focusable control의 viewport 교차를 검사한다. screenshot은 assertion의 보조 증거다.

```js
assert.ok(main.width >= 320);
assert.ok(main.x >= nav.x + nav.width || navigationIsDrawer);
assert.ok(main.y >= notice.y + notice.height || noticeAbsent);
assert.equal(clippedPrimaryControls.length, 0);
```

위 값은 `browser-product-layout.mjs`에서 실제 bounding rect와 computed visibility로 계산한다. mock DOM의 layout 수치로 통과시키지 않는다.

```bash
pnpm --filter @devbox/product-shell exec vitest run src/shell.test.tsx src/availability.test.ts src/AgentStatus.test.tsx src/IncomingCommands.test.tsx src/SuiteConnection.test.tsx
node --test .github/scripts/browser-product-layout.test.mjs
```

### Task 06.2 — 내부 panel·toolbar·표·dialog의 제품별 정리

수정 대상(각 앱의 실제 route를 따라 필요한 영역만 변경):

- `/home/jihoon/projects/devbox/apps/devbox-{workspace,api-studio,knowledge,control-center}/src/App.css`
- `/home/jihoon/projects/devbox/packages/knowledge-features/src/notes/App.css`, `App.tsx`
- `/home/jihoon/projects/devbox/packages/workspace-features/src/files/App.css`, Source/Tasks의 기존 CSS와 toolbar component
- `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/App.css`, RequestSidebar/ProtocolLab/GrpcLab의 toolbar
- `/home/jihoon/projects/devbox/apps/devbox-control-center/src/{Inventory,Health,Restore,Updates}.tsx`

추가 개선은 [01 §2](01-product-contract.md)의 최소 크기, 고정 primary action, 빈/실패 상태, 키보드 계약으로 한정한다. 색상 팔레트 전면 교체나 새 디자인 의존성은 추가하지 않는다.

- [ ] RED: 720×480 노트 기본 sidebar+backlinks 열린 상태에서 편집 폭/toolbar 가시성 검사. 현 33px 경계를 잡는다.
- [ ] RED: 모든 제품에서 실제 대표 편집 화면·표·dialog를 720×480/기본 크기로 연다. 저장/전송/취소 control 도달, modal focus trap/복귀, 200% 텍스트 확대를 검사한다.
- [ ] 공간이 부족하면 secondary panel을 drawer로 접고 토글에 accessible name/pressed 상태를 준다. 전환이 editor를 remount하여 draft를 잃지 않아야 한다.
- [ ] 표의 내부 스크롤, toolbar overflow menu, primary/secondary action 구분, loading/empty/error의 다음 행동을 구현한다.
- [ ] 각 페이지에서 키보드로 주요 행동을 수행하고 한국어 IME 조합 중 단축키가 실행되지 않는 것을 확인한다. R01/R02의 draft 전환 회귀를 다시 실행한다.

수용: 네 제품×상태×대표 창 크기 캡처 목록이 완전하고 잘린 핵심 control 0개. Notes 편집 폭≥320px, keyboard-only 작업 완료. 기능 상태 테스트와 실제 Chromium layout 검사 둘 다 통과. Windows WebView2에서 최소·기본 창과 배율 조합을 별도로 확인한다.

## R07 — 설치 준비부터 사용 가능까지 한 흐름

제목: `fix(control-center): guide and resume complete suite activation`

선행: R03/R05/R06. Workspace close/context 보호 없이 자동 닫기를 추가하지 않는다.

### Task 07.1 — 준비 전용 권한과 UI 계약 일치

수정:

- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/RegistryGate.tsx`, `RegistryGate.test.tsx`
- `/home/jihoon/projects/devbox/apps/devbox-control-center/src/Content.tsx`, `Health.tsx`, `Recovery.tsx`, `Restore.tsx`
- `/home/jihoon/projects/devbox/apps/devbox-control-center/src-tauri/src/ipc/delivery.rs`
- `/home/jihoon/projects/devbox/crates/product-shell-tauri/src/admission.rs`의 회귀만 추가(권한 완화는 기본 해법 아님)

신규 테스트: `/home/jihoon/projects/devbox/apps/devbox-control-center/src/Health.test.tsx`.

- [ ] 기존 로컬 RED 2개를 실제 native contract fixture로 재확인한다. setupOnly에서는 status/start_empty 성공, registry.snapshot은 거절한다.
- [ ] 준비 전용 selected는 registry를 읽지 않고 완료로 게시한다. 이미 준비된 상태의 재실행도 start_empty 재호출 없이 안내를 표시한다. 일반 committed 모드에서는 snapshot 완료 후 ready 계약을 유지한다.
- [ ] products에서 준비 미완료이면 실제 `recovery` route로 안내/이동하여 SetupFlow를 렌더한다. 화면 route와 header가 다르게 보이는 위조로 해결하지 않는다. products의 금지된 record 버튼은 남지 않는다.
- [ ] Health의 상태 기록 완료가 상위 restore inventory 재조회로 이어지게 `onRecorded: () => Promise<void>`를 연결한다. 완료 이전에 다음 단계 버튼을 활성화하지 않는다.

```ts
expect(registryCall).not.toHaveBeenCalled(); // setupOnly selected
expect(setupCall).not.toHaveBeenCalledWith('start_empty'); // already prepared
expect(recordedHeaders.every(h => h.route === visibleRoute)).toBe(true);
expect(await screen.findByRole('button', {name: '다음 단계'})).toBeEnabled();
```

마지막 assertion은 준비 기록 후 부모 snapshot을 갱신한 실제 SetupFlow integration에서 사용한다. `visibleRoute`는 테스트의 실제 navigation state다.

```bash
pnpm --filter devbox-workspace exec vitest run src/RegistryGate.test.tsx
pnpm --filter devbox-control-center exec vitest run src/Health.test.tsx src/App.test.tsx src/Restore.test.tsx
cargo test -p devbox-control-center --lib ipc::delivery::tests
cargo test -p product-shell-tauri --lib admission
```

### Task 07.2 — native 원장 기반 SetupFlow와 재개

신규:

- `/home/jihoon/projects/devbox/apps/devbox-control-center/src/SetupFlow.tsx`, `SetupFlow.test.tsx`
- `/home/jihoon/projects/devbox/apps/devbox-control-center/src/setupFlow.ts`, `setupFlow.test.ts`
- `/home/jihoon/projects/devbox/.github/scripts/windows-suite-user-flow.mjs`

수정:

- 기존 Content/Health/Recovery/Restore/delivery wrapper
- `/home/jihoon/projects/devbox/apps/devbox-control-center/src-tauri/src/bootstrap/interactive.rs`, `bootstrap.rs`
- `/home/jihoon/projects/devbox/.github/scripts/build-suite-installer.py`
- `/home/jihoon/projects/devbox/docs/windows-guide.md`, 제품 README

renderer 계약:

```ts
type SetupStage = 'prepareStores' | 'review' | 'activating' | 'health' | 'committing' | 'complete' | 'recover';
type SetupView = {
  stage: SetupStage;
  next: 'openOwner' | 'recordHealth' | 'activateClean' | 'commitClean' | 'resume' | null;
  blockedReason: string | null;
};
// RestoreInventory/RecoveryStatus는 기존 generated type을 import한다.
function deriveSetupView(inventory: RestoreInventory, recovery: RecoveryStatus): SetupView;
```

`next`는 renderer 안내용이며 native admission/receipt를 대신하지 않는다. update/reinstall/data-restore는 각 기존 action을 사용하는 별도 분기이며 clean 설치에 잘못 합치지 않는다. 제품별 준비 결과는 기존 health report의 owner/setupSelected/busy/reviewRequired를 표시한다.

- [ ] RED: 준비 3/4, 4/4 but stale health, busy owner, recover, committed 조합의 다음 행동을 표로 검사한다. 잘못된 조합은 next=null과 이유를 보여야 한다.
- [ ] SetupFlow가 owner 열기→선택 완료 확인→상태 기록→helper 전환→재시작 후 재개를 안내하도록 구현한다. 제품 실행은 검증된 bootstrap의 고정 product id만 사용한다. 임의 executable/path를 renderer가 보내지 않는다.
- [ ] 자동화가 필요한 helper 재시작은 기존 journal/action receipt와 installation key/generation에 묶인다. accepted는 성공 완료가 아니다. UI는 해당 단계의 실제 결과를 다시 읽고 나서 진행한다.
- [ ] 제품을 닫아야 할 때 강제 process kill이나 새 범용 close RPC를 만들지 않는다. 현재 화면에서 소유 제품의 정상 종료 검토를 안내하고, native가 종료를 확인하면 helper가 자동으로 다음 상태를 진행한다. 미저장 취소한 제품은 blocker로 남고 진행하지 않는다. 자동 close 협업은 R02의 handshake를 사용하는 한정된 제품 adapter로만 추가할 수 있다.
- [ ] installer Finish는 Control Center 안내에 연결한다. 사용 준비 전 ‘설치 완료’와 ‘일반 작업 사용 가능’을 혼동하지 않도록 문구를 구분한다.
- [ ] staged helper 실패·프로세스 종료 중단·Control Center 재열기·expiry에서 복구 UI가 같은 설치를 이어가는지 검사한다. UI 새로고침으로 native 실패 기록을 지우지 않는다.

좁은 명령:

```bash
pnpm --filter devbox-control-center exec vitest run src/setupFlow.test.ts src/SetupFlow.test.tsx src/Health.test.tsx src/Restore.test.tsx
cargo test -p devbox-control-center --lib core::delivery
python3 .github/scripts/test-build-suite-installer.py
```

기존 `test-build-suite-installer.py`에 NSIS Finish 연결/문구와 artifact contract 회귀를 추가한다.

Windows 수용: 새 임시 설치에서 installer UI→Control Center→네 제품 준비→활성화→재실행→실행 확인→commit→각 제품 첫 작업. driver는 visible input만 쓰고 기록 API/helper를 직접 호출하지 않는다. 기존 silent/helper suite는 별도 native-boundary 검사로 유지한다. 중간 취소/재실행·한 제품 불응답·실패 뒤 retry·dirty 종료 취소도 같은 후보로 확인한다.

R07 완료 기준: 설치 과정을 개발자 지식이나 콘솔 명령 없이 마칠 수 있고, 임의 route/기존 store 재생성 오류가 없다. 설치 안내의 다음 행동은 실제로 실행 가능하며 사용자가 다수의 화면에서 단계 순서를 추측하지 않아도 된다.
