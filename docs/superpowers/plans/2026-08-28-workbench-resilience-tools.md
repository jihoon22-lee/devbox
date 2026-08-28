# Workbench resilience/inspection tools — implementation plan (#359 + #360 + #361)

> Issues: #359, #360, #361 · branch: `feat/workbench/resilience-tools`
> Worktree: `/mnt/e/projects/devbox-worktrees/workbench-resilience-tools`
> Base: latest `origin/main` at preparation time (`2b53744`)

## Goal

Workbench의 P3-14 보강 세 가지를 하나의 검토 가능한 사용자 흐름 후보로
구현한다.

```text
프로필 템플릿 선택/새 프로젝트 wizard
  → profile 저장
  → dependency health read-only inspection
  → Start Workspace 결과 확인
  → 실패한 bounded step부터 retry
```

## Grouping decision

세 이슈는 모두 같은 Workbench 경계(프로필 선택, dependency 관찰, Workspace
transition, 실행 소유권)를 순서대로 통과하므로 한 PR 후보로 묶는 것이 타당하다.
다만 한 계약으로 합치지 않고 acceptance와 rollback 경계를 분리한다.

| Issue | 사용자 acceptance | 이 후보에서 유지하는 경계 | 독립 rollback boundary |
|---|---|---|---|
| #359 | template CRUD와 템플릿 기반 새 프로젝트 wizard | project-independent defaults만 복사하고 environment/secret은 복사하지 않음 | template 파일 CAS/atomic write 실패 또는 profile validation 실패 시 기존 template/profile 파일과 프로젝트 파일을 그대로 유지 |
| #360 | app/distro/path/port/service dependency health | 기존 bounded preflight DTO와 provenance를 read-only inspection에서 재사용; 자동 설치·시작·복구 없음 | health는 자원 mutation/cleanup을 하지 않음. stale 응답은 renderer sequence로 버리고 기존 화면을 유지 |
| #361 | 실패한 단계부터 idempotent retry | known step suffix만 실행하고 성공 step·external resource·Workbench-owned process를 재시작하지 않음 | 기존 run은 commit 전 authoritative 상태로 유지. 이번 retry가 새로 만든 PID만 guard로 rollback; 일반 launch failure는 partial run으로 남김 |

한 이슈의 실패는 다른 이슈의 저장소나 외부 앱 DB를 되돌리지 않는다. #359는
템플릿/profile 데이터 경계, #360은 관찰 전용 경계, #361은 run/PID 소유권 경계를
각각 소유한다.

## Acceptance #359 — profile template CRUD + wizard

### Contract

- `%LOCALAPPDATA%\com.devbox.workbench\profile-templates.json`에 별도 bounded
  `ProfileTemplateStore`를 둔다. version 1, 최대 128개, 직렬화 파일 1 MiB,
  strict unknown-field rejection을 적용한다.
- 템플릿에는 ID/name, 선택적 Windows/WSL/Git 기본 경로, expected ports와
  Run Manager service IDs만 둔다. `environment`, raw value, ciphertext, secret
  reference는 타입과 저장소 모두에 존재하지 않는다.
- wizard는 기존 입력값을 우선하고 빈 field에만 템플릿 기본값을 채운다. 새 profile
  ID는 backend가 생성하며, template 적용 후에도 profile 전체를 다시 validate한다.
- profile template CRUD는 Workbench profile store lock과 CAS/atomic write를
  공유하지만 두 JSON 파일을 한 트랜잭션으로 묶지 않는다. profile 생성 실패 시
  template 파일은 바뀌지 않는다.

### Fixtures

- Rust core: round-trip, unknown field, duplicate ID/name, list/file bounds,
  unsafe path, apply-only-empty-fields, environment drop.
- Rust command: missing file → empty store, regular-file round-trip, symlink/link
  rejection, malformed/credential-looking bytes with fixed error.
- React: template draft round-trip, path-less template, invalid port/service input,
  wizard selection and create payload with `environment: null`, manager update.

### Rollback boundary

템플릿 load/validate/CAS/atomic write 오류는 원본 bytes를 유지한다. wizard 취소나
검증 실패는 profile store, project file, child process를 건드리지 않는다. template
삭제는 기존 concrete profile을 수정하지 않는다.

## Acceptance #360 — dependency health

### Contract

- `dependency_health`와 `workspace_preflight`는 `health_operation` single-flight
  lane을 공유해 Start Workspace/project health와 native probe가 겹치지 않게 하며,
  각 요청을 bounded budget으로 취소·대기시킨다. 두 명령은 동일한 bounded probes와
  `WorkspacePreflight`/`ResourceProvenance` DTO를 반환한다.
- 새 read-only 요청은 같은 operation family의 이전 health/preflight 작업과 pending
  ticket만 supersede할 수 있다. 서로 다른 health surface는 같은 lane에서 순차 실행하며,
  보호된 `workspace-start` transition의 token은 취소하지 않는다. Start가
  lane을 소유한 동안 read-only 요청은 bounded budget 안에서 대기하거나 fixed error로
  종료한다.
- required app capability, WSL distro 존재/running, Windows·WSL path,
  expected TCP ports, Run Manager service snapshot을 pass/warning/failure/
  unavailable로 구분한다.
- probe는 fixed argv, null stdin, discarded stderr, bounded output/timeout을
  사용하고 stopped distro를 inspection 때문에 시작하지 않는다.
- UI는 선택 profile에 대해 자동 refresh와 명시적 새로고침을 제공하되, 결과에
  executable path, PID, stderr, raw service metadata 또는 credential을 표시하지
  않는다. template modal은 Escape, Tab 순환, 초기/복귀 focus와 busy 상태를
  보장하고, template 목록 요청은 generation guard로 닫힌 dialog나 새 요청을
  덮어쓰지 않는다.

### Fixtures

- Pure preflight fixture를 재사용해 installed/missing app, distro/path missing or
  unavailable, free/existing/conflicting port, missing/partial/corrupt service
  snapshot을 각각 확인한다.
- React fixture는 pass/warning/failure 상태와 resource labels, independent
  refresh, profile navigation 중 stale 결과 무시를 확인한다.

### Rollback boundary

health 명령에는 rollback이 없다. read-only probe 중 오류는 fixed unavailable
결과 또는 fixed error로 끝나며 WSL/service/port/profile을 변경하거나 종료하지
않는다. renderer stale 결과는 현재 선택을 덮어쓰지 않는다.

## Acceptance #361 — idempotent failed-step retry

### Contract

- `WorkspaceRun`은 `retryCount`, `canRetry`, `failedStep`을 추가하고 기존
  bounded `steps`와 stable resource provenance를 유지한다. ownership restore
  DTO에는 PID가 계속 노출되지 않는다.
- retry planner가 허용하는 순서는 `wait-port` → `open-wsl-desktop` →
  `open-code-pad`뿐이다. 첫 failed known step에서 시작하며, 성공한 step과
  `Existing`/`WorkbenchStarted` process provenance를 skip한다. unknown failed
  step/no failed step은 fail-closed한다.
- retry 시작 전 profile을 다시 로드하고 preflight를 재실행한다. profile 변경,
  cancellation/timeout, environment/provider 오류, publish 전 오류는 새 PID만
  `StartedPidGuard`로 정리하고 기존 run을 보존한다.
- child launch 자체의 fixed failure는 고정된 failed step으로 기록한다. 서비스
  자동 시작, 기존 external process 강제 종료, 전체 Workspace 재시작과 destructive
  auto-repair는 포함하지 않는다.

### Fixtures

- Pure retry fixture: failure-step resume, successful/Workbench-owned process skip,
  external existing process is not restarted, no-failure and unknown-step rejection.
- Rust workspace fixture: failed-step metadata, provenance merge/deduplication,
  run ownership/stop identity checks.
- React fixture: failed Code Pad retry, existing WSL provenance preserved, retry
  result/remaining failure rendering and fixed error path.

### Rollback boundary

retry의 기존 `WorkspaceRun`은 마지막 commit 전까지 authoritative하다. retry 중 새로
생긴 PID는 별도 guard에만 들어가며 any stale profile/budget/operation failure에서
그 PID만 terminate한다. 성공한 child는 즉시 종료하지 않고 partial run으로 게시해
사용자가 `Stop What I Started`로 Workbench-owned PID만 정리한다.

Stop What I Started는 OS의 process-tree 종료 결과를 확인한다. 종료가 거부되거나
일시적으로 실패한 PID가 있으면 실행 기록을 삭제하지 않고 남은 ownership만 보존해
재시도할 수 있게 한다. taskkill/kill의 원문 출력은 UI/오류로 전달하지 않는다.

## Implementation map

### Rust

- `apps/workbench/src-tauri/src/core/templates.rs` — bounded template DTO/store/
  apply contract and pure fixtures.
- `apps/workbench/src-tauri/src/commands/templates.rs` — template file CRUD and
  backend wizard/profile creation boundary.
- `apps/workbench/src-tauri/src/core/retry.rs` — deterministic retry planner and
  fixtures.
- `apps/workbench/src-tauri/src/core/preflight.rs` and
  `commands/preflight.rs` — health type alias, read-only command, and shared
  `health_operation` single-flight boundary.
- `apps/workbench/src-tauri/src/commands/workspace.rs` — step/provenance metadata,
  child launch guard, retry command and stop/start transition gate.
- `apps/workbench/src-tauri/src/{core,commands}/mod.rs` and `lib.rs` — module and IPC
  registration.

### Frontend

- `apps/workbench/src/api.ts` — template, dependency health and retry IPC contracts.
- `apps/workbench/src/lib/profileTemplateEditor.ts` plus test — wizard/template
  draft validation and no-environment mapping.
- `apps/workbench/src/App.tsx`, `App.css` — wizard, manager, health panel and retry
  actions with busy/request guards, modal focus containment/restore, and stale
  template request isolation.
- `apps/workbench/src/App.test.tsx`, `App.applink.test.tsx` — UI fixtures and mocks.

### Documentation

- `apps/workbench/README.md` — P3-14 candidate user flow, data and scope boundary.
- `docs/roadmap.md` — grouped preparation status and separate acceptance/rollback.
- `workthrough/2026-08-28-workbench-resilience-tools.md` — implementation record.

## Verification plan and preparation status

The original dirty-candidate phase intentionally deferred resource-heavy commands while
other grouped work was running. The remediation pass then used a single Rust worker and
the native target directory, and completed the following checks on the latest changes:

```text
cargo fmt --all -- --check                         PASS
git diff --check                                   PASS
cargo check -p workbench -p launch -j1             PASS
cargo test -p workbench -p launch -j1              PASS
  workbench: 113 passed; launch: 25 passed
cargo clippy -p workbench -p launch --all-targets -j1 -- -D warnings
                                                     PASS
pnpm --dir apps/workbench exec tsc --noEmit        PASS
```

The focused Rust fixtures include template bounds/CAS/path safety, preflight probe
outcomes, cancellation-before-spawn, retry planning, and Linux process-creation
identity/failed-cleanup outcomes. The frontend fixtures include template focus and stale
request handling, health refresh, retry rendering, preflight cancellation from its
loading state, and keeping a run visible when Stop retains failed ownership.

The launch boundary now clears inherited host environment variables before adding the
small platform runtime allowlist and the validated project overlay. This keeps unrelated
host secrets and shell hooks out of Workbench-launched apps while preserving the normal
no-overlay launch API. The launch crate has a focused allowlist fixture.

`pnpm --dir apps/workbench build` passed independently. The latest full Vitest attempt
was deliberately limited to our own process because the `/mnt/e` 9p mount became
I/O-bound under concurrent workspace work. The run was stopped safely after prolonged
no-progress; the earlier baseline run had 6 files/69 tests passing before the newest
cancellation/Stop fixtures. Parent must rerun the latest `pnpm --dir apps/workbench test`
when the host has headroom, then run the full Rust/frontend gates, CI, and Windows
packaged acceptance.

A source-level Windows GNU check reached the Tauri build script but could not complete on
this WSL host because `x86_64-w64-mingw32-windres` is not installed. This is an environment
toolchain gap, not a Rust diagnostic; the Windows packaged gate remains required.

No commit, push, PR, rebase, merge, worktree deletion, or branch deletion is part of
this candidate.

## Remediation details and risks for parent review

- Preflight and dependency health now use exact NUL-delimited request keys, a shared
  health single-flight lane, an operation token/budget, bounded child stdout and fixed
  argv/error output, and a detached worker that always joins its port worker before
  releasing the lane. Read-only cancellation supersedes only older work in the same
  operation family and its pending tickets; independent health surfaces share the lane
  without cancelling one another, and it preserves an active `workspace-start` mutation. The renderer
  shows a cancellable loading status and does not let stale or unmounted requests
  overwrite the selected profile.
- Workbench-owned process records capture a creation identity (Windows process creation
  time or Unix `/proc` start ticks), so PID reuse is treated as mismatch rather than a
  valid cleanup target. Workbench launches use a private Unix process group; when the
  root exits first, Stop still performs bounded TERM/KILL escalation against that group
  after the recorded identity check. Windows Stop uses `taskkill /PID /T /F` with bounded
  waiting; failed or unavailable termination is retained in ownership and the UI re-reads
  the authoritative run before clearing it. Short-lived native probes use a Unix process
  group or Windows kill-on-close Job Object, including the assignment-failure fallback,
  so timeout/cancel/drop cannot silently degrade to root-only cleanup.
- The Workbench environment launch boundary clears inherited host variables and restores
  only the platform runtime allowlist before applying the validated project overlay. This
  closes the documented host-secret inheritance gap without changing the ordinary launch
  path used by apps that do not request a project environment.
- Runtime app-capability catalog reads are bounded to 1 MiB before parsing, preventing a
  corrupted/oversized catalog from becoming an unbounded health probe.
- Native Windows packaged checks must exercise app capability discovery, stopped distro
  behavior, junction/reparse handling, port races, cancellation/timeout, and
  child-process PID rollback/reuse.
- Retry launch calls are intentionally narrow and do not auto-start Run Manager
  services; product review must confirm that partial-run UX is sufficient.
- The template file is independently atomic from the profile file. A crash between
  the two writes can leave a newly created profile and an unchanged template, which
  is safe but should remain visible in the PR rationale.
- `WorkspaceRun` persisted/IPC compatibility and TypeScript command payloads passed the
  focused Rust check/clippy and TypeScript compile. The latest frontend build passed;
  the full Vitest run and full workspace/Windows review remain parent gates.
