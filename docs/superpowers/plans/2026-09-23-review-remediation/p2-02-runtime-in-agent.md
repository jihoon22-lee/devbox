# P2-02 런타임 작업·서비스·스케줄을 agent로 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4와 ADR 0015를 읽는다.

**Goal:** Workspace의 런타임(작업 실행·서비스 감시·예약 실행·프로세스 관찰·로그 읽기)을 `devbox-agent`로 옮겨, Workspace 창을 닫거나 Workspace가 비정상 종료돼도 작업·서비스·예약이 계속 돌게 한다(D24, 리뷰 §8 C안).

**Architecture:**
- 런타임 engine(`runtime_engine`, `ports_engine`, `logs_engine`)은 이미 `AppHandle` + 데이터 폴더 + task source provider로 초기화된다. agent도 Tauri 앱이므로 **같은 초기화 함수를 agent에서 부른다**. 데이터는 지금처럼 Workspace 데이터 폴더(`com.devbox.v08.workspace.i<접미사>`)의 component 폴더에 둔다(ADR 0007; 데이터 이전 없음). 한 번에 한 프로세스만 런타임을 연다: agent를 쓸 수 있으면 Workspace는 런타임을 초기화하지 않는다.
- task source provider(프로젝트·worktree·신뢰된 정의를 읽어 실행 계획을 만드는 부분)는 Workspace 앱 안의 `Host`·`ProjectOwner`·저장소·Git 신뢰 코드에 묶여 있다. 이 읽기 계층을 `crates/workspace-core`로 옮겨 Workspace와 agent가 함께 쓴다(registry 쓰기는 계속 Workspace만 한다. 파일은 원자적 쓰기라 agent는 매번 새로 읽는다).
- Workspace host의 `runtime`·`processes`·`process_actions`·`logs` command(P1-14)는 입장 검사 뒤 agent가 있으면 같은 `ComponentRequest`를 agent로 전달하고, 없으면(portable) 지금처럼 자체 처리한다. 응답 모양은 같다.
- agent 쪽 라우팅: `workspace.runtime`·`workspace.processes`·`workspace.process-actions`·`workspace.logs`. agent는 연결한 peer가 Workspace일 때만 이 component를 받는다.

**Tech Stack:** Rust(Tauri v2), `agent-client`·`agent-protocol`, TypeScript(표시만)

**Spec:** ADR 0015 · `00-roadmap.md` D24

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 한 설치에서 런타임 소유자는 하나: agent가 `workspace.runtime`을 소유하는 동안 Workspace가 로컬 런타임을 초기화하면 안 된다(Task 3의 소유 판정).
- 작업 프로세스 트리 소유(Job Object, `runtime_engine/src/platform/windows.rs`)는 agent 프로세스로 옮겨진다. Workspace를 닫아도 작업이 끝나지 않는다. agent 종료(트레이 "종료", 업데이트 전 `Shutdown`)는 기존 런타임 종료 절차로 작업을 정리한다.
- 비밀 환경 변수(DPAPI CurrentUser)는 같은 사용자 프로세스인 agent가 그대로 푼다(P1-09 봉인기).
- 업데이트·데이터 복원은 agent가 종료된 뒤에만 진행된다(P2-01 업데이트 절차).

## Review Focus

1. 작업 실행 중 Workspace 창을 닫았다 다시 열기 → 작업이 계속 돌고, 다시 연 Workspace에 실행 상태·로그가 보인다. (Task 4, 사용자 실기)
2. Workspace가 꺼져 있을 때 예약 시각 도래 → agent가 예약 작업을 실행하고 기록을 남긴다. (Task 4)
3. Workspace에서 프로젝트 정의 신뢰를 철회 → 그 뒤 agent의 예약 실행이 "신뢰 필요"로 멈춘다(agent가 신뢰 정보를 매번 새로 읽음). (Task 1·4 테스트)
4. agent가 없는 portable 빌드 → 기존처럼 Workspace 안에서 런타임이 돈다. (Task 3)
5. agent가 작업 중에 죽음 → Job Object 규칙으로 작업 프로세스도 끝나고, 다시 시작한 agent가 실행 기록을 "중단됨"으로 정리한다(기존 복구 규칙). (Task 4)

## 2026-09-27 승인된 보완 — Sessions 소유권과 복구

사용자는 ledger #580의 [보완 요청](https://github.com/jihoon22-lee/devbox/issues/580#issuecomment-5850796958)에 대해 “네 계속해주세요”로 권장안을 승인했다. 아래 내용은 Task 2–4의 engine 전달·수명 범위를 보완한다. engine dispatch를 그대로 전달하라는 기존 문구는 host 조합·lease·복구 경계를 우회한다는 뜻이 아니다. 완료 상태는 ledger/PR에만 기록한다.

### 실제 native 소비자와 적용 경계

현재 계획 Task 2·3은 네 component의 engine dispatch를 agent로 옮기는 것으로 런타임 이전을 설명한다. 실제 구현에는 추가 native 소비자가 있다.

- `development_host.rs`는 `PreparedJob`, `RuntimeLease`, `RuntimeStartWitness`로 실행 준비·기동·서비스 빌림·소유 resource 정리를 수행한다. `RuntimeLease`는 AppHandle과 SchedulerCoordinator를 직접 보유한다.
- `RuntimeStartWitness`는 프로세스를 만든 뒤 응답이나 기록에 실패해도 생성자 lease를 유지한다. 단순 JSON 요청/응답으로 바꾸면 이 복구 경계가 사라진다.
- Workspace 종료는 Sessions의 `request_shutdown`/`shutdown`을 호출해 런타임 lease도 정리한다. UI 닫기와 사용자 명시적 Session 종료를 나눠야 백그라운드 실행을 보장할 수 있다.
- 포트 소유자 판정, 진단/로그 탐색, Problems·Sessions preflight도 같은 native 런타임 API를 사용한다. 기존 UI·선택 receipt를 유지한 owner 조회 경로가 필요하다.

### 확정 범위

B9와 P2-02 안에서 Task 2·3·4를 보완한다. 새 외부 의존성·버전/CHANGELOG 변경·새 제품은 추가하지 않는다. agent 프로토콜의 기존 Call/Reply를 사용한다.

#### Task 2 추가: agent가 실행 준비와 실행 lease를 보유

1. agent에 한정된 typed 내부 RPC를 추가한다: prepare, start/acquire, status/ready, cancel, stop/release, operation receipt 조회. renderer component의 허용 메서드를 넓히지 않는다.
2. agent가 실제 PreparedJob·RuntimeLease·RuntimeStartWitness를 소유한다. Workspace에는 검증된 요약·불투명 참조만 반환한다. 비밀 환경·실행기 내부 경로를 투영하지 않는다.
3. 참조는 동일 설치/generation·제품·durable development session/operation·project context에 묶고 기존 개수·기한 제한을 유지한다. 새 UI handshake로 재연결할 때는 Workspace의 저장된 Session 기록과 현재 registry를 다시 검증한다.
4. start 전에 operation별 witness를 등록한다. 응답 유실은 같은 mutation을 재전송하지 않고 receipt 조회로 판정한다. cancel도 실제 시작/정리 종료까지 witness와 worker permit을 보유한다.
5. 포트 소유자·런타임 로그·진단 조회는 agent가 수행하며, UI 탐색과 선택/전달 receipt는 Workspace에 남긴다. 파일/터미널/LSP/Git 소유권은 이전하지 않는다.

#### Task 3 추가: Workspace native 소비자를 owner adapter로 연결

1. 기존 네 command뿐 아니라 development sessions, Problems, preflight, 포트/로그/진단 경로를 빠짐없이 owner adapter에 연결한다.
2. installed에서는 연결 실패 시 오류로 끝내고 local runtime DB를 열지 않는다. portable만 기존 local 구현을 사용한다.
3. UI 닫기는 agent의 완료된 실행을 유지한다. 사용자가 Session을 명시적으로 멈추면 기존 생성/빌림 구분에 맞춰 정확한 리소스만 정리한다. PTY/LSP 등 UI 소유 리소스는 기존 종료 절차를 따른다.
4. UI 재개 시 저장된 Session 기록과 agent receipt를 대조한다. agent가 재시작해 lease가 사라졌으면 기존 런타임 중단 기록을 근거로 복구 상태를 표시하고 임의로 작업을 재실행/종료하지 않는다.

#### Task 4 추가: 회귀 및 hosted acceptance

- 실제 시작 뒤 응답/기록 유실, 중복 operation, 시작 중 cancel, 오래된/다른 설치 참조, 빌린 서비스의 잘못된 종료를 테스트한다.
- 작업/서비스를 띄우고 Workspace를 닫았다 다시 열어 실행·로그·Sessions/Problems 연계가 이어짐을 확인한다.
- 명시적 Session 종료와 Workspace UI 종료를 구별하고, terminal/LSP는 UI 종료 시 정리되는지 확인한다.
- agent 강제 종료 뒤 기존 중단 복구, 정의 신뢰 철회 뒤 예약 실행 차단, portable local 동작을 확인한다.
- 업데이트/복원은 모든 engine·witness·writer가 정리된 뒤에만 기존 exclusive gate로 진행한다.


## Branch · PR

- 묶음: **B9** — 브랜치 `feat/suite/devbox-agent`, PR 제목 `feat(suite): run runtime, webhooks and collectors in devbox-agent`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(suite): run tasks, services and schedules in devbox-agent`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: `crates/workspace-core`

**Files:** Create `crates/workspace-core/{Cargo.toml,src/lib.rs}`; Move from `apps/devbox-workspace/src-tauri/src/`: `host.rs`, `project_owner.rs`, `core/{stores,registry,templates,profiles,editor_sessions}.rs`(P1-04 이후 이름), `platform/{git_trust,task_sources,storage_paths}.rs`와 이들이 컴파일에 필요로 하는 도우미

**Interfaces (Produces):** `workspace_core::{Host, ProjectOwner, task_sources::Sources, storage_paths::ProtectedStorage}`(이동만, 동작 불변); `Host::open_read_only(root: &Path) -> Result<Host>`(registry 쓰기 메서드를 부르면 `registry_read_only` 오류)

- [ ] **Step 1: 이동** — 위 파일을 `git mv`로 옮기고 `crate::` 경로를 `workspace_core::`로 바꾼다. 옮긴 모듈이 Workspace 앱의 다른 모듈(예: Tauri `WebviewWindow`를 쓰는 곳)에 의존하면, 그 의존은 trait이나 인자로 바꿔 core가 Tauri 창 타입에 의존하지 않게 한다(core는 `tauri`의 `AppHandle`까지만 허용). 옮긴 파일의 테스트도 함께 옮긴다.
- [ ] **Step 2: 읽기 전용 열기 테스트** — `crates/workspace-core/src/host.rs` 테스트 모듈

```rust
    #[test]
    fn a_read_only_host_resolves_projects_but_refuses_registry_writes() {
        let dir = tempfile::tempdir().unwrap();
        let writer = Host::open(dir.path()).unwrap();
        writer.start_empty().unwrap();
        let reader = Host::open_read_only(dir.path()).unwrap();
        assert!(reader.projects().unwrap().snapshot().is_ok());
        assert_eq!(reader.start_empty().unwrap_err(), "registry_read_only");
    }
```

- [ ] **Step 3: 구현** — `Host`에 `read_only: bool`을 두고, registry·저장소를 바꾸는 공개 메서드 앞에서 `if self.read_only { return Err("registry_read_only") }`. `ProjectOwner`도 같은 플래그를 받는다.
- [ ] **Step 4: 확인·커밋** — Run: `source ~/.cargo/env && cargo test -p workspace-core && cargo test -p devbox-workspace --lib` → PASS(이동 후 Workspace 테스트 그대로 통과). `git add -A && git commit -m "refactor(devbox-workspace): move the project host into a shared crate"`

---

### Task 2: agent에서 런타임 소유

**Files:** Create `apps/devbox-agent/src/runtime.rs`; Modify `apps/devbox-agent/{Cargo.toml,src/lib.rs,src/routes.rs}`

**Interfaces (Produces):** agent 라우트 `workspace.runtime`, `workspace.processes`, `workspace.process-actions`, `workspace.logs`(Workspace peer만); `runtime::initialize(app, workspace_data: &Path) -> Result<(), &'static str>`

- [ ] **Step 1: 실패하는 테스트** — `routes.rs`

```rust
    #[test]
    fn runtime_components_accept_only_workspace_peers() {
        let routes = Routes::with_runtime_for_tests();
        assert!(routes.accepts("workspace", "workspace.runtime"));
        assert!(routes.accepts("workspace", "workspace.logs"));
        assert!(!routes.accepts("knowledge", "workspace.runtime"));
        assert!(!routes.accepts("workspace", "workspace.files"), "files stay in the UI process");
    }
```

- [ ] **Step 2: 구현**
  - `runtime.rs::initialize`: agent의 설치 접미사로 Workspace 데이터 폴더(`%LOCALAPPDATA%\com.devbox.v08.workspace.i<접미사>`)를 찾고, `workspace_core::Host::open_read_only(root)`로 host를 연 뒤 Workspace `runtime_host::Owners`가 하던 초기화(`runtime_engine::component::initialize_with_sources(app, host.component("runtime"), host.component("common"), storage_root.parent(), Some(Sources{host}))`, `ports_engine::component::initialize`, `logs_engine::component::initialize(… RuntimeLogs …)`)를 그대로 한다. `RuntimeLogs`(로그 소스 검증)는 Task 1에서 core로 옮겼거나, 이 파일로 옮긴다.
  - agent는 제품과 같은 writer lease(`product_shell_tauri::WriterGuard`)를 잡아 업데이트가 agent를 기다리게 한다(P2-01의 `Shutdown`이 먼저 오므로 교착 없음).
  - `routes.rs`: 네 component를 등록하고 dispatch는 P1-14의 engine `api::dispatch`를 그대로 부른다(`admit`와 같은 입장 검사는 agent 쪽 `admit_remote(peer_session, header, call)`로: 세션은 peer 연결의 `Hello.session`, deadline·replay·lane은 P1-11·P1-14와 같은 코드).
- [ ] **Step 3: 확인·커밋** — Run: `cargo test -p devbox-agent --lib` → PASS. `git commit -am "feat(suite): host the runtime engines in devbox-agent"`

---

### Task 3: Workspace 전달과 소유 판정

**Files:** `apps/devbox-workspace/src-tauri/src/{ipc/runtime.rs,runtime_host.rs,ipc/mod.rs}`

**Interfaces (Produces):** `RuntimeOwner { Agent(AgentClient), Local }`와 `fn runtime_owner(app) -> RuntimeOwner`(agent 연결 성공 → Agent, `Unsupported` → Local, `Unavailable` → 오류 `runtime_agent_unavailable`)

- [ ] **Step 1: 실패하는 테스트** — `ipc/runtime.rs`

```rust
    #[test]
    fn ownership_follows_agent_availability() {
        assert_eq!(decide_owner(AgentAvailability::Connected), Ok(OwnerKind::Agent));
        assert_eq!(decide_owner(AgentAvailability::Unsupported), Ok(OwnerKind::Local));
        assert_eq!(decide_owner(AgentAvailability::Unavailable), Err("runtime_agent_unavailable"));
    }
```

  ("agent를 쓸 수 있는 설치인데 연결이 안 되면 로컬로 조용히 대체하지 않는다": 두 프로세스가 같은 런타임 DB를 열지 않게 하기 위해서다.)

- [ ] **Step 2: 구현**
  - `runtime`·`processes`·`process_actions`·`logs` command: `admit` 뒤 `decide_owner`가 Agent면 `agent_client.call(component, request_json)` 결과를 그대로 `Reply`로 돌려주고(`operation` provenance는 agent 응답을 쓰지 않고 이 host의 admission으로 다시 만든다), Local이면 지금 코드를 그대로 쓴다. `Unavailable`이면 `Unavailable` Problem과 issue `runtime_agent_unavailable`("백그라운드 서비스에 연결하지 못했습니다. 작업 상태에서 다시 시작해 주세요.").
  - `runtime_host::Owners::initialize_runtime`은 Local일 때만 부른다. `start_empty`에서 부르던 `owners.initialize_runtime(&app, &host)`도 같은 조건을 따른다.
  - Workspace 트레이의 "종료"는 더 이상 작업을 멈추지 않는다(작업은 agent 소유). 트레이 메뉴 문구를 "Workspace 닫기"로 바꾸고, `--background` 실행 모드는 agent가 대신하므로 제거한다(자동 시작은 P2-04의 agent 설정).
  - 로그 화면의 폴링(P1-15 `usePolling`)은 그대로 둔다(전달 경로만 바뀐다).
- [ ] **Step 3: 확인·커밋** — Run: `cargo test -p devbox-workspace --lib && pnpm --filter devbox-workspace exec vitest run` → PASS. `git commit -am "feat(devbox-workspace): forward runtime commands to devbox-agent"`

---

### Task 4: 수명 검사와 acceptance

- [ ] agent 종료 절차: 트레이가 아직 없으므로(P2-04) agent `Shutdown` 처리에서 런타임의 기존 종료 함수(Workspace `lifecycle`이 부르던 것: `runtime_engine::component::shutdown(app)` 등)를 부른 뒤 `app.exit(0)`. 테스트: `apps/devbox-agent/src/runtime.rs`에 "Shutdown이 오면 런타임 종료 함수를 한 번 부른다"를 종료 함수 주입으로 확인하는 단위 테스트를 둔다.
- [ ] candidate acceptance(`windows-suite-workflows.mjs` 또는 해당 스크립트): 설치본에서 작업을 시작하고 Workspace를 종료한 뒤 작업 프로세스가 계속 살아 있는지, Workspace를 다시 열면 실행 중 상태가 보이는지 확인하는 단계를 추가한다.
- [ ] 커밋: `git commit -am "test(suite): cover runtime ownership across Workspace restarts"`

---

### Task 5: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): 서비스 하나를 시작하고 Workspace를 닫은 뒤 해당 포트가 계속 응답, 다시 연 Workspace에서 로그가 이어서 보임, 1분 뒤 예약 작업을 만들고 Workspace를 닫아도 실행 기록이 남음, 작업 관리자에서 `devbox-agent.exe`를 끝내면 작업도 끝나고 다음 실행 때 기록이 "중단됨".
