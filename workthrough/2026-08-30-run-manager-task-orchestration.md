# Run Manager workspace task orchestration and Workbench task control

## Overview

Run Manager의 VS Code workspace task import(PR1)을 기반으로 dependency DAG 실행, shell task 별도
신뢰 승인, bounded problem matcher diagnostics, durable operation/receipt와 Workbench typed
task-control을 보강했다. Run Manager는 `0.5.0`, Workbench는 `0.3.0`으로 올리는 통합 변경이며,
기존 cron/service scheduler와 실행 adapter의 ownership 경계를 유지한다.

PR1의 기준 merge는 `#504`, commit
`dc763b7e98a4d970643201aa46d5c4541d6d054b`이다. 현재 문서는 그 위에 쌓인 PR2 작업의 구현
기록이다.

## Context

- PR1은 `.vscode/tasks.json`을 bounded/offline으로 읽어 process task를 disabled·untrusted
  draft로 저장했지만, `dependsOn`, shell task, problem matcher와 Workbench 실행 연계는 후속
  범위였다.
- 사용자가 task를 하나씩 실행할 때 dependency 순서·병렬성·실패 branch를 안전하게 추적하고,
  중지 시 다른 operation의 process를 건드리지 않도록 durable ownership이 필요했다.
- shell command는 process task와 다른 위험 경계를 가지므로 일반 source trust와 분리된 현재
  revision별 shell trust가 필요했다.
- matcher가 로그의 경로와 메시지를 그대로 IPC/handoff로 내보내지 않도록 bounded diagnostics와
  project-root containment 검증이 필요했다.
- Workbench는 Run Manager의 DB나 실행 process를 직접 소유하지 않는다. 따라서 opaque task
  snapshot → one-time `task-control/v1` handoff → Run Manager 확인 modal → redacted receipt의
  명시적 흐름을 사용한다.

## Changes Made

### 1. Durable workspace task DAG orchestration

- `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/core/workspace_orchestration.rs`
  - 같은 source의 exact label만 dependency로 허용한다.
  - root와 dependency closure를 계산하고 parallel/sequence layer를 deterministic하게 만든다.
  - 최대 128 task와 512 edge, cycle/missing/self-edge를 bounded하게 검증한다.
  - operation 및 child 상태(`queued`/`running`/`stopping`/terminal,
    `pending`/`launching`/`running`/terminal)를 정의한다.
- `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/workspace_orchestration.rs`
  - operation을 먼저 durable하게 만들고 normal scheduler run으로 child를 실행한다.
  - source 전체를 operation 시작 전에 검증하고, 각 child spawn 직전에 projection/revision을
    재검증한다.
  - fail-fast는 해당 operation의 exact child run만 중지하고 downstream을 skipped로 처리한다.
  - explicit stop은 active run id와 operation ownership을 재확인하며, 다른 operation·외부 PID를
    추측해서 종료하지 않는다.

### 2. Shell trust와 실행 진입점 보호

- `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/core/workspace_tasks.rs`
  와 `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/storage.rs`
  에 dependency/problem matcher projection, `shell_trusted` 상태와 schema v4 persistence를
  반영했다.
- `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/commands.rs`
  에 현재 revision에 대한 `execute-shell-tasks` 별도 확인, source 변경 시 trust/availability
  무효화, managed task의 direct run/enable guard를 유지했다.
- source가 관리하는 name/command/cwd/target은 일반 Job Editor에서 임의로 바꿀 수 없고,
  dependency task는 `run_workspace_task_operation` 경로를 사용한다.

### 3. Recovery, deletion guard와 durable receipt

- `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/storage.rs`
  에 `workspace_task_operations`, child run, `workspace_task_control_receipts` table/index와
  atomic state transition을 추가했다.
- `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/scheduler.rs`
  는 startup/tick마다 stale run을 먼저 복구한 뒤 interrupted operation을 reconcile한다.
  active child가 확인되지 않을 때만 pending을 skipped로 만들고 parent를 failed로 끝낸다.
  cleanup을 입증하지 못하면 operation은 `stopping`으로 남아 다음 tick에서 재시도한다.
- stop settle에는 30초 bounded timeout을 적용한다. launch reservation/child attach가 끝나기
  전에는 parent를 terminal로 앞당기지 않는다.
- `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/commands.rs`
  와 `/home/jihoon/projects/devbox/apps/run-manager/src/App.tsx`는
  active run 또는 active workspace operation이 있는 job의 삭제를 UI/native 양쪽에서 막는다.
- receipt가 accepted에서 rejected/started/stopped/failed로 한 번만 전이되며, receipt 전이 직후
  `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/integration.rs`
  가 `task-control-receipts` snapshot을 즉시 갱신한다. snapshot에는 operation id와 고정
  failure code만 남고 expected revision, command, path, environment 값은 노출하지 않는다.
- active operation의 root 또는 dependency job 삭제는 storage transaction에서 거부한다. 완료된
  operation의 member job 삭제는 operation·child·연결 receipt를 함께 정리해 부분 history나
  provenance 없는 receipt를 남기지 않는다.

### 4. Bounded problem matcher diagnostics

- `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/core/workspace_diagnostics.rs`
  는 명시적 matcher object만 사용해 terminal child의 stdout/stderr 보존 범위를 분석한다.
  stream당 4 MiB·50,000 line·500 diagnostic 상한과 `truncated` 표시를 적용한다.
- 진단 결과는 project-relative file, 1-based line/column, 정규화 severity, bounded message와
  stream만 가진다. file은 Code Pad로 열기 직전에 canonical regular file이고 project root
  내부인지 다시 확인한다.
- `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/commands.rs`
  의 diagnostics command는 실행 중인 child나 operation에 속하지 않은 run을 거부한다.
  로그 원문·matcher capture·absolute path는 handoff/snapshot에 저장하지 않는다.

### 5. Workbench one-time task control

- `/home/jihoon/projects/devbox/crates/applink/src/task_control.rs`
  에 `task-control/v1` 계약을 추가했다. payload는 schemaVersion, random requestId, opaque
  taskId, `start|stop`, expectedRevision만 허용한다.
- `/home/jihoon/projects/devbox/apps/workbench/src-tauri/src/commands/task_control.rs`
  는 Run Manager named snapshot을 strict하게 읽고, one-time handoff publish/launch 실패 시
  exact pending envelope을 정리한다. dispatch 직전에도 task/revision/trust/active 상태를 native
  snapshot과 다시 대조해 renderer 입력이 durable metadata로 바로 넘어가지 않게 한다. receipt는
  허용된 shape와 fixed error code만 전달한다.
- `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/task_control.rs`
  는 handoff claim/lease/renew/ack/restore를 수행하고, 현재 DB/source revision을 다시 검증한
  뒤 Run Manager 창에서 명시적 확인을 받는다. workspace job ID는 canonical UUID만 받아 임의
  correlator가 receipt snapshot에 저장되는 것을 차단한다. Start는 dependency closure operation을
  만들고, Stop은 해당 task가 root인 active owned operation만 중지한다.
- `/home/jihoon/projects/devbox/apps/workbench/src/components/WorkspaceTaskControlPanel.tsx`
  와 `/home/jihoon/projects/devbox/apps/workbench/src/lib/taskControl.ts`
  는 safe task snapshot만 표시하고, request/task/action correlator가 맞는 receipt만 polling해
  보여 준다. 전역 단일 요청 guard와 충돌 없는 ARIA status reference를 사용한다. 확인 modal은
  keyboard focus 복구와 bounded lease renewal을 사용한다.
- `/home/jihoon/projects/devbox/apps/run-manager/src/App.tsx`는
  Run Manager 확인 modal에서 label/kind/action/revision만 표시하고, ESC/거절을 rejected
  receipt로 처리한다. 오래된/mismatched receipt는 렌더링하지 않는다.

### 6. Integration, versions와 documentation

- `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/integration.rs`
  는 `workspace-tasks`와 `task-control-receipts` named view를 발행한다. task view에는 trust,
  availability, dependency 존재 여부와 `operationActive`만 제공하고 실행 원문은 제외한다.
  operation 생성·executor 종료·명시적 stop 직후 `workspace-tasks` view를 다시 발행하므로
  Workbench의 수동 새로고침은 주기 발행을 기다리지 않는다.
- Run Manager UI는 같은 timestamp의 history가 있어도 active operation을 우선하며, native DB
  polling이 정상인 장기 task는 임의의 10분 실행 시간 제한으로 추적을 중단하지 않는다.
- `/home/jihoon/projects/devbox/apps/catalog.json`과
  `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/applink.rs`
  에 Run Manager/Workbench capability와 routing을 연결했다.
- Run Manager 0.5.0:
  `/home/jihoon/projects/devbox/apps/run-manager/package.json`,
  `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/Cargo.toml`,
  `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/tauri.conf.json`.
- Workbench 0.3.0:
  `/home/jihoon/projects/devbox/apps/workbench/package.json`,
  `/home/jihoon/projects/devbox/apps/workbench/src-tauri/Cargo.toml`,
  `/home/jihoon/projects/devbox/apps/workbench/src-tauri/tauri.conf.json`.
- 관련 README/spec와 이 기록은 다음 절대 경로에 있다:
  `/home/jihoon/projects/devbox/apps/run-manager/README.md`,
  `/home/jihoon/projects/devbox/apps/workbench/README.md`,
  `/home/jihoon/projects/devbox/docs/superpowers/specs/2026-08-12-run-manager-design.md`,
  `/home/jihoon/projects/devbox/docs/superpowers/specs/2026-08-17-app-interop-design.md`,
  `/home/jihoon/projects/devbox/docs/superpowers/specs/2026-08-30-run-manager-workspace-task.md`.

### 7. 전체 검증 중 발견한 기존 테스트 안정화

- 전체 Rust workspace를 병렬 검증하는 동안 Repo Manager Dependency Lens의 stale lockfile
  fixture가 파일시스템 mtime 해상도에 따라 간헐적으로 실패하는 것을 재현했다.
- `/home/jihoon/projects/devbox/apps/repo-manager/src-tauri/src/core/dependency_lens.rs`의 해당
  테스트가 `sleep(5ms)`에 의존하지 않고 명시적으로 구분된 mtime을 사용하도록 바꿨다.
  런타임 분석 로직이나 제품 결과에는 변화가 없으며, Linux/Windows CI에서 동일한 stale
  조건을 결정적으로 검증한다.

### Complete PR2 file inventory

The integrated change also touches the following support, UI, test, and generated dependency files:

- `/home/jihoon/projects/devbox/.github/scripts/windows-packaged-smoke-config.json`
- `/home/jihoon/projects/devbox/Cargo.lock`
- `/home/jihoon/projects/devbox/THIRD_PARTY_NOTICES.md`
- `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/core/mod.rs`
- `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/applink.rs`
- `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/lib.rs`
- `/home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/scheduler.rs`
- `/home/jihoon/projects/devbox/apps/run-manager/src/App.css`
- `/home/jihoon/projects/devbox/apps/run-manager/src/App.test.tsx`
- `/home/jihoon/projects/devbox/apps/run-manager/src/api.ts`
- `/home/jihoon/projects/devbox/apps/run-manager/src/types.ts`
- `/home/jihoon/projects/devbox/apps/run-manager/src/components/ImportDialog.tsx`
- `/home/jihoon/projects/devbox/apps/run-manager/src/components/ImportDialog.test.tsx`
- `/home/jihoon/projects/devbox/apps/run-manager/src/components/JobEditor.tsx`
- `/home/jihoon/projects/devbox/apps/run-manager/src/components/JobEditor.test.tsx`
- `/home/jihoon/projects/devbox/apps/workbench/src-tauri/src/commands/mod.rs`
- `/home/jihoon/projects/devbox/apps/workbench/src-tauri/src/lib.rs`
- `/home/jihoon/projects/devbox/apps/workbench/src/App.tsx`
- `/home/jihoon/projects/devbox/apps/workbench/src/App.css`
- `/home/jihoon/projects/devbox/apps/workbench/src/App.test.tsx`
- `/home/jihoon/projects/devbox/apps/workbench/src/App.applink.test.tsx`
- `/home/jihoon/projects/devbox/apps/workbench/src/api.ts`
- `/home/jihoon/projects/devbox/apps/workbench/src/components/WorkspaceTaskControlPanel.tsx`
- `/home/jihoon/projects/devbox/apps/workbench/src/components/WorkspaceTaskControlPanel.test.tsx`
- `/home/jihoon/projects/devbox/apps/workbench/src/lib/taskControl.ts`
- `/home/jihoon/projects/devbox/crates/applink/src/lib.rs`
- `/home/jihoon/projects/devbox/crates/applink/src/task_control.rs`
- `/home/jihoon/projects/devbox/apps/repo-manager/src-tauri/src/core/dependency_lens.rs`

## Code Examples

### Exact task-control payload

```json
{
  "schemaVersion": 1,
  "requestId": "<32 lower-hex characters>",
  "taskId": "<opaque task id>",
  "action": "start",
  "expectedRevision": "<64 lower-hex characters>"
}
```

The payload intentionally has no command, cwd, argv, environment, path, or PID.

### Direct run guard

```rust
// /home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/commands.rs
if revalidate_workspace_task_action(&id, database.inner().as_ref())?.is_some() {
    return Err("workspace-task-orchestration-required".to_owned());
}
```

Managed workspace tasks cannot bypass the orchestration boundary through the ordinary single-run
command.

### Exact owned stop

```rust
// /home/jihoon/projects/devbox/apps/run-manager/src-tauri/src/scheduler.rs
pub async fn stop_exact_active_at(
    &self,
    job_id: &str,
    expected_run_id: &str,
    now: i64,
) -> Result<Option<Run>, SchedulerError> {
    self.stop_active_matching_at(job_id, Some(expected_run_id), now)
        .await
}
```

The comparison runs under the same per-job lifecycle mutex as start/stop. The stop path therefore
acts only on the run id recorded by that operation and cannot terminate a replacement process.

### Workbench confirmation handoff

```rust
// /home/jihoon/projects/devbox/apps/workbench/src-tauri/src/commands/task_control.rs
let request = TaskControlRequest {
    schema_version: TASK_CONTROL_SCHEMA_VERSION,
    request_id: uuid::Uuid::new_v4().simple().to_string(),
    task_id,
    action,
    expected_revision,
};
```

Run Manager receives this one-time envelope, shows its own confirmation UI, then writes a redacted
receipt before Workbench records the result.

## Verification Results

The final PR2 source passed the following local gates:

```text
cargo test --workspace -j2
  passed across the complete Rust workspace

Repo Manager stale-lockfile regression fixture
  passed 10 consecutive deterministic repetitions after removing the mtime sleep race

cargo test -p run-manager --lib -j2
  run-manager: 262 passed, 1 ignored (Windows/WSL interoperability)

selected backend package totals from the workspace run
  applink: 75 passed
  workbench: 125 passed

cargo check --workspace -j2
  passed

cargo clippy --workspace --all-targets -j2 -- -D warnings
  passed

cargo deny --locked check
  passed (configured duplicate/yanked warnings only; advisories/bans/licenses/sources all OK)

Run Manager frontend
  7 files, 53 tests passed
  production build passed

Workbench frontend
  8 files, 84 tests passed
  production build passed

pnpm build && pnpm test
  passed across all 19 participating frontend workspace packages

pnpm audit --audit-level moderate
  no known vulnerabilities

dependency/notices/build-manifest/release-input/catalog/Windows smoke contract scripts
  passed

cargo fmt --all -- --check
  passed

git diff --check
  passed
```

Windows packaged/manual acceptance has not been performed yet. In WSL, one Windows-only test is
intentionally `ignored`; that is not evidence of Windows acceptance. The final report must keep
the Windows results separate from the WSL source/build results and update the counts after the
latest agent changes.

## Next Steps / known limitations

- All six GitHub Actions CI jobs remain required before merge; their PR evidence is recorded in
  GitHub rather than presented as local Windows acceptance.
- Perform Windows acceptance for local-drive and `\\wsl$`/`\\wsl.localhost` roots, process/shell
  task execution, dependency parallel/sequence/fail-fast/stop, source revision changes, diagnostics
  to Code Pad, Run Manager restart recovery, and Workbench one-time confirmation/receipt flow.
  Record the physical acceptance separately in #176/#492.
- The Windows-only ignored test remains pending until the Windows environment can exercise the
  adapter. No Windows packaged release evidence exists yet.
- VS Code extension tasks, dynamic variables, background/runOptions semantics, remote hosts and
  generic cron dependency workflows remain outside this implementation. Unsupported forms stay
  blocked instead of being interpreted through a shell fallback.
- Recovery deliberately leaves an operation in `stopping` when exact child cleanup cannot be
  proven; the scheduler retries on a later tick. Diagnostics remain bounded and only inspect the
  currently retained app-owned logs.
