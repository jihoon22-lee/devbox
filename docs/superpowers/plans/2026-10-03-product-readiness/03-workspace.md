# Workspace 후속 실행계획 제안 — 2026-10-03

> **For agentic workers:** [00 실행 규칙](00-roadmap.md)·[01 제품 계약](01-product-contract.md)·[06 수용 매트릭스](06-acceptance-release.md)를 함께 적용한다. 선행/파일 배정 충돌 시 00의 통합 표가 우선한다.

**Goal:** Workspace 초안·context·registry·실행 복구를 완성한다.

**Architecture:** 기존 제품/native 소유권·revision·typed IPC를 유지하고 실제 UI 전이를 검증한다. 신규 제안 파일/테스트는 아래 과제에서 생성한다.

**Tech Stack:** 현 Rust·React 19·TypeScript·pnpm 9·CSS·Windows WebView2.

**Spec:** 01. **Global Constraints / Review Focus:** 01의 전체 제약과 5개 검토 관점을 상속한다.

**과제 실행 순서:** 각 하위 과제에서 (1) 명시한 assertion의 RED 작성, (2) 실제 실패 원인 확인, (3) 아래 계약에 맞춘 최소 구현, (4) 동일 좁은 검사 GREEN, (5) 과제 커밋. 명령은 실행 계획이며 현재 PASS 기록이 아니다. 직접 pnpm/cargo 명령은 반드시 `python3 .github/scripts/verify-resources.py --` 아래에서 실행한다. 통합 완료 후 최종 검증은 00을 따른다. 작업마다 전체 검증·CI를 반복하지 않는다.


계획만 작성했다. 제품/테스트 코드 변경·빌드·테스트·서비스 조작은 하지 않았다. 기준 감사는 [Workspace 감사](/home/jihoon/projects/devbox-audit-2026-10-03/devbox-audit-workspace.md), 추가 범위는 [추가 감사](/home/jihoon/projects/devbox-audit-2026-10-03/devbox-audit-workspace-extra.md)다. 설치 활성화 W0(R07)와 공통 shell grid·반응형(R06)는 부모 담당으로 이 계획에서 제외한다. 아래 명령은 향후 실행할 명령이며 현재 PASS 근거가 아니다.

## 분류와 작업 경계

| 분류 | 항목 | 처리 |
|---|---|---|
| 이미 확정된 결함 W1 | Files에 recovery reader/API만 있고 편집→writer 연결 없음; 정상 창 종료도 즉시 native drain 진입 | R02 |
| 이미 확정된 결함 W2 | AgentHub context 전환이 Workspace dirty/busy guard를 우회하여 Source 로컬 초안 소실 | R02 |
| 이미 확정된 결함 W3 | Agent worktree 등록/정리 후 Workspace registry snapshot이 갱신되지 않음 | R03 |
| 새 확정 결함 | 이번 계획 검토에서 추가하지 않음 | 불확실 후보를 결함 수에 합산하지 않음 |
| 개선 제안 | Tasks 요청 복구에서 대상·동작·다음 행동을 명료화; Terminal 실패 후 상태 확인과 재시도 분리; Dependencies 재분석/재승인 동선 | R14 |
| 검증 보강 | 실제 Windows 종료/복구, Agent 응답 유실, WSL task/process, 터미널 재연결, provider 실패/lock 변경 | 아래 수용 시나리오 |

3개는 독립적으로 설명·리뷰·회귀할 수 있는 내부 작업이다. **R02 변경을 통합 개발 브랜치에 반영한 뒤 R03을 시작**한다. 별도 PR이나 작업별 CI는 만들지 않는다. Workspace.tsx/AgentHub.tsx/agentFlow 계약을 공유하므로 동시에 수정하지 않는다. R14는 코드상 일부 병행 가능하더라도 실행은 통합 표의 R03/R06 선행을 따른다. 무거운 검증은 같은 자원 잠금 아래 직렬화한다. R07과 R02는 RegistryGate.tsx를 공유하므로 해당 파일 변경은 순차 적용한다. R07은 설치본 end-to-end 수용의 선행이며 R02 순수 writer/guard 개발의 논리적 선행은 아니다. 신규 의존성·앱/crate 추출·릴리스 버전 변경 없음.

## R02 — Files 복구 기록과 공통 전환·종료 보호

제안 제목: `fix(devbox-workspace): persist dirty buffers and guard context transitions`

### 근거와 완료 행동

- `/home/jihoon/projects/devbox/apps/devbox-workspace/src-tauri/src/component.rs:349`는 main CloseRequested를 곧바로 app.exit로 연결한다. `/home/jihoon/projects/devbox/packages/workspace-features/src/files/App.tsx:1438`는 문서 목록만 session에 저장한다. recovery API는 `/home/jihoon/projects/devbox/packages/workspace-features/src/files/api.ts:521`에 있으나 생산 caller가 없다.
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/Workspace.tsx:201`의 guard는 RegistryGate에만 전달되고 `:288`의 AgentHub는 보호 상태를 받지 않는다. Source의 commit/conflict/PR 초안은 component state다. 본 작업에서 Source 보호는 **dirty 상태에서 context 전환 및 종료를 막고, 편집 화면으로 돌아가 완료/명시 폐기하게 하는 것**이다. Source 초안을 디스크에 영구 저장하거나 crash 후 복원하는 새 제품 기능은 이 작업의 완료 주장에 포함하지 않는다.
- 경로 이동 자체는 허용해 hidden Source/Files state를 유지한다. context를 바꾸는 native 호출 전에 동일한 guard를 검사한다. 변경된 context를 뒤늦게 원상 복구하는 방식은 사용하지 않는다.

### 변경/신규 파일과 소유권

필수 변경:

- `/home/jihoon/projects/devbox/packages/workspace-features/src/files/App.tsx`
- `/home/jihoon/projects/devbox/packages/workspace-features/src/files/api.ts`
- `/home/jihoon/projects/devbox/packages/workspace-features/src/files/hooks/useFileActions.ts`
- `/home/jihoon/projects/devbox/packages/workspace-features/src/files/components/RecoveryDialog.tsx`
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/Workspace.tsx`
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/RegistryGate.tsx`
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/agents/AgentHub.tsx`
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/agents/agentFlow.ts`
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/agents/api.ts`
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src-tauri/src/component.rs`
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src-tauri/src/core/mod.rs`

신규 제안:

- `/home/jihoon/projects/devbox/packages/workspace-features/src/files/recoveryWriter.ts` — context에 묶인 단일 writer/flush 상태기계. UI component에 debounce·CAS 충돌 처리를 분산하지 않는다.
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/contextTransition.ts` — Workspace 소유 dirty/busy 사유와 전환 임계구역. ref 기반 최신 상태 확인, 전환 중 중복 차단, 실제 native select 바로 전 재확인.
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/CloseReview.tsx` — 종료 검토·저장 상태·취소. Source 초안이 남으면 편집 화면으로 돌아갈 수 있게 한다.
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src-tauri/src/core/close_review.rs` — request generation/nonce, 검토 취소·승인·drain 진입을 구분하는 순수 상태기계. 종료 검토 중에는 Files 저장 IPC를 계속 허용한다.

종료 handshake는 제품 소유의 기존 `workspace.commands` typed component에 한정하여 `prepare_close`/`confirm_close`/`cancel_close` 계약을 추가하는 안을 권장한다. Files command에 앱 종료 권한을 넣지 않는다. 실제 native 닫기/ExitRequested는 pending close를 만들고 renderer에 검토를 요청하며, 현재 request에 대한 confirm만 기존 native drain으로 넘어간다. main window 이외에는 이 명령을 허용하지 않는다. 미응답 timeout을 승인으로 해석하지 않는다. 등록 전/설치 준비 화면에서는 문서 편집이 없으므로 기존 종료 가능성을 보존하고 W0의 import admission을 넓히지 않는다.

이 계약을 선택할 때 함께 수정할 정확한 파일:

- `/home/jihoon/projects/devbox/apps/devbox-workspace/src-tauri/src/ipc/commands.rs`, `ipc/mod.rs`, `ipc/allow_table.rs`, `ipc/deadlines.rs`
- `/home/jihoon/projects/devbox/packages/workspace-features/src/generated/CommandsCall.ts`, `commands-results.ts`, `deadline-budgets.ts` (Rust 원장에서 재생성; 직접 수정 금지)
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/native.ts`는 기존 typed 호출을 사용하며 별도 범용 invoke 우회 추가 금지. 현재 plugin handler/build command/권한에 commands가 등록되어 있어 새 광역 capability는 필요하지 않다.

복구 소유권은 Files writer→기존 FilesHost→MetadataRoot를 유지한다. browser localStorage에 파일 본문을 넣지 않는다. native mirror의 hash를 recovery text로 오인하지 않는다. `loadRecoveryState()`로 nativeRevision을 받으며 save/discard를 한 큐에서 CAS 순서대로 실행한다. writer는 시작 시 context와 generation을 고정하고 전환 완료 전 flush한다. 이전 context의 늦은 완료가 새 context의 revision을 갱신해서는 안 된다. 현재 transport가 호출 때마다 최신 context를 읽는 구조를 고려해, 전환 guard가 flush 동안 native select 자체를 보류하도록 한다.

복구 계약 세부:

1. dirty 판정은 빈 문자열 여부가 아니라 editor snapshot 기준. 전체 내용을 지운 dirty 문서도 보존한다. 기존 `core/recovery.rs::should_snapshot`의 nonempty 조건을 그대로 재사용하지 않는다.
2. 유휴 시 debounce 기록, 정상 종료 요청 때 마지막 generation flush, 기록 성공/실패 상태를 UI에서 구분한다. 정상 종료는 저장·명시 폐기·취소 중 결정하며, 기록 실패를 성공으로 간주해 닫지 않는다.
3. saveFile가 성공하더라도 해당 save snapshot 이후 추가 편집이 있으면 새 recovery를 남긴다. 저장 실패 또는 외부 변경 충돌 때 recovery를 지우지 않는다. 명시적 문서 폐기만 해당 항목 제거.
4. CAS 실패는 최신 revision을 다시 읽고 다른 문서 entry를 보존한 병합으로 처리한다. 같은 문서의 더 최신 recovery가 있으면 사용자에게 충돌을 알리고 덮어쓰지 않는다. 무한 자동 재시도 금지.
5. 현재 native `StoredRecovery.merge`는 한도 초과 시 전체 실패하고 기존 내용을 유지한다. 이 성질을 유지한다. 큰 문서/총량 초과는 복구 가능 표시를 하지 않고 일반 저장 또는 취소로 해결하게 한다.
6. encoding/lineEnding 복구는 **필수**다. RecoveryEntry에 기존 core::encoding::Encoding과 core::line_ending::LineEnding을 각각 `encoding: Option<Encoding>`와 `line_ending: Option<LineEnding>` 필드(각각 `#[serde(default)]`)로 추가한다. 옛 entry에 필드가 없으면 원본 metadata를 재확인해 적용하고, 원본도 없으면 사용자에게 encoding을 명시 선택하게 한다. 이 변경에서 `/home/jihoon/projects/devbox/crates/editor-engine/src/core/recovery.rs`, `/home/jihoon/projects/devbox/apps/devbox-workspace/src-tauri/src/core/editor_recovery.rs`, `files_host.rs`, `ipc/files.rs`와 생성된 RecoveryEntry/FilesRecovery 계약을 함께 갱신한다. 옛 recovery fixture를 읽을 수 있어야 한다. 저장된 metadata가 있는 항목은 recovery UI와 documentStore에 그대로 적용한다. 문자열 내용만 복구하면서 encoding까지 복구했다고 표시하지 않는다.

공통 guard는 Files dirty/write-pending, Tasks dirty, Source dirty/busy, Definitions editing, Dependencies preview/execution을 설명 가능한 사유로 집계한다. RegistryGate의 select/clear/remove-current와 Agent 생성/resume/review/PR/reopen/base 이동/merge/cleanup의 **context를 바꾸는 모든 경로**가 이 coordinator를 거친다. Agent 부수효과 시작 전에도 검사하여 dirty 상태에서 worktree만 만든 뒤 막히는 결과를 피한다. RegistryGate 내부의 WSL 등록 폼이 선택을 직접 수행하는지도 같은 테스트에서 확인한다.

### RED 회귀와 좁은 실행

변경 테스트: Files `App.test.tsx`, `api.test.ts`; Workspace `Workspace.test.tsx`, `RegistryGate.test.tsx`, `agents/AgentHub.test.tsx`, `agents/agentFlow.test.ts`; native `files_host.rs` 내부 tests 및 `ipc/mod.rs` tests.

신규 테스트: `/home/jihoon/projects/devbox/packages/workspace-features/src/files/recoveryWriter.test.ts`, `/home/jihoon/projects/devbox/apps/devbox-workspace/src/contextTransition.test.ts`, `/home/jihoon/projects/devbox/apps/devbox-workspace/src/CloseReview.test.tsx`, `/home/jihoon/projects/devbox/apps/devbox-workspace/src/WorkspaceDraftGuard.test.tsx`, `core/close_review.rs` 내부 tests. WorkspaceDraftGuard는 실제 Source/Files 하위 component를 렌더하고 native boundary만 fixture로 대체한다. 기존 Workspace.test.tsx의 ready-only Registry/Source mock만으로 초안 보존을 증명하지 않는다.

실패 assertion 예시:

- fixture에 recovery를 미리 넣지 않고 open→edit→advanceTimers→`expect(saveRecovery).toHaveBeenCalledWith([{path, content: editedText, ...}], initialRevision)`; remount는 writer가 실제 기록한 fake native store를 읽어 편집 내용 표시.
- text가 그대로인 encoding-only/BOM-only/lineEnding-only dirty→recovery write→owned crash→재열기: 본문과 metadata가 모두 복원됨. 옛 optional field 없는 recovery도 읽히며 원본 metadata를 사용하고, 원본 없는 경우 임의 encoding 자동 저장 없음.
- edit→save 시작→다시 edit→save 첫 응답: 새 text recovery가 남음; `discardRecovery`가 최신 초안을 지우지 않음.
- save_recovery rejection 또는 한도 초과→CloseReview에서 confirm 호출 0회, 창 유지; flush 중 추가 edit→마지막 generation 확인 전 confirm 0회.
- Source commit/conflict/PR draft→Agents의 다른 worktree review: native select 0회, 기존 draft text 그대로, 원래 context 그대로. dirty를 해제하면 선택 1회.
- dirty 변경이 preview await 중 발생하면 최종 native select 0회. clean context 전환 동안 새 dirty 발생을 막거나 전환 보류; stale 완료는 무시.
- native confirm의 old nonce/다른 window/context generation→거부; 취소 후 retry→새 nonce; 승인 전 `shutdown_started == false`, 승인 후 기존 drain 한 번.

향후 좁은 명령:

```sh
python3 .github/scripts/verify-resources.py -- pnpm --filter @devbox/workspace-features exec vitest run src/files/recoveryWriter.test.ts src/files/App.test.tsx src/files/api.test.ts
python3 .github/scripts/verify-resources.py -- pnpm --filter devbox-workspace exec vitest run src/contextTransition.test.ts src/CloseReview.test.tsx src/WorkspaceDraftGuard.test.tsx src/Workspace.test.tsx src/RegistryGate.test.tsx src/agents/AgentHub.test.tsx src/agents/agentFlow.test.ts
python3 .github/scripts/verify-resources.py -- cargo test -p devbox-workspace --lib core::close_review
python3 .github/scripts/verify-resources.py -- cargo test -p devbox-workspace --lib files_host::tests
python3 .github/scripts/verify-resources.py -- cargo test -p devbox-workspace --lib ipc::tests
```

Rust는 WSL에서 먼저 `source ~/.cargo/env`; Windows cfg 테스트는 Windows 같은 revision에서 별도 실행. R02의 필수 schema 확장에 대해 `cargo test -p devbox-editor-engine --lib core::recovery`와 Workspace editor_recovery 호환성 회귀를 supervisor 아래에서 실행한다.

### Windows/WSL 수용기준

임시 fixture 파일에서 한글·빈 내용·CRLF·encoding 변경 편집→정상 X→취소하면 그대로 편집 가능; 저장 성공 후 종료/재실행하면 새 내용; 저장 실패이면 종료되지 않음. writer가 기록 완료를 표시한 뒤 테스트 전용 Workspace 프로세스 강제 종료→재실행하면 마지막 확인된 dirty snapshot 복구. 기록 전 강제 종료에서 마지막 키 입력까지 보장한다고 주장하지 않는다.

Windows local root와 이미 준비된 격리 WSL root 각각 시행한다. WSL helper 단절 시 disk save 실패가 초안 폐기로 이어지지 않아야 한다. 두 worktree를 먼저 등록하고 Source 커밋 메시지/충돌 결과/PR 초안별로 Registry와 Agents의 모든 전환 동작을 반복해 원문 보존 확인. 실제 보조 terminal 창이 열려 있어도 main 종료 검토가 먼저 나오고 취소 시 terminal을 중단하지 않아야 한다.

## R03 — Agents registry/context 갱신의 단일 완료 경계

제안 제목: `fix(devbox-workspace): reconcile registry after agent worktree changes`

근거: `/home/jihoon/projects/devbox/apps/devbox-workspace/src/agents/agentFlow.ts:87`은 apply 후 context만 갱신한다. `/home/jihoon/projects/devbox/apps/devbox-workspace/src/Workspace.tsx:108`의 selectedTree와 AgentHub worktreeContext는 이전 snapshot을 계속 읽는다. 단순 setTimeout, 화면 reload, 기존 refreshSignal만 증가시키고 완료를 기다리지 않는 해결은 제외한다.

변경 파일:

- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/Workspace.tsx`
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/RegistryGate.tsx`
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/agents/AgentHub.tsx`
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/agents/agentFlow.ts`
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/agents/api.ts`
- 위 파일들의 기존 `.test.tsx`/`.test.ts`

신규:

- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/registryProjection.ts`
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/registryProjection.test.ts`

Workspace가 canonical Registry projection을 소유하고 `refreshRegistry(): Promise<Registry>`를 제공한다. RegistryGate는 자체 등록/rename/remove 결과를 같은 publisher로 보낸다. AgentFlow의 register/bind/select/cleanup 후 registry refresh와 context refresh를 하나의 사용자 operation 완료 조건으로 묶는다. operation에서 반환받은 snapshot으로 즉시 worktree를 해석하며 React state가 다음 render로 반영될 때까지 오래된 closure를 재사용하지 않는다. 선택된 context와 대응 snapshot이 아직 일치하지 않을 때 Source/Dependencies는 '동기화 중/다시 확인' 상태를 표시하고 잘못된 '프로젝트를 선택' 메시지를 내지 않는다.

native registry 저장을 renderer registry로 대체하지 않는다. Native expected context/revision 검증과 R02 guard를 그대로 사용한다. mutation 성공 후 refresh 실패이면 성공한 registration을 다시 실행하지 않고 읽기 refresh만 재시도한다. 오래된 load 응답이 더 최신 registry revision을 덮어쓰지 못하게 한다. incoming/external 변경 이벤트는 필요한 경우 동일한 refresh 함수로만 합류하며, 이번 W3 해결에 broad global 이벤트 설계 변경은 필요하지 않다.

RED assertion:

- 초기 fixture worktree w1만→새 task 등록 w2→registry snapshot w1,w2→선택 w2: 수동 새로고침 없이 Source root w2, Dependencies root w2, task review/reopen 성공.
- cleanup w2→native snapshot w1→목록/lookup에서 w2 없음. 제거된 current context는 native 확정 결과에 따라 처리하며 renderer 임의 선택 금지.
- registration 성공→snapshot 한 번 실패→재시도: apply_registration 1회 유지, snapshot만 2회 이상, terminal open 중복 없음.
- revision 12 응답 후 늦은 revision 11 응답→UI 12 유지. 기존 dirty가 있는 동안 mutation 및 context 전환은 R02 규칙에 따라 차단.

```sh
python3 .github/scripts/verify-resources.py -- pnpm --filter devbox-workspace exec vitest run src/registryProjection.test.ts src/Workspace.test.tsx src/RegistryGate.test.tsx src/agents/AgentHub.test.tsx src/agents/agentFlow.test.ts
```

수용: 격리 WSL의 임시 Git repository에서 실제 task create→Source/Dependencies→review/reopen→정리. 처음에 base만 등록해야 한다(기존 테스트처럼 처음부터 w2를 fixture에 넣으면 W3를 검증하지 못함). 중간 app 종료/reopen에서도 등록된 worktree를 다시 만들지 않고 이어서 확인. 기존 unrelated worktree가 삭제되지 않음. Windows에서는 Source/Registry 일반 linked worktree 등록·선택 회귀를 확인하고 WSL-only Agent 동작을 Windows-native 대상으로 허용하지 않음.

선행: R02 변경의 통합 브랜치 반영. R03 단독 L4는 사전 준비한 소유 namespace에서 actual UI로 수행한다. 새 설치부터 시작하는 L5는 R07 뒤 R15에서 연결한다. native registry/engine 변경을 기본 범위에 넣지 않으며 새 backend 결함이 재현되면 별도 증거와 회귀를 먼저 제시한다.

## R14 — 실사용 실패 복구 동선과 실행 수용 보강

제안 제목: `feat(devbox-workspace): clarify runtime recovery and dependency refresh actions`

이 작업은 새로 확정한 데이터 손실 결함 수정이 아니라 사용성·관측 가능성 개선이다. 기존 durable receipt, Agent ownership, terminal operation ID, provider 승인 정책을 교체하지 않는다.

### 구체 변경

1. Tasks: 현재 `/home/jihoon/projects/devbox/packages/workspace-features/src/tasks/components/RuntimeRecovery.tsx`는 '처리/중단 요청 · targetId'만 표시한다. 이미 `RuntimeControlReceipt`에 있는 method/state/createdAt으로 '어떤 작업을 언제 요청했고 무엇을 먼저 확인할지'를 표시하고 해당 job/현재 실행 화면으로 이동시킨다. '현재 상태 확인 완료'가 프로세스 중단 버튼처럼 보이지 않게 한다. 성공한 상태 새로고침은 이전 읽기 실패 문구를 지운다. native 확인 전 local pending ID 삭제 금지.
2. Terminal: `/home/jihoon/projects/devbox/apps/devbox-workspace/src/Terminal.tsx`의 각 catch는 원인을 generic 문구로 합친다. 이미 투영된 WorkspaceOperationError의 허용된 메시지를 보여주고 '상태 새로고침'과 '동일 요청 다시 확인'을 분리한다. open/restore 성공 후 목록 refresh만 실패한 경우 open 실패로 표시하지 않는다. late 응답이 다른 context 화면을 바꾸지 않게 request generation을 고정한다. 기술적 operation ID/raw path/error 본문은 화면에 노출하지 않는다.
3. DevelopmentSessions: 이미 recovered background ownership/명시 cleanup UI가 있으므로 중복 복구 화면을 만들지 않는다. `/home/jihoon/projects/devbox/apps/devbox-workspace/src/DevelopmentSessions.tsx`의 시작 결과 불확실 상태에 '세션 상태 확인' 동작을 명시하고, 현재 session 상태를 읽는 것과 새 prepare/start를 구분한다. borrowed/created 자원 설명을 기존 native 정보로 표시한다.
4. Dependencies: `/home/jihoon/projects/devbox/packages/workspace-features/src/source/components/DependencyLensPanel.tsx`의 lock revision mismatch에 바로 '다시 분석'을 제공하고 새 inventory→전송 preview→승인의 기존 경계를 유지한다. provider 부분 실패/오래된 캐시는 현재 state label을 유지하면서 '전송 내용 다시 검토'로 연결한다. read-only inventory 기능을 패키지 설치/제거로 오인하지 않도록 설명을 명확히 한다. 자동 network retry/자동 설치/전체 패키지 매니저 실행을 추가하지 않는다.

수정 파일:

- `/home/jihoon/projects/devbox/packages/workspace-features/src/tasks/components/RuntimeRecovery.tsx`
- `/home/jihoon/projects/devbox/packages/workspace-features/src/tasks/App.tsx` (기존 job selection callback 전달)
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/Terminal.tsx`
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/DevelopmentSessions.tsx`
- `/home/jihoon/projects/devbox/packages/workspace-features/src/source/components/DependencyLensPanel.tsx`
- `/home/jihoon/projects/devbox/apps/devbox-workspace/src/Terminal.test.tsx`, `DevelopmentSessions.test.tsx`
- `/home/jihoon/projects/devbox/packages/workspace-features/src/tasks/runtimeControls.test.ts`, `source/components/DependencyLensPanel.test.tsx`

신규 테스트:

- `/home/jihoon/projects/devbox/packages/workspace-features/src/tasks/components/RuntimeRecovery.test.tsx`

기존 `/home/jihoon/projects/devbox/apps/devbox-workspace/README.md`에 실사용 수용 절차를 추가하고 실행 증거는 PR 본문/08 추적표에 기록한다. 별도 workthrough 문서 생성 없음. native runtime/terminal/dependency engine은 기본 수정 소유권에서 제외한다. 아래 실기 실패가 나오면 해당 경로의 최소 failing regression으로 먼저 확정한 뒤 범위를 재평가한다.

RED assertion 예시:

- interrupted stop_service receipt→'서비스 중지 요청' 및 대상 이동 표시; 단순 상태 확인 click은 runtime_control 호출 0회; review 실패면 local ID 유지, 성공 후에만 정리.
- RuntimeRecovery 목록 refresh 실패→다음 성공이면 낡은 alert 사라짐. pending 상태에는 확인 완료/신규 실행 유도 없음.
- terminal open IPC 성공→sessions 목록 실패: '열림 확인 후 목록 확인 실패' 상태, retry는 목록만 조회하여 open_terminal 호출 수 1 유지.
- terminal open 응답 유실→동일 요청 확인: 같은 operationId 2회, 새 terminal generation 1개. context A의 늦은 응답은 context B 상태에 반영되지 않음.
- DevelopmentSessions start 응답 유실→상태 확인: prepare/start 호출 수 증가 없이 session snapshot 조회. borrowed service cleanup은 stop 대상이 아님.
- Dependencies lock revision mismatch→재분석 click은 inventory만 호출, execute 0회. 새 transmission preview 명시 승인 후에만 execute 1회. provider partial failure를 '취약점 없음'으로 표시하지 않음.

```sh
python3 .github/scripts/verify-resources.py -- pnpm --filter @devbox/workspace-features exec vitest run src/tasks/components/RuntimeRecovery.test.tsx src/tasks/runtimeControls.test.ts src/source/components/DependencyLensPanel.test.tsx
python3 .github/scripts/verify-resources.py -- pnpm --filter devbox-workspace exec vitest run src/Terminal.test.tsx src/DevelopmentSessions.test.tsx
```

### 기존 미검증 경계를 실제 수용 시나리오로 바꾸기

공통 fixture는 이 작업에서 정의한 임시 repository, 공개 테스트 package명/lockfile, 테스트 전용 작업·터미널·run만 사용한다. 사용자 파일/secret/실제 서비스 사용 금지. Windows 11 테스트 PC 또는 독립 CI VM에서 제품 실행, WSL은 준비된 독립 VM에서 수행한다. Docker/방화벽/공유 network 조작이 필요한 검사는 hosted 격리 VM 전용이며 이 작업의 기본 수용에서는 제외한다.

| 시나리오 | 실행 방법 | 합격 기준 |
|---|---|---|
| R1 응답 유실 후 중복 방지 | test transport에서 native start 실행은 완료시키되 최초 reply만 유실; UI 재시도 및 app 재열기 | operation receipt/run 1개, 추가 process 없음, 불확실 상태가 성공으로 표시되지 않음 |
| R2 Agent 연결 단절 | 테스트 전용 Agent/VM에서 연결을 끊고 같은 operation 상태 확인 후 복원 | local engine fallback 0회, 연결 복원 후 기존 receipt로 표시, 자동 재실행 없음 |
| R3 created/borrowed 소유권 | 테스트 service S를 먼저 실행→session에서 S borrow+새 service T 생성→session cleanup | T만 종료, S는 유지; stopping 실패는 성공 표시 없이 재확인 가능 |
| R4 process tree 종료 | Windows job은 root가 자식 process를 생성; WSL fixture는 TERM을 지연하고 child 유지→명시 중단 | 해당 run descendants 모두 종료 증명 후 stopped, timeout이면 stopped 오표시 금지; unrelated sentinel process 유지 |
| R5 로그와 재접속 | run A 로그에 고유 marker→로그 열기→rotate/Agent 재접속→다른 run B 생성 | A/B 혼합 없음, stale descriptor는 재해석/명시 오류; 보호 경로 임의 열기 불가 |
| T1 터미널 창 lifecycle | 테스트 terminal 열기→보조창 X→main에서 창 표시→명시 터미널 종료 | X는 hide, 동일 session/window 재표시; 명시 종료만 shell 종료 |
| T2 터미널 restore | fixture layout을 저장하고 앱 종료/재실행→상태만 다시 연결 | 시작 명령 재실행 0회, 새 명령은 별도 명시 실행; restore generation 재사용으로 중복 shell 없음 |
| D1 offline inventory | pnpm/Cargo/public fixture lockfile을 Windows/WSL root 각각 분석, network provider는 선택하지 않음 | inventory 성공; network 전송 0회; lockfile 없음/형식 미지원 별도 표시 |
| D2 승인 사이 lock 변경 | inventory→preview 후 fixture lock 수정→승인 | stale preview 거부, 재분석·재승인 필요, 이전 revision 결과를 현재 결과처럼 표시하지 않음 |
| D3 provider 부분 실패 | hosted HTTP adapter fixture에서 OSV success/deps.dev timeout 또는 429; 필요한 호환성 변경이 있을 때만 별도 public provider smoke | 서비스별 failed/cached/fresh 구분, inventory 유지, 자동 전송 없음; 공개 provider smoke는 외부 전송 없는 기본 수용과 분리하며 이 계획의 필수 완료 조건으로 두지 않음 |
| I1 managed LSP 설치 경계 | reviewed catalog의 테스트용 manifest/archive로 install→cancel/retry→실행→uninstall; corrupt index fixture는 사용자 데이터와 분리 | partial/staging 잔재 정리, 정확한 version/entrypoint만 실행, 해당 managed dir만 제거, 실패 시 기존 index/설치본 보존 |

R1/R2/D3 실패 주입은 테스트 harness 또는 독립 VM에서 수행한다. 실제 사용자 Agent 중단, hosts/proxy/iptables 변경으로 모사하지 않는다. 현재 보유하지 않은 fixture/harness는 구현 단계의 테스트 범위이며 지금 실행했다고 기록하지 않는다. I1은 production 수정 없이 기존 managed installer 경로 수용을 보강하며, 새 패키지 관리자 UI를 만드는 범위가 아니다.

## 공통 실행·완료 게이트

각 작업에서는 원인에 직접 닿는 최소 RED→GREEN과 영향 회귀만 로컬에서 실행한다. 전체 영향 검증·Biome·Windows 수용은 모든 작업을 통합한 뒤 00 §4에 따라 모으며 작업별 전체 검사나 Actions를 실행하지 않는다. 최종 PR은 1개다. Windows cfg는 WSL PASS로 대체하지 않고 실제 Windows 결과를 기록한다. 독립 환경이 필요한 항목은 최종 후보 수용에 합치고 출시 전에 완료한다. 실행 증거는 같은 PR 본문과 추적표에 연결하며 닫힌 #580에는 메시지를 쓰지 않는다.

`pnpm verify:all` 중복 실행과 기존 서비스 조작은 하지 않는다. candidate와 공개 릴리스는 이 앱 작업에서 별도로 실행하지 않고 R16에서 한 번 수행한다. 설치 W0, shell grid, R02, R03의 핵심 수용이 끝나기 전에 Workspace 일반 기능이 준비됐다고 판정하지 않는다. Source crash-draft 복원, 모든 lockfile parser/모든 terminal escape 처리, 모든 scheduler 분기는 이 세 작업의 전수 보장 범위가 아니다.

## Windows UI 시나리오 등록

R02에서 `.github/scripts/windows-workspace-draft-ui.mjs`와 `.test.mjs`(WORK-01/02), R03에서 `windows-workspace-agent-registry-ui.mjs`와 `.test.mjs`(WORK-03), R14에서 `windows-workspace-runtime-recovery-ui.mjs`와 `.test.mjs`(RUNTIME-01/02, DEPS-01)를 생성한다. 절대 기준 디렉터리는 `/home/jihoon/projects/devbox/.github/scripts/`다. 각 모듈은 04의 `run(context): Promise<ScenarioResult[]>`와 동일한 context/export 계약을 구현하고 R00 matrix ownerWorkItem/module에 등록한다. R02의 native close·recovery, R03의 registry 변경, R14의 receipt/terminal/provider fixture는 해당 모듈이 임시 소유 영역에서 준비·관찰·정리한다. 제품 조작은 UiDriver로 하고 내부 invoke로 화면 단계를 건너뛰지 않는다.

R14의 managed LSP install/import/uninstall 시나리오는 같은 모듈의 추가 ID `LSP-01`로 등록한다. 설치 전 catalog/digest/선택한 fixture server artifact와 제거 후 owned directory만 사라졌다는 결과를 남긴다. 실제 LSP artifact가 없으면 NOT_RUN이고 필수 수용은 미완료다.
