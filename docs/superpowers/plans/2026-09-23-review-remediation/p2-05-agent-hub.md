# P2-05 Agent Hub(PR 2개) — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4와 `CONVENTIONS.md`의 "메서드 추가 절차"(P1-11)를 읽는다.

**Goal:** Workspace에 "에이전트" 화면을 추가한다. 작업마다 Git worktree를 만들고, 그 폴더에서 Claude Code·Codex(또는 직접 입력한 명령) 터미널을 열고, 결과를 Source 화면에서 검토한 뒤 원래 작업 폴더에 병합하거나 버린다(D25 1순위). 두 번째 PR에서 작업별 CPU·메모리와 토큰 사용량을 보여 준다.

**Architecture:**
- 조립 방식: 새 Git·터미널 구현을 만들지 않는다. 기존 부품(`workspace.source`의 `preview_worktree`·`create_worktree`, `workspace.registry`의 `preview_wsl`·`apply_registration`·`select_project`·`remove`, `workspace.terminal`의 터미널 창)을 프런트 흐름(`agentFlow.ts`)이 순서대로 부르고, native에는 빠진 조각만 더한다.
  - `workspace.agents` component(host 전용): 작업 기록 저장소(`agents` store의 `tasks.json`)와 상태 전이(`planned → created → ready → running → merged | discarded`).
  - `workspace.terminal.open_agent_terminal`: 작업 폴더에서 시작 명령을 자동 실행하는 한 칸짜리 터미널 창(`WorkspaceProfile.auto_run`).
  - `workspace.source.repo_merge`·`remove_agent_worktree`: 기본 작업 폴더에서 에이전트 branch 병합, 에이전트 worktree·branch 제거(`agent/` branch만).
- 단계마다 저장된 상태에서 다시 시작할 수 있다. 중간에 실패해도 같은 단계를 다시 부르면 중복 worktree·등록·터미널이 생기지 않는다(Registry의 `known` 판정, 터미널 operation id 재사용).
- Workspace는 "선택한 작업 폴더 하나"를 기준으로 동작한다(대부분의 command가 요청 context == 선택 context를 요구). 그래서 에이전트 작업을 열면 그 worktree를 선택하고, 병합·버리기는 기본 작업 폴더를 다시 선택한 뒤 실행한다. 에이전트 목록은 프로젝트 단위라 선택이 바뀌어도 그대로 보인다.
- 새 worktree의 Git 실행 승인은 이어받지 않는다. 승인 digest가 worktree 경로·식별자를 포함하므로(`source_host.rs` `Snapshot::capture`, helper `git_files.rs` `digest`) 같다고 볼 근거가 없다. "변경 검토"를 처음 열 때 기존 Source 화면의 승인 확인을 한 번 거친다.
- WSL 프로젝트만 지원한다(두 도구 모두 WSL에서 쓴다). Windows 프로젝트에서는 안내만 보인다.
- PR B: WSL helper에 `agent_resources`(작업 폴더 안에서 실행 중인 프로세스의 CPU tick·RSS 합)와 `agent_usage`(Claude Code·Codex 세션 기록의 토큰 합)를 더하고, 화면이 보일 때만 10초마다 갱신한다.

**Tech Stack:** Rust(Tauri v2, git CLI), TypeScript·React 19, ts-rs 12, Vitest

**Spec:** `review.md` §8 신규 기능 표 1번 · `00-roadmap.md` D25 · ADR 0015(터미널은 Workspace에 남음) · ADR 0016(보안 범위)

## Global Constraints

- `00-roadmap.md` §3 전부 적용. P1-11·P1-14의 타입 IPC 규칙(메서드 추가 절차, 허용 표 fixture, lane·deadline 예산, 생성 TS) 그대로.
- 기존 명령의 인자·결과 모양은 바꾸지 않는다. 새 필드는 serde 기본값(`#[serde(default)]`)으로만 더한다.
- `remove_agent_worktree`는 `agent/`로 시작하는 branch와 그 branch를 체크아웃한 연결 worktree에만 동작한다. 기본(main) worktree와 요청을 보낸 worktree는 지우지 않는다.
- 병합 충돌이면 `git merge --abort`로 기본 작업 폴더를 병합 전 상태로 되돌리고 충돌 파일 목록만 돌려준다(충돌 해결 화면은 P2-09).
- 사용량 파서는 알려진 필드만 읽는다. 모르는 줄·깨진 줄은 건너뛰고, 한도를 넘으면 `truncated: true`로 표시한다(오류로 화면을 막지 않는다).
- 새 의존성 없음.

## Review Focus

1. 기본 작업 폴더의 Git 승인이 없으면 흐름이 첫 단계에서 멈추고 "Source 화면에서 Git 설정을 확인" 안내와 이동 버튼을 보인다. 작업 기록은 `planned`로 남아 다시 시도하거나 지울 수 있다. (PR A Task A6 테스트)
2. 각 단계 실패 뒤 "다시 시도"가 저장된 상태부터 이어 가며 worktree·Registry 등록·터미널을 중복으로 만들지 않는다. (A2·A6 테스트)
3. 버리기는 `agent/` branch와 그 worktree만 지우고, 기본 worktree·다른 branch·요청한 worktree 자신은 거부한다. 병합 뒤 정리는 커밋하지 않은 변경이 있으면 거부한다(`--force` 없음). (A4 테스트, 실제 git)
4. 충돌하는 병합은 중단되고 기본 작업 폴더가 병합 전 HEAD·깨끗한 상태로 남는다. (A4 테스트, 실제 git)
5. 한글만 있는 제목은 `task-<초>` slug가 되고, 같은 제목을 두 번 쓰면 `-2`가 붙는다. 사용자 명령에 평문 자격증명이 있으면 저장 전에 거부한다. (A2 테스트)

---

## PR A — Agent Hub

- 묶음: **B10** — 브랜치 `feat/suite/agent-hub-and-mcp`, PR 제목 `feat(suite): agent hub and Devbox MCP server`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(devbox-workspace): agent hub for worktree-per-task coding agents`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

### Task A1: 카탈로그·저장소 등록

**Files:** Modify `apps/products.json`, `crates/catalog/src/products.rs`, `apps/devbox-workspace/src-tauri/src/core/stores.rs`(P2-02 뒤에는 `crates/workspace-core/src/stores.rs`), 하드코딩된 catalog revision이 있는 fixture·테스트

- [ ] **Step 1: 실패하는 테스트** — `crates/catalog/tests/catalog.rs`

```rust
#[test]
fn workspace_has_an_agents_route_backed_by_the_agents_component() {
    let catalog = catalog::products::ProductCatalog::parse(catalog::products::SOURCE).unwrap();
    let feature = catalog.features.iter().find(|f| f.id == "workspace.agents").expect("agents feature");
    assert_eq!((feature.route.as_str(), feature.label.as_str()), ("agents", "에이전트"));
    let component = catalog.components.iter().find(|c| c.id == "workspace.agents").expect("agents component");
    assert_eq!(component.authority, "agent-task");
}
```

  `stores.rs` 테스트 모듈:

```rust
    #[test]
    fn agents_is_an_additional_component_created_on_demand() {
        assert!(COMPONENTS.contains(&"agents"));
        assert!(ADDITIONAL_COMPONENTS.contains(&"agents"));
    }
```

- [ ] **Step 2: 실패 확인** — Run: `source ~/.cargo/env && cargo test -p catalog --test catalog workspace_has_an_agents_route && cargo test -p devbox-workspace --lib agents_is_an_additional_component` → FAIL
- [ ] **Step 3: 구현**
  - `apps/products.json`: `components`에 `{"id":"workspace.agents","owner":"workspace","authority":"agent-task","lifecycle":"product","protocolVersion":1}`, `features`에 `{"id":"workspace.agents","owner":"workspace","route":"agents","label":"에이전트","component":"workspace.shell","command":"workspace.open-agents","authority":"shell-read","status":"foundation"}`(`workspace.terminal` 뒤). `catalogRevision` +1. `rg -n "catalogRevision\"?:? ?[0-9]+|catalog_revision: [0-9]+" apps packages crates --glob '!**/node_modules/**'`로 찾은 옛 값을 새 값으로 바꾼다(P1-02 방법).
  - `products.rs` `component_authority`: `| ("workspace", "workspace.agents", "agent-task")`.
  - `stores.rs`: `COMPONENTS`와 `ADDITIONAL_COMPONENTS` 끝에 `"agents"`. 기존 generation에는 없어도 되고(`validate_generation`의 NotFound 허용), `ensure_runtime_components`가 처음 쓸 때 만든다.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS, `cargo test -p catalog && pnpm --filter @devbox/product-shell exec vitest run` → PASS. `git add -A && git commit -m "feat(devbox-workspace): register the agents route and store"`

---

### Task A2: 작업 이름 규칙과 작업 기록 저장소

**Files:** Create `apps/devbox-workspace/src-tauri/src/agent_hub/{mod.rs,plan.rs,store.rs}`; Modify `apps/devbox-workspace/src-tauri/src/lib.rs`(`mod agent_hub;`)

**Interfaces (Produces):**
- `plan::AgentTool { ClaudeCode, Codex, Custom }`(serde camelCase, `ts_rs::TS`), `plan::slug(title: &str, now_ms: u64) -> String`, `plan::unique_slug(base: &str, taken: &HashSet<String>) -> Result<String, AgentIssue>`, `plan::branch(slug: &str) -> String`(`agent/<slug>`), `plan::default_target_dir(base_root: &str, slug: &str) -> Result<String, AgentIssue>`, `plan::tool_command(tool: AgentTool, custom: Option<&str>) -> Result<String, AgentIssue>`, `plan::agent_layout(task: &AgentTask, distro: &str) -> WorkspaceProfile`
- `store::AgentTask { id, revision: u64, project_id, base_worktree_id, title, tool, command, branch, target_dir, worktree_id: Option<String>, terminal_id: Option<String>, state: AgentTaskState, created_at_ms: u64, updated_at_ms: u64 }`(camelCase, TS), `store::AgentTaskState { Planned, Created, Ready, Running, Merged, Discarded }`(camelCase), `store::AgentOutcome { Merged, Discarded }`, `store::Change { WorktreeCreated { path: String }, WorktreeBound { worktree_id: String }, TerminalOpened { terminal_id: String }, Finished { outcome: AgentOutcome } }`
- `store::AgentTaskStore::open(dir: &Path) -> Self`, `list(&self, project_id: &str) -> Result<Vec<AgentTask>, AgentIssue>`(최근 생성 순), `get(&self, id: &str)`, `insert(&self, task: AgentTask) -> Result<AgentTask, AgentIssue>`, `apply(&self, id: &str, expected_revision: u64, change: Change, now_ms: u64) -> Result<AgentTask, AgentIssue>`, `forget(&self, id: &str, expected_revision: u64) -> Result<(), AgentIssue>`
- `mod.rs`: `issue_codes! { pub enum AgentIssue { TaskMissing = "agent_task_missing", TaskChanged = "agent_task_changed", StateInvalid = "agent_task_state_invalid", TaskLimit = "agent_task_limit", TitleInvalid = "agent_title_invalid", CommandInvalid = "agent_command_invalid", TargetInvalid = "agent_target_invalid", SlugExhausted = "agent_slug_exhausted", WslRequired = "agent_wsl_required", ContextMismatch = "agent_task_context_mismatch", StoreUnavailable = "agent_store_unavailable" } }`

- [ ] **Step 1: 실패하는 테스트** — `plan.rs` 테스트 모듈

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn slugs_are_ascii_and_fall_back_to_time_for_non_ascii_titles() {
        assert_eq!(slug("Fix login: retry 3x!", 0), "fix-login-retry-3x");
        assert_eq!(slug("  --Hello__World--  ", 0), "hello-world");
        assert_eq!(slug("로그인 고치기", 1_790_000_000_123), "task-1790000000");
        assert!(slug(&"a".repeat(200), 0).len() <= MAX_SLUG_BYTES);
    }

    #[test]
    fn duplicate_slugs_get_numbered_suffixes() {
        let taken: HashSet<String> = ["fix".into(), "fix-2".into()].into();
        assert_eq!(unique_slug("fix", &taken).unwrap(), "fix-3");
        assert_eq!(unique_slug("new", &taken).unwrap(), "new");
        let full: HashSet<String> = (1..=99).map(|n| if n == 1 { "x".into() } else { format!("x-{n}") }).collect();
        assert_eq!(unique_slug("x", &full), Err(AgentIssue::SlugExhausted));
    }

    #[test]
    fn default_target_is_a_sibling_folder_or_reuses_a_worktrees_parent() {
        assert_eq!(default_target_dir("/home/me/projects/devbox", "fix").unwrap(), "/home/me/projects/devbox-fix");
        assert_eq!(default_target_dir("/home/me/projects/.worktrees/devbox-a", "fix").unwrap(), "/home/me/projects/.worktrees/devbox-a-fix");
        assert_eq!(default_target_dir("/", "fix"), Err(AgentIssue::TargetInvalid));
        assert_eq!(default_target_dir("relative/path", "fix"), Err(AgentIssue::TargetInvalid));
    }

    #[test]
    fn tool_commands_are_fixed_or_validated() {
        assert_eq!(tool_command(AgentTool::ClaudeCode, None).unwrap(), "claude");
        assert_eq!(tool_command(AgentTool::Codex, None).unwrap(), "codex");
        assert_eq!(tool_command(AgentTool::Custom, Some("aider --model x")).unwrap(), "aider --model x");
        assert_eq!(tool_command(AgentTool::Custom, None), Err(AgentIssue::CommandInvalid));
        assert_eq!(tool_command(AgentTool::Custom, Some("tool --token=sk-abcdefghijklmnopqrstuvwx")), Err(AgentIssue::CommandInvalid));
        assert_eq!(tool_command(AgentTool::ClaudeCode, Some("rm -rf /")), Err(AgentIssue::CommandInvalid));
    }

    #[test]
    fn agent_layout_is_one_auto_run_pane_in_the_worktree() {
        let task = crate::agent_hub::store::tests::task("t1", "Fix login");
        let layout = agent_layout(&task, "Ubuntu-24.04");
        layout.validate().unwrap();
        assert!(layout.auto_run);
        assert_eq!(layout.panes.len(), 1);
        assert_eq!(layout.panes[0].cwd.as_deref(), Some(task.target_dir.as_str()));
        assert_eq!(layout.panes[0].start_command.as_deref(), Some("claude"));
        assert_eq!(layout.panes[0].distro, "Ubuntu-24.04");
    }
}
```

  `store.rs` 테스트 모듈(`pub(crate) mod tests`로 두어 `task()` 도우미를 plan 테스트가 쓴다):

```rust
#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn task(id: &str, title: &str) -> AgentTask {
        AgentTask {
            id: id.into(), revision: 1, project_id: "p1".into(), base_worktree_id: "w1".into(), title: title.into(),
            tool: AgentTool::ClaudeCode, command: "claude".into(), branch: "agent/fix-login".into(),
            target_dir: "/home/me/projects/devbox-fix-login".into(), worktree_id: None, terminal_id: None,
            state: AgentTaskState::Planned, created_at_ms: 1, updated_at_ms: 1,
        }
    }

    #[test]
    fn tasks_move_forward_one_step_at_a_time_with_revision_checks() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentTaskStore::open(dir.path());
        let t = store.insert(task("t1", "Fix login")).unwrap();
        assert_eq!(
            store.apply("t1", t.revision, Change::WorktreeBound { worktree_id: "w2".into() }, 2),
            Err(AgentIssue::StateInvalid)
        );
        let t = store.apply("t1", t.revision, Change::WorktreeCreated { path: t.target_dir.clone() }, 2).unwrap();
        assert_eq!((t.state, t.revision), (AgentTaskState::Created, 2));
        assert_eq!(store.apply("t1", 1, Change::WorktreeBound { worktree_id: "w2".into() }, 3), Err(AgentIssue::TaskChanged));
        let t = store.apply("t1", 2, Change::WorktreeBound { worktree_id: "w2".into() }, 3).unwrap();
        let t = store.apply("t1", t.revision, Change::TerminalOpened { terminal_id: "00000000-0000-4000-8000-000000000001".into() }, 4).unwrap();
        let t = store.apply("t1", t.revision, Change::TerminalOpened { terminal_id: "00000000-0000-4000-8000-000000000002".into() }, 5).unwrap();
        assert_eq!(t.state, AgentTaskState::Running);
        let t = store.apply("t1", t.revision, Change::Finished { outcome: AgentOutcome::Merged }, 6).unwrap();
        assert_eq!(store.apply("t1", t.revision, Change::Finished { outcome: AgentOutcome::Discarded }, 7), Err(AgentIssue::StateInvalid));
        store.forget("t1", t.revision).unwrap();
        assert_eq!(store.get("t1"), Err(AgentIssue::TaskMissing));
    }

    #[test]
    fn worktree_path_must_match_the_plan_and_active_tasks_cannot_be_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentTaskStore::open(dir.path());
        let t = store.insert(task("t1", "Fix login")).unwrap();
        assert_eq!(store.apply("t1", 1, Change::WorktreeCreated { path: "/elsewhere".into() }, 2), Err(AgentIssue::StateInvalid));
        let t = store.apply("t1", t.revision, Change::WorktreeCreated { path: t.target_dir.clone() }, 2).unwrap();
        assert_eq!(store.forget("t1", t.revision), Err(AgentIssue::StateInvalid));
    }

    #[test]
    fn list_is_scoped_to_a_project_and_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        AgentTaskStore::open(dir.path()).insert(task("t1", "a")).unwrap();
        let mut other = task("t2", "b");
        other.project_id = "p2".into();
        AgentTaskStore::open(dir.path()).insert(other).unwrap();
        let listed = AgentTaskStore::open(dir.path()).list("p1").unwrap();
        assert_eq!(listed.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["t1"]);
    }

    #[test]
    fn a_corrupt_file_is_reported_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(TASKS_FILE), b"{not json").unwrap();
        assert_eq!(AgentTaskStore::open(dir.path()).list("p1"), Err(AgentIssue::StoreUnavailable));
        assert_eq!(std::fs::read(dir.path().join(TASKS_FILE)).unwrap(), b"{not json");
    }
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-workspace --lib agent_hub` → FAIL(모듈 없음)
- [ ] **Step 3: 구현**
  - `plan.rs`: `MAX_TITLE_CHARS = 120`, `MAX_SLUG_BYTES = 40`. `slug`: 영숫자는 소문자로 넣고, 그 밖의 문자는 연속이면 `-` 하나로 합치며 앞뒤 `-`를 없앤다. 40바이트에서 자른 뒤 끝의 `-`를 다시 없애고, 비면 `format!("task-{}", now_ms / 1000)`. `unique_slug`: `base`, `base-2` … `base-99` 중 `taken`에 없는 첫 값. `default_target_dir`: `/`로 시작하고 `/`가 아닌 경로만 받는다(아니면 `TargetInvalid`). 끝의 `/`를 없앤 뒤 부모와 이름으로 나눠 `{parent}/{name}-{slug}`를 돌려준다. 기준 폴더와 같은 부모 아래 새 폴더라 `preview_worktree`의 "기존 부모 폴더 아래 새 폴더" 조건을 항상 만족하고, 기준 폴더가 `.worktrees/` 아래 있으면 결과도 그 아래 생긴다. `tool_command`: `ClaudeCode`·`Codex`는 `custom`이 `None`일 때만 `"claude"`·`"codex"`, `Custom`은 `terminal_engine::core::workspace::validate_start_command`를 통과한 trim 값.
  - `agent_layout`은 `WorkspaceProfile::validate`를 통과해야 한다: `id: task.id.clone()`(`open_window`가 operation id로 덮어쓴다), 탭 하나(`id: "agent"`, `title: truncate_bytes(&task.title, 120)`, `custom_title: true`, `layout: Layout::Grid`, `pane_keys: vec!["agent"]`, `sizing: PaneSizing { columns: vec![1.0], rows: vec![1.0] }` — 기본값인 빈 비율은 검증에서 거부된다), 칸 하나(`key: "agent"`, `distro`, `cwd: Some(target_dir)`, `start_command: Some(command)`, `multiplexer: Native`), `name: format!("에이전트 · {}", truncate_bytes(&task.title, 104))`(이름 한도 120바이트, 접두어 16바이트), `active_tab_id: "agent"`, `active_pane_key: Some("agent".into())`, `auto_run: true`. `truncate_bytes(s, max)`는 char 경계에서 자르는 모듈 안 도우미다. `auto_run` 필드는 Task A5 Step 3에서 추가하므로, A2 Step 3을 시작하기 전에 그 필드 추가(한 줄)를 먼저 해 둔다.
  - `store.rs`: 파일 `TASKS_FILE = "tasks.json"`, 문서 `{ "schemaVersion": 1, "tasks": [...] }`(`deny_unknown_fields`, 알 수 없는 `schemaVersion`이면 `StoreUnavailable`). `MAX_TASKS = 200`(초과하면 `TaskLimit`). 모든 읽기·쓰기는 프로세스 전역 `static LOCK: Mutex<()>`를 잡고 "읽기 → 검증 → `devbox_filesystem::atomic_write`"로 한다(store 값은 요청마다 새로 연다). 파일이 없으면 빈 목록, 형식이 깨졌으면 `StoreUnavailable`(파일은 그대로 둔다). `apply` 검사 순서: 작업 없음 → `TaskMissing`, revision 다름 → `TaskChanged`, 그다음 전이 표(표에 없거나 추가 검사 실패 → `StateInvalid`):

| 현재 | 변경 | 다음 | 추가 검사 |
|---|---|---|---|
| Planned | WorktreeCreated{path} | Created | `path == target_dir` |
| Created | WorktreeBound{worktree_id} | Ready | 비어 있지 않은 id |
| Ready·Running | TerminalOpened{terminal_id} | Running | UUID 형식 |
| Created·Ready·Running | Finished{Discarded} | Discarded | — |
| Ready·Running | Finished{Merged} | Merged | — |

  그 밖은 `StateInvalid`. 성공하면 `revision += 1`, `updated_at_ms = now_ms`. `forget`은 `Planned`·`Merged`·`Discarded`만 지운다.
  - `mod.rs`: `pub mod plan; pub mod store;`, `AgentIssue`, `pub(crate) fn tasks(host: &Host) -> Result<AgentTaskStore, AgentIssue>`(= `AgentTaskStore::open(&host.component("agents")?)`, 실패는 `StoreUnavailable`).
- [ ] **Step 4: 확인·커밋** — Run: `cargo test -p devbox-workspace --lib agent_hub` → PASS. `git add -A && git commit -m "feat(devbox-workspace): agent task naming rules and store"`

---

### Task A3: `workspace.agents` command

**Files:** Create `apps/devbox-workspace/src-tauri/src/ipc/agents.rs`; Modify `ipc/mod.rs`, `lib.rs`(`generate_handler!`), `capabilities/*.json`, `tests/typescript.rs`, `tests/fixtures/allow-table.json`

**Interfaces (Produces):** `AgentsCall`(P1-11 `ActivityCall`과 같은 serde 속성의 인접 태그 enum): `List {}`, `Plan { title: String, tool: AgentTool, command: Option<String>, target_dir: Option<String> }`, `RecordWorktree { task_id: String, revision: u64, path: String }`, `BindWorktree { task_id: String, revision: u64, worktree_id: String }`, `Finish { task_id: String, revision: u64, outcome: AgentOutcome }`, `Forget { task_id: String, revision: u64 }`. 결과 map `AgentsResults { list: Vec<AgentTask>, plan: AgentTask, record_worktree: AgentTask, bind_worktree: AgentTask, finish: AgentTask, forget: () }`. `ComponentCall`: `COMPONENT = "workspace.agents"`, `routes() = ["agents"]`, `class() = Normal`, `lane() = Lane::Metadata`, `deadline_budget_ms() = DEFAULT_BUDGET_MS`.

- [ ] **Step 1: 실패하는 테스트** — `ipc/agents.rs` 테스트 모듈(host 없이 검증 함수만 부른다)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use product_contract::{ExecutionTarget, ProjectContext};

    fn wsl(worktree: &str) -> ProjectContext {
        ProjectContext { project_id: "p1".into(), worktree_id: worktree.into(), revision: 1, target: ExecutionTarget::Wsl { distro_id: "d1".into() } }
    }

    #[test]
    fn calls_parse_from_the_wire_shape() {
        let call: AgentsCall = serde_json::from_value(serde_json::json!({"method":"plan","args":{"title":"Fix login","tool":"claudeCode"}})).unwrap();
        assert_eq!(call.method(), "plan");
        assert!(serde_json::from_value::<AgentsCall>(serde_json::json!({"method":"plan","args":{"title":"x","tool":"claudeCode","extra":1}})).is_err());
    }

    #[test]
    fn planning_needs_a_wsl_context_and_a_valid_title() {
        let windows = ProjectContext { target: ExecutionTarget::Windows, ..wsl("w1") };
        assert_eq!(check_plan_context(Some(&windows)), Err(AgentIssue::WslRequired));
        assert_eq!(check_plan_context(None), Err(AgentIssue::WslRequired));
        assert!(check_plan_context(Some(&wsl("w1"))).is_ok());
        assert_eq!(check_title(""), Err(AgentIssue::TitleInvalid));
        assert_eq!(check_title(&"가".repeat(121)), Err(AgentIssue::TitleInvalid));
        assert_eq!(check_title("a\nb"), Err(AgentIssue::TitleInvalid));
        assert_eq!(check_title("  Fix login  ").unwrap(), "Fix login");
    }

    #[test]
    fn binding_requires_the_planned_folder_in_the_same_project() {
        let task = crate::agent_hub::store::tests::task("t1", "Fix login");
        assert!(check_binding(&task, "p1", &task.target_dir).is_ok());
        assert_eq!(check_binding(&task, "p2", &task.target_dir), Err(AgentIssue::ContextMismatch));
        assert_eq!(check_binding(&task, "p1", "/other"), Err(AgentIssue::ContextMismatch));
    }
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-workspace --lib ipc::agents` → FAIL
- [ ] **Step 3: 구현** — command `agents`(P1-14 command와 같은 모양: `admit` → `runtime.enter(call.lane())` → `spawn_blocking` 안에서 처리 → `admission.finish(result, classify)`). 처리:
  - `List`: `header.context`가 없으면 빈 목록, 있으면 `tasks(host)?.list(&context.project_id)`.
  - `Plan`: `check_plan_context(header.context)`, `check_title`, `plan::tool_command`. `base_root = host.projects()?.binding(context)?.root`. `taken` = 같은 프로젝트 작업들의 branch에서 `agent/`를 뗀 slug 집합. `slug = unique_slug(&plan::slug(title, now), &taken)?`. `target_dir`: 입력이 있으면 `/`로 시작하는 절대 경로이고 `base_root` 아래가 아닐 때만 받고(아니면 `TargetInvalid`), 없으면 `default_target_dir(&base_root, &slug)`. `id = Uuid::new_v4()`, `state = Planned`로 `insert`.
  - `RecordWorktree`: `apply(WorktreeCreated{path})`.
  - `BindWorktree`: Registry snapshot에서 `worktree_id`를 찾고 `check_binding(&task, &worktree.project_id, &worktree.binding.root)` 뒤 `apply(WorktreeBound)`.
  - `Finish`·`Forget`: 그대로 전달.
  - `ipc/mod.rs`의 command → component map, `lib.rs` `generate_handler!`에 `ipc::agents`, capability 파일에 `allow-agents` 권한을 더한다.
  - `tests/typescript.rs`에 `AgentsCall`, `AgentTask`, `AgentTaskState`, `AgentTool`, `AgentOutcome`, `AgentIssue`, `agents-results.ts`를 추가한다(출력 `packages/workspace-features/src/generated`).
  - 허용 표 fixture: `UPDATE_ALLOW_TABLE=1 cargo test -p devbox-workspace --lib allow_table`로 다시 쓰고, `git diff tests/fixtures/allow-table.json`이 `workspace.agents` 행 6개만 늘었는지 확인한다.
- [ ] **Step 4: 확인·커밋** — Run: `cargo test -p devbox-workspace --lib ipc && cargo test -p devbox-workspace --test typescript && bash .github/scripts/check-generated-bindings.sh` → PASS. `git add -A && git commit -m "feat(devbox-workspace): agents component commands"`

---

### Task A4: 병합과 에이전트 worktree 제거

**Files:** Modify `crates/repositories-engine/src/commands.rs`(요청·함수), `crates/repositories-engine/src/api.rs`(P1-14의 `SourceCall`), `crates/wsl-helper/src/control.rs`(P1-10 뒤 경로; `source_method`), `apps/devbox-workspace/src-tauri/src/ipc/source.rs`(routes에 `agents`), `packages/workspace-features/src/issues/source.ts`

**Interfaces (Produces):**
- `MergeRequest { path: String, branch: String, operation_id: String }`, `MergeResult { merged: bool, head: String, conflicts: Vec<String> }`, `pub async fn repo_merge(request: MergeRequest) -> Result<MergeResult, String>`
- `RemoveAgentWorktreeRequest { path: String, worktree: String, branch: String, force: bool, operation_id: String }`, `pub async fn remove_agent_worktree(request: RemoveAgentWorktreeRequest) -> Result<(), String>`
- `SourceCall::RepoMerge { request: MergeRequest }`, `SourceCall::RemoveAgentWorktree { request: RemoveAgentWorktreeRequest }`; 오류 코드 `source_merge_dirty`, `source_merge_failed`, `worktree_not_agent`, `worktree_remove_dirty`, `worktree_branch_unmerged`
- `SourceCall::routes()`와 host terminal enum의 `routes()`, registry enum의 `routes()`에 `"agents"` 추가

- [ ] **Step 1: 실패하는 테스트** — `commands.rs` 테스트 모듈(기존 `read_only_history_detail_and_dirty_diff_smoke_use_real_git_output`과 같은 방식)

```rust
    fn git(repo: &Path, args: &[&str]) -> String {
        let out = Command::new("git").args(args).current_dir(repo).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    }

    fn repo_with_agent_branch(file_on_agent: &str, file_on_main: Option<&str>) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("devbox");
        fs::create_dir(&main).unwrap();
        git(&main, &["init", "--quiet", "-b", "main"]);
        for (key, value) in [("user.email", "hub@example.test"), ("user.name", "Hub")] {
            git(&main, &["config", key, value]);
        }
        fs::write(main.join("shared.txt"), "base\n").unwrap();
        git(&main, &["add", "shared.txt"]);
        git(&main, &["commit", "--quiet", "-m", "base"]);
        let agent = tmp.path().join("devbox-fix");
        git(&main, &["worktree", "add", "--quiet", "-b", "agent/fix", agent.to_str().unwrap()]);
        fs::write(agent.join("shared.txt"), file_on_agent).unwrap();
        git(&agent, &["commit", "--quiet", "-am", "agent change"]);
        if let Some(content) = file_on_main {
            fs::write(main.join("shared.txt"), content).unwrap();
            git(&main, &["commit", "--quiet", "-am", "main change"]);
        }
        (tmp, main, agent)
    }

    #[test]
    fn merge_creates_a_merge_commit_on_the_base_worktree() {
        let (_tmp, main, _agent) = repo_with_agent_branch("agent\n", None);
        let result = crate::runtime::block_on(repo_merge(MergeRequest {
            path: main.to_string_lossy().into(), branch: "agent/fix".into(), operation_id: "merge-1".into(),
        })).unwrap();
        assert!(result.merged && result.conflicts.is_empty());
        assert_eq!(fs::read_to_string(main.join("shared.txt")).unwrap(), "agent\n");
        assert_eq!(git(&main, &["rev-list", "--count", "--merges", "HEAD"]).trim(), "1");
    }

    #[test]
    fn conflicting_merge_is_aborted_and_reports_paths() {
        let (_tmp, main, _agent) = repo_with_agent_branch("agent\n", Some("main\n"));
        let before = git(&main, &["rev-parse", "HEAD"]);
        let result = crate::runtime::block_on(repo_merge(MergeRequest {
            path: main.to_string_lossy().into(), branch: "agent/fix".into(), operation_id: "merge-2".into(),
        })).unwrap();
        assert!(!result.merged);
        assert_eq!(result.conflicts, vec!["shared.txt".to_string()]);
        assert_eq!(git(&main, &["rev-parse", "HEAD"]), before);
        assert!(git(&main, &["status", "--porcelain"]).is_empty());
        assert!(!main.join(".git/MERGE_HEAD").exists());
    }

    #[test]
    fn merge_refuses_a_dirty_base_worktree() {
        let (_tmp, main, _agent) = repo_with_agent_branch("agent\n", None);
        fs::write(main.join("wip.txt"), "wip\n").unwrap();
        let error = crate::runtime::block_on(repo_merge(MergeRequest {
            path: main.to_string_lossy().into(), branch: "agent/fix".into(), operation_id: "merge-3".into(),
        })).unwrap_err();
        assert_eq!(error, "source_merge_dirty");
    }

    #[test]
    fn removal_is_limited_to_agent_branches_and_linked_worktrees() {
        let (_tmp, main, agent) = repo_with_agent_branch("agent\n", None);
        let request = |worktree: &Path, branch: &str, force: bool| RemoveAgentWorktreeRequest {
            path: main.to_string_lossy().into(), worktree: worktree.to_string_lossy().into(),
            branch: branch.into(), force, operation_id: "remove-1".into(),
        };
        assert_eq!(crate::runtime::block_on(remove_agent_worktree(request(&main, "main", true))).unwrap_err(), "worktree_not_agent");
        assert_eq!(crate::runtime::block_on(remove_agent_worktree(request(&main, "agent/fix", true))).unwrap_err(), "worktree_not_agent");
        git(&main, &["merge", "--quiet", "--no-edit", "agent/fix"]);
        fs::write(agent.join("uncommitted.txt"), "x\n").unwrap();
        assert_eq!(crate::runtime::block_on(remove_agent_worktree(request(&agent, "agent/fix", false))).unwrap_err(), "worktree_remove_dirty");
        assert!(agent.exists());
        crate::runtime::block_on(remove_agent_worktree(request(&agent, "agent/fix", true))).unwrap();
        assert!(!agent.exists());
        assert!(git(&main, &["branch", "--list", "agent/fix"]).is_empty());
    }

    #[test]
    fn a_merged_clean_worktree_is_removed_without_force() {
        let (_tmp, main, agent) = repo_with_agent_branch("agent\n", None);
        git(&main, &["merge", "--quiet", "--no-edit", "agent/fix"]);
        crate::runtime::block_on(remove_agent_worktree(RemoveAgentWorktreeRequest {
            path: main.to_string_lossy().into(), worktree: agent.to_string_lossy().into(),
            branch: "agent/fix".into(), force: false, operation_id: "remove-3".into(),
        })).unwrap();
        assert!(!agent.exists());
        assert!(git(&main, &["branch", "--list", "agent/fix"]).is_empty());
    }

    #[test]
    fn safe_removal_keeps_an_unmerged_branch() {
        let (_tmp, main, agent) = repo_with_agent_branch("agent\n", None);
        let error = crate::runtime::block_on(remove_agent_worktree(RemoveAgentWorktreeRequest {
            path: main.to_string_lossy().into(), worktree: agent.to_string_lossy().into(),
            branch: "agent/fix".into(), force: false, operation_id: "remove-2".into(),
        })).unwrap_err();
        assert_eq!(error, "worktree_branch_unmerged");
        assert!(!git(&main, &["branch", "--list", "agent/fix"]).is_empty());
        assert!(agent.exists(), "an unmerged branch keeps its worktree too");
    }
```

  `control.rs` 테스트: `assert!(source_method("repo_merge") && source_method("remove_agent_worktree"));`

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-repositories-engine --lib -- merge_ conflicting_merge removal_ safe_removal a_merged_clean && cargo test -p workspace-wsl --lib source_method` → FAIL
- [ ] **Step 3: 구현** — 두 함수 모두 기존 mutation 함수(`repo_commit`)처럼 `begin_git_operation(&operation_id, …)`로 작업을 등록하고 `spawn_git_task` 안에서 `validated_git_path(&path)`로 저장소를 확인한 뒤 `run_git_mutation`/`devbox_git::run_bounded_target`을 쓴다.
  - `repo_merge`: `valid_worktree_branch(&branch)`가 아니면 `worktree_branch_invalid`. `git status --porcelain`(기존 `run_git_status_bounded(&git_status_changes_args(), …)`)이 비어 있지 않으면 `source_merge_dirty`. `git --no-pager merge --no-ff --no-edit <branch>`를 실행한다. 실패하면 `git --no-pager diff --name-only --diff-filter=U -z`로 충돌 경로를 모으고(NUL 구분, 최대 200개) `git merge --abort`를 실행한다. 충돌 경로가 있으면 `Ok(MergeResult { merged: false, head: <현재 HEAD>, conflicts })`, 없으면 `source_merge_failed`. 성공이면 `head = git rev-parse HEAD`.
  - `remove_agent_worktree`: `branch`가 `agent/`로 시작하고 `valid_worktree_branch`를 통과해야 한다(아니면 `worktree_not_agent`). `git worktree list --porcelain`을 `core::cleanup::parse_worktree_records`로 읽어, 첫 기록(기본 worktree)과 `path` 자신이 아니며 경로가 `worktree`(기존 `host_path_from_git`으로 맞춘 표기)이고 branch가 `refs/heads/<branch>`인 기록이 있어야 한다(아니면 `worktree_not_agent`). `force`면 `git worktree remove --force <worktree>` 후 `git branch -D <branch>`. 아니면 먼저 `git merge-base --is-ancestor refs/heads/<branch> HEAD`(종료 코드 0이 아니면 아무것도 바꾸지 않고 `worktree_branch_unmerged`), 이어서 `git worktree remove <worktree>`(실패 → `worktree_remove_dirty`), `git branch -d <branch>`.
  - `api.rs` `SourceCall`에 두 variant(lane `Source`, 예산 `LONG_BUDGET_MS`), `SOURCE_COMMANDS`에 두 이름, helper `control::source_method`에 두 이름을 더한다. 경로 인자는 기존 규칙대로 `request.path`가 context root와 같아야 한다(`dispatch_source_native`).
  - `SourceCall`, host terminal enum, registry enum의 `routes()`에 `"agents"`를 더하고 허용 표 fixture를 다시 쓴 뒤 diff가 `agents` route 행과 두 source 메서드 행만인지 확인한다.
  - `packages/workspace-features/src/issues/source.ts`에 문구: `source_merge_dirty: "기본 작업 폴더에 커밋하지 않은 변경이 있습니다. 커밋하거나 정리한 뒤 병합해 주세요."`, `source_merge_failed: "병합을 완료하지 못했습니다. 기본 작업 폴더의 Git 상태를 확인해 주세요."`, `worktree_not_agent: "에이전트가 만든 작업 폴더와 branch만 정리할 수 있습니다."`, `worktree_remove_dirty: "작업 폴더에 커밋하지 않은 변경이 있습니다. 변경을 확인하거나 '버리기'를 사용해 주세요."`, `worktree_branch_unmerged: "branch가 아직 병합되지 않아 지우지 않았습니다."`
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS, `cargo test -p devbox-workspace --lib allow_table` → PASS. `git add -A && git commit -m "feat(devbox-workspace): merge and remove agent worktrees"`

---

### Task A5: 자동 실행 터미널 창

**Files:** Modify `crates/terminal-engine/src/core/workspace.rs`(`WorkspaceProfile.auto_run`), `apps/devbox-workspace/src-tauri/src/terminal_host.rs`(`open_agent_terminal`, 프로필 저장 때 `auto_run` 끄기), `apps/devbox-workspace/src-tauri/src/ipc/terminal.rs`(host enum variant), `packages/workspace-features/src/terminal/{App.tsx,lib/workspace.ts}`

**Interfaces (Produces):** `WorkspaceProfile { …, #[serde(default, skip_serializing_if = "std::ops::Not::not")] pub auto_run: bool }`, host terminal call `OpenAgentTerminal { operation_id: String, task_id: String }`(lane `Terminal`, 예산 `LONG_BUDGET_MS`)

- [ ] **Step 1: 실패하는 테스트**
  - `workspace.rs`:

```rust
    #[test]
    fn auto_run_defaults_off_and_is_omitted_when_off() {
        let profile: WorkspaceProfile = serde_json::from_value(serde_json::json!({
            "name":"p","tabs":[{"id":"t","title":"t","layout":"grid","paneKeys":["a"]}],
            "panes":[{"key":"a","distro":"Ubuntu"}],"activeTabId":"t"
        })).unwrap();
        assert!(!profile.auto_run);
        assert!(serde_json::to_value(&profile).unwrap().get("autoRun").is_none());
    }
```

  - `terminal_host.rs` 테스트 모듈(순수 함수): `saved_profile(layout)`이 `auto_run`을 `false`로 바꾼다.

```rust
    #[test]
    fn saving_an_agent_layout_as_a_profile_turns_auto_run_off() {
        let task = crate::agent_hub::store::tests::task("t1", "Fix login");
        let layout = crate::agent_hub::plan::agent_layout(&task, "Ubuntu");
        assert!(!saved_profile(layout).auto_run);
    }
```

  - `packages/workspace-features/src/terminal/App.autorun.test.tsx`: `autoRun: true` 레이아웃을 받은 companion은 확인 대화 없이 `write_initial_command`를 한 번 보내고, `autoRun`이 없는 레이아웃은 기존처럼 "시작 명령 1개를 실행할까요?"를 묻는다. 복원 전용(`isRestoreOnly`)이면 `autoRun`이어도 보내지 않는다. (기존 `App.applink.test.tsx`의 mock 방식을 따른다.)
- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-terminal-engine --lib auto_run && cargo test -p devbox-workspace --lib saving_an_agent_layout && pnpm --filter @devbox/workspace-features exec vitest run src/terminal/App.autorun.test.tsx` → FAIL
- [ ] **Step 3: 구현**
  - `WorkspaceProfile`에 `auto_run` 필드. 프로필 저장(`save_workspace_profile`)과 `ProfileStore::upsert`로 들어가는 값은 `saved_profile()`로 `auto_run = false`로 만든다(사용자가 저장한 프로필은 계속 확인을 묻는다).
  - `open_agent_terminal`(`terminal_host.rs`의 dispatch match에 추가):

```rust
            "open_agent_terminal" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    operation_id: String,
                    task_id: String,
                }
                let input: Input = parse(args)?;
                if !id(&input.operation_id) {
                    return Err("terminal_operation_invalid");
                }
                let context = header.context.as_ref().ok_or("project_selection_required")?;
                let tasks = crate::agent_hub::tasks(host).map_err(|issue| issue.code())?;
                let task = tasks.get(&input.task_id).map_err(|issue| issue.code())?;
                if task.project_id != context.project_id || task.worktree_id.as_deref() != Some(context.worktree_id.as_str()) {
                    return Err("agent_task_context_mismatch");
                }
                let distro = crate::agent_hub::distro_name(context)?;
                let layout = crate::agent_hub::plan::agent_layout(&task, &distro);
                let value = self.open_prepared(window, host, header, &input.operation_id, Some(layout))?;
                tasks
                    .apply(&task.id, task.revision, crate::agent_hub::store::Change::TerminalOpened { terminal_id: input.operation_id.clone() }, now_ms())
                    .map_err(|issue| issue.code())?;
                Ok(value)
            }
```

    같은 operation id로 다시 부르면 `open_window`가 기존 기록을 돌려주고(`prior` 분기), 작업이 이미 `Running`이고 `terminal_id`가 같으면 `apply`를 건너뛴다(중복 전이 방지). `now_ms()`는 `SystemTime::now()`의 UNIX 밀리초를 돌려주는 `agent_hub` 도우미다. `agent_hub::distro_name(context)`는 `session_preflight.rs:251`과 같은 방법(`platform::wsl_distro::list()`에서 `distro_id`로 이름 찾기, 못 찾으면 `terminal_distro_unavailable`)으로 만든다.
  - companion `App.tsx` 1031행 근처: `const runStartCommands = !isRestoreOnly() && (commands.length === 0 || workspace.autoRun === true || (await ask({...})).confirmed);`. `lib/workspace.ts`의 레이아웃 타입·파서에 `autoRun?: boolean`을 허용한다.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS. `git add -A && git commit -m "feat(devbox-workspace): open agent terminals that start their tool"`

---

### Task A6: 에이전트 화면

**Files:** Create `apps/devbox-workspace/src/agents/{AgentHub.tsx,NewAgentTask.tsx,AgentTaskRow.tsx,agentFlow.ts,agentFlow.test.ts,AgentHub.test.tsx,api.ts,messages.ts}`; Modify `apps/devbox-workspace/src/Workspace.tsx`

**Interfaces (Produces):**
- `api.ts`: `agentsCall = typedCall<AgentsCall, AgentsResults>("workspace.agents")`와 흐름용 포트 구현 `nativePorts(): FlowPorts`(모두 `nativeCall`을 써서 **호출 시점의 현재 description**으로 보낸다)
- `agentFlow.ts`: `interface FlowPorts { agents: { recordWorktree(id: string, revision: number, path: string): Promise<AgentTask>; bindWorktree(id: string, revision: number, worktreeId: string): Promise<AgentTask> }; source: { previewWorktree(branch: string, targetDir: string): Promise<{ previewId: string }>; createWorktree(previewId: string, operationId: string): Promise<{ path: string }> }; registry: { previewWsl(distroId: string, root: string): Promise<{ previewId: string; discovery: { kind: string } }>; cancel(previewId: string): Promise<void>; apply(previewId: string, name: string): Promise<{ context: ProjectContext }>; select(context: ProjectContext): Promise<void> }; terminal: { openAgentTerminal(operationId: string, taskId: string): Promise<void> }; refreshContext(): Promise<void>; currentContext(): Promise<ProjectContext | null>; operationId(key: string): string; settle(key: string): void }`, `advance(task: AgentTask, ports: FlowPorts, env: { distroId: string; projectName: string; worktreeContext(worktreeId: string): ProjectContext | null }): Promise<AgentTask>`, `class AgentFlowError extends Error { code: string }`

- [ ] **Step 1: 실패하는 테스트** — `agentFlow.test.ts`

```ts
import { describe, expect, it, vi } from "vitest";
import { advance, AgentFlowError, type FlowPorts } from "./agentFlow";
import type { AgentTask } from "@devbox/workspace-features/generated/AgentTask";

const base = { projectId: "p1", worktreeId: "w1", revision: 1, target: { kind: "wsl" as const, distroId: "d1" } };
const agentContext = { ...base, worktreeId: "w2" };
function task(state: AgentTask["state"], extra: Partial<AgentTask> = {}): AgentTask {
  return { id: "t1", revision: 1, projectId: "p1", baseWorktreeId: "w1", title: "Fix login", tool: "claudeCode", command: "claude",
    branch: "agent/fix-login", targetDir: "/home/me/projects/devbox-fix-login", worktreeId: null, terminalId: null,
    state, createdAtMs: 1, updatedAtMs: 1, ...extra };
}
function ports(overrides: Partial<Record<string, unknown>> = {}) {
  let selected: typeof base | null = base;
  const p = {
    agents: {
      recordWorktree: vi.fn(async (_id, revision, path) => task("created", { revision: revision + 1, targetDir: path })),
      bindWorktree: vi.fn(async (_id, revision, worktreeId) => task("ready", { revision: revision + 1, worktreeId })),
    },
    source: {
      previewWorktree: vi.fn(async () => ({ previewId: "pv1" })),
      createWorktree: vi.fn(async () => ({ path: "/home/me/projects/devbox-fix-login" })),
    },
    registry: {
      previewWsl: vi.fn(async () => ({ previewId: "rg1", discovery: { kind: "linkedWorktree" } })),
      cancel: vi.fn(async () => {}),
      apply: vi.fn(async () => ({ context: agentContext })),
      select: vi.fn(async (context) => { selected = context; }),
    },
    terminal: { openAgentTerminal: vi.fn(async () => {}) },
    refreshContext: vi.fn(async () => {}),
    currentContext: vi.fn(async () => selected),
    operationId: vi.fn(() => "00000000-0000-4000-8000-000000000001"),
    settle: vi.fn(),
    ...overrides,
  };
  return p as unknown as FlowPorts & typeof p;
}
const env = { distroId: "d1", projectName: "devbox", worktreeContext: (id: string) => (id === "w2" ? agentContext : id === "w1" ? base : null) };

describe("agent flow", () => {
  it("runs every step once from a planned task", async () => {
    const p = ports();
    await advance(task("planned"), p, env);
    expect(p.source.createWorktree).toHaveBeenCalledWith("pv1", "t1");
    expect(p.registry.apply).toHaveBeenCalledWith("rg1", "devbox");
    expect(p.registry.select).toHaveBeenCalledWith(agentContext);
    expect(p.terminal.openAgentTerminal).toHaveBeenCalledWith("00000000-0000-4000-8000-000000000001", "t1");
    expect(p.settle).toHaveBeenCalledTimes(1);
  });

  it("resumes a created task at registration without creating another worktree", async () => {
    const p = ports();
    await advance(task("created"), p, env);
    expect(p.source.previewWorktree).not.toHaveBeenCalled();
    expect(p.registry.previewWsl).toHaveBeenCalledWith("d1", "/home/me/projects/devbox-fix-login");
  });

  it("accepts an already registered folder when a retry follows a lost reply", async () => {
    const p = ports();
    p.registry.previewWsl.mockResolvedValueOnce({ previewId: "rg2", discovery: { kind: "known" } });
    await advance(task("created"), p, env);
    expect(p.agents.bindWorktree).toHaveBeenCalledWith("t1", 1, "w2");
  });

  it("cancels an unexpected registration and stops", async () => {
    const p = ports();
    p.registry.previewWsl.mockResolvedValueOnce({ previewId: "rg3", discovery: { kind: "newProject" } });
    await expect(advance(task("created"), p, env)).rejects.toEqual(new AgentFlowError("agent_worktree_unexpected"));
    expect(p.registry.cancel).toHaveBeenCalledWith("rg3");
    expect(p.agents.bindWorktree).not.toHaveBeenCalled();
  });

  it("keeps the terminal operation id when opening fails so a retry is idempotent", async () => {
    const p = ports();
    p.terminal.openAgentTerminal.mockRejectedValueOnce(new Error("busy"));
    await expect(advance(task("ready", { worktreeId: "w2" }), p, env)).rejects.toThrow("busy");
    expect(p.settle).not.toHaveBeenCalled();
    await advance(task("ready", { worktreeId: "w2" }), p, env);
    expect(p.terminal.openAgentTerminal).toHaveBeenNthCalledWith(2, "00000000-0000-4000-8000-000000000001", "t1");
  });

  it("refuses to open a terminal when the selection did not switch", async () => {
    const p = ports({ currentContext: vi.fn(async () => base) });
    await expect(advance(task("ready", { worktreeId: "w2" }), p, env)).rejects.toEqual(new AgentFlowError("agent_context_changed"));
  });
});
```

  `AgentHub.test.tsx`(Testing Library + `@devbox/a11y/testing`의 axe, 기존 Workspace 화면 테스트의 `invoke` mock 방식):
  - Windows 프로젝트가 선택돼 있으면 "에이전트 작업은 WSL 프로젝트에서 사용할 수 있습니다." 안내만 보이고 폼이 없다.
  - 새 작업 폼(제목·도구 선택 "Claude Code"/"Codex"/"직접 입력"; "직접 입력"이면 명령 입력칸)을 제출하면 `plan` 다음 흐름이 시작되고, `preview_worktree`가 `source_review_required`로 실패하면 "기본 작업 폴더의 Git 설정을 먼저 확인해 주세요." 경고와 "소스 화면 열기" 버튼(누르면 `navigate("source")`)이 보이며 작업 행은 "준비 중 · 다시 시도/목록에서 지우기"로 남는다.
  - `running` 작업 행에는 "터미널 보기"(`focus_terminal {id: terminalId}`), "변경 검토"(작업 worktree 선택 → `navigate("source")`), "병합", "버리기"가 있다.
  - "병합" → 기본 worktree가 선택돼 있지 않으면 먼저 `select_project(base)`와 `refreshContext` → `repo_status {path: baseRoot}`로 현재 branch를 읽어 확인 영역("'agent/fix-login'을 기본 작업 폴더의 현재 branch '<branch>'에 병합합니다.")을 보인다 → 확인하면 `repo_merge {request: {path: baseRoot, branch, operationId}}` → 성공이면 "병합했습니다. 작업 폴더를 정리할까요?"와 "정리" 버튼(`stop_terminal` → `remove_agent_worktree {force:false}` → registry `remove` → `finish(merged)`), 충돌이면 "충돌 파일 N개" 목록과 안내.
  - "버리기" → 확인 영역("작업 폴더와 branch를 지웁니다. 커밋하지 않은 변경도 사라집니다.")에서 확인하면 `stop_terminal`(실행 중일 때) → 기본 worktree 선택 → `remove_agent_worktree {force:true}` → registry `remove` → `finish(discarded)`.
  - axe 위반 0.
- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter devbox-workspace exec vitest run src/agents` → FAIL
- [ ] **Step 3: 구현**
  - `agentFlow.ts`의 `advance`:

```ts
export class AgentFlowError extends Error {
  constructor(readonly code: string) { super(code); this.name = "AgentFlowError"; }
}

export async function advance(task: AgentTask, ports: FlowPorts, env: FlowEnv): Promise<AgentTask> {
  let current = task;
  if (current.state === "planned") {
    const preview = await ports.source.previewWorktree(current.branch, current.targetDir);
    const created = await ports.source.createWorktree(preview.previewId, current.id);
    current = await ports.agents.recordWorktree(current.id, current.revision, created.path);
  }
  if (current.state === "created") {
    const preview = await ports.registry.previewWsl(env.distroId, current.targetDir);
    if (preview.discovery.kind !== "linkedWorktree" && preview.discovery.kind !== "known") {
      await ports.registry.cancel(preview.previewId);
      throw new AgentFlowError("agent_worktree_unexpected");
    }
    const { context } = await ports.registry.apply(preview.previewId, env.projectName);
    current = await ports.agents.bindWorktree(current.id, current.revision, context.worktreeId);
  }
  if (current.state === "ready" || current.state === "running") {
    const target = current.worktreeId ? env.worktreeContext(current.worktreeId) : null;
    if (!target) throw new AgentFlowError("agent_task_context_mismatch");
    await ports.registry.select(target);
    await ports.refreshContext();
    const selected = await ports.currentContext();
    if (selected?.worktreeId !== target.worktreeId) throw new AgentFlowError("agent_context_changed");
    if (current.state === "ready") {
      const key = `workspace-agent-terminal:${current.id}`;
      await ports.terminal.openAgentTerminal(ports.operationId(key), current.id);
      ports.settle(key);
    }
  }
  return current;
}
```

    `running` 작업을 다시 `advance`하면 선택만 바꾼다(터미널을 다시 여는 것은 행의 "터미널 다시 열기" 버튼이 새 operation id로 부른다). `operationId(key)`/`settle(key)`는 `sessionStorage`에 UUID를 보관·삭제한다(`Terminal.tsx`의 `workspace-terminal-open:` 방식).
  - `apps/devbox-workspace/src/native.ts`의 component → command 표(P1-14)와 `Component` 유니온에 `"workspace.agents": "plugin:workspace|agents"`를 더한다(없으면 `typedCall`이 호출할 command를 찾지 못한다).
  - `api.ts` `nativePorts()`: source `previewWorktree` = `nativeCall("workspace.source", "preview_worktree", {branch, targetDir}, "agents")`, `createWorktree` = `nativeCall("workspace.source", "create_worktree", {previewId, operationId}, "agents")`, registry는 `preview_wsl {distroId, root, startStopped: false}`·`cancel_registration`·`apply_registration {previewId, name, action: "register"}`·`select_project {context}`(route `agents`), terminal `open_agent_terminal {operationId, taskId}`, `currentContext` = `(await currentDescription("workspace")).context ?? null`.
  - `AgentHub.tsx`(props: `description`, `registry`, `navigate`, `refreshContext`): `usePolling`(3초, 화면이 보일 때)으로 `agentsCall("list", {})`와 `terminal_sessions`를 읽어 행을 만든다. 오류는 `useOperation().run`과 `messages.ts`의 `Record<AgentIssue | "agent_worktree_unexpected" | "agent_context_changed", string>` 카탈로그로 보인다. 프로젝트 이름은 `registry.projects`, 기본 worktree context는 `registry.worktrees`에서 만든다. 컴포넌트는 P1-15 크기 한도(1,000줄) 안에서 `NewAgentTask`(폼)·`AgentTaskRow`(행과 병합·버리기 확인 영역)로 나눈다.
  - 상태 표시: `planned` "준비 중", `created` "작업 폴더 생성됨", `ready` "터미널 준비", `running` "실행 중"(터미널 기록이 `stopped`면 "터미널 종료됨"), `merged` "병합됨", `discarded` "버림". 끝난 작업(`merged`·`discarded`)은 "목록에서 지우기"(`forget`)만.
  - `Workspace.tsx`: `const AgentHub = lazy(() => import("./agents/AgentHub"));`와 `{ready && route === "agents" && <Suspense fallback={<p role="status">에이전트 작업을 불러오고 있습니다…</p>}><AgentHub description={description} registry={registry} navigate={navigate} refreshContext={refreshContext}/></Suspense>}`.
- [ ] **Step 4: 확인·커밋** — Run: `pnpm --filter devbox-workspace exec vitest run src/agents && pnpm --filter devbox-workspace exec tsc --noEmit` → PASS. `git add -A && git commit -m "feat(devbox-workspace): agent hub screen"`

---

### Task A7: PR 완료

- [ ] `docs/workspace-guide.md`(없으면 `docs/windows-guide.md`의 Workspace 절)에 "에이전트" 절: 작업 만들기, 터미널, 변경 검토(처음 한 번 Git 설정 확인), 병합·정리, 버리기.
- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): (1) WSL 프로젝트에서 "Claude Code" 작업을 만들면 새 폴더·branch가 생기고 터미널 창에서 `claude`가 바로 시작된다. (2) 작업 중 파일을 바꾸고 커밋한 뒤 "병합" → 기본 폴더에 merge commit, "정리" → 폴더·branch 삭제. (3) 다른 작업을 "버리기" → 커밋하지 않은 변경과 함께 삭제. (4) Codex 작업도 같은 흐름.

---

## PR B — 리소스·토큰 사용량

- 묶음: **B10** — 브랜치 `feat/suite/agent-hub-and-mcp`, PR 제목 `feat(suite): agent hub and Devbox MCP server`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(devbox-workspace): show agent CPU, memory and token usage`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).
- 순서: 같은 묶음 안에서 PR A 과제를 모두 마친 뒤.

### Task B1: helper 프로세스 표본

**Files:** Create `crates/wsl-helper/src/agent_resources.rs`; Modify `crates/wsl-helper/src/{lib.rs,control.rs,engine.rs}`

**Interfaces (Produces):** `ProcessSample { processes: u32, rss_bytes: u64, cpu_ticks: u64, clock_ticks_per_second: u64, truncated: bool }`(camelCase), `sample(proc_root: &Path, root: &Path, page_size: u64, clock_ticks: u64) -> ProcessSample`, helper project method `agent_resources { context }`

- [ ] **Step 1: 실패하는 테스트** — `agent_resources.rs`(가짜 `/proc` 트리. symlink를 쓰므로 Linux에서만 컴파일한다)

```rust
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::fs;

    fn process(proc_root: &Path, pid: &str, cwd: &Path, utime: u64, stime: u64, resident_pages: u64) {
        let dir = proc_root.join(pid);
        fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink(cwd, dir.join("cwd")).unwrap();
        // comm may contain spaces and parentheses; fields are read after the last ')'.
        fs::write(dir.join("stat"), format!("{pid} (node (x)) S 1 1 1 0 -1 0 0 0 0 0 {utime} {stime} 0 0 20 0 1 0 0 0 0\n")).unwrap();
        fs::write(dir.join("statm"), format!("1000 {resident_pages} 10 1 0 100 0\n")).unwrap();
    }

    #[test]
    fn sums_processes_whose_working_directory_is_inside_the_root() {
        let tmp = tempfile::tempdir().unwrap();
        let proc_root = tmp.path().join("proc");
        let root = tmp.path().join("devbox-fix");
        fs::create_dir_all(root.join("src")).unwrap();
        let elsewhere = tmp.path().join("devbox");
        fs::create_dir_all(&elsewhere).unwrap();
        process(&proc_root, "101", &root, 30, 20, 256);
        process(&proc_root, "102", &root.join("src"), 5, 5, 128);
        process(&proc_root, "103", &elsewhere, 999, 999, 999);
        fs::create_dir_all(proc_root.join("self")).unwrap();
        fs::create_dir_all(proc_root.join("104")).unwrap(); // exited between listing and reading
        let sample = sample(&proc_root, &root, 4096, 100);
        assert_eq!(sample.processes, 2);
        assert_eq!(sample.cpu_ticks, 60);
        assert_eq!(sample.rss_bytes, 384 * 4096);
        assert_eq!(sample.clock_ticks_per_second, 100);
        assert!(!sample.truncated);
    }

    #[test]
    fn a_sibling_with_the_same_prefix_is_not_inside() {
        let tmp = tempfile::tempdir().unwrap();
        let proc_root = tmp.path().join("proc");
        let root = tmp.path().join("devbox");
        let sibling = tmp.path().join("devbox-fix");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        process(&proc_root, "201", &sibling, 1, 1, 1);
        assert_eq!(sample(&proc_root, &root, 4096, 100).processes, 0);
    }
}
```

  `control.rs` 테스트: `assert!(project_method("agent_resources"));`
- [ ] **Step 2: 실패 확인** — Run: `cargo test -p workspace-wsl --lib -- agent_resources project_method` → FAIL
- [ ] **Step 3: 구현**
  - `sample`: `proc_root`의 항목 중 이름이 숫자인 것만 본다(최대 `MAX_PROCESSES = 32_768`개, 넘으면 `truncated`). `read_link(<pid>/cwd)`가 실패하면(권한·종료) 건너뛰고, `Path::starts_with(root)`(구성요소 단위라 `devbox`와 `devbox-fix`를 구분)일 때만 `stat`의 마지막 `)` 뒤 필드에서 12·13번째(utime·stime, 0부터 11·12)를, `statm`의 두 번째 값(resident pages)을 읽는다. 읽기·파싱 실패는 건너뛴다. 합은 `saturating_add`.
  - `engine.rs`: `"agent_resources"` 요청을 `dependency_inventory`처럼 처리하되 files attach를 요구하지 않는다: 인자 `{context}`(`deny_unknown_fields`)가 WSL 대상이고 `distroId`가 token 형식인지 확인하고(`files_attach`와 같은 검사), root token의 `root.observation.revalidate()` 뒤 `sample(Path::new("/proc"), root.observation.root(), sysconf(_SC_PAGESIZE), sysconf(_SC_CLK_TCK))`(`libc`, 값이 0 이하이면 4096·100).
  - `control.rs` `project_method`에 `"agent_resources"`.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS. `git add -A && git commit -m "feat(devbox-workspace): sample agent process resources in the WSL helper"`

---

### Task B2: helper 토큰 사용량

**Files:** Create `crates/wsl-helper/src/agent_usage.rs`, `crates/wsl-helper/tests/fixtures/agent-usage/{claude.jsonl,codex.jsonl}`; Modify `control.rs`, `engine.rs`

**Interfaces (Produces):** `TokenTotals { input: u64, output: u64, cache_read: u64, cache_write: u64, sessions: u32 }`(camelCase), `UsageReport { claude: Option<TokenTotals>, codex: Option<TokenTotals>, truncated: bool }`, `claude_project_dir(home: &Path, root: &str) -> PathBuf`, `claude_totals(lines: impl BufRead, totals: &mut TokenTotals, seen: &mut HashSet<String>)`, `codex_session(lines: impl BufRead, root: &Path) -> Option<TokenTotals>`, `usage(home: &Path, root: &Path, since_ms: u64, now_ms: u64) -> UsageReport`, helper project method `agent_usage { context, sinceMs }`

- [ ] **Step 1: fixture** — 실제 형식을 줄여 만든다.
  - `claude.jsonl`: `{"type":"assistant","requestId":"req_1","message":{"id":"msg_1","usage":{"input_tokens":6,"cache_creation_input_tokens":100,"cache_read_input_tokens":0,"output_tokens":20}}}` 두 줄(같은 `msg_1` 중복 — 스트리밍 기록이 같은 사용량을 반복한다), `msg_2`(input 4, cache_read 200, output 10) 한 줄, `{"type":"user","message":{"content":"hi"}}` 한 줄, 깨진 줄 `{not json` 한 줄.
  - `codex.jsonl`: `{"type":"session_meta","payload":{"cwd":"/home/me/projects/devbox-fix","id":"s1"}}`, `{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":7,"total_tokens":107}}}}`, 두 번째 token_count(input 300, cached 120, output 30, total 330).
- [ ] **Step 2: 실패하는 테스트** — `agent_usage.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const CLAUDE: &str = include_str!("../tests/fixtures/agent-usage/claude.jsonl");
    const CODEX: &str = include_str!("../tests/fixtures/agent-usage/codex.jsonl");

    #[test]
    fn claude_project_directories_replace_every_non_alphanumeric_byte() {
        assert_eq!(
            claude_project_dir(Path::new("/home/me"), "/home/me/projects/.worktrees/devbox_fix"),
            Path::new("/home/me/.claude/projects/-home-me-projects--worktrees-devbox-fix")
        );
    }

    #[test]
    fn claude_usage_counts_each_message_once_and_skips_unknown_lines() {
        let mut totals = TokenTotals::default();
        let mut seen = HashSet::new();
        claude_totals(Cursor::new(CLAUDE), &mut totals, &mut seen);
        assert_eq!((totals.input, totals.output, totals.cache_write, totals.cache_read), (10, 30, 100, 200));
    }

    #[test]
    fn codex_usage_takes_the_last_cumulative_count_for_sessions_inside_the_root() {
        let inside = codex_session(Cursor::new(CODEX), Path::new("/home/me/projects/devbox-fix")).unwrap();
        assert_eq!((inside.input, inside.cache_read, inside.output, inside.sessions), (300, 120, 30, 1));
        assert!(codex_session(Cursor::new(CODEX), Path::new("/home/me/projects/devbox")).is_none());
    }

    #[test]
    fn usage_reads_both_tools_from_a_home_folder() {
        let home = tempfile::tempdir().unwrap();
        let root = Path::new("/home/me/projects/devbox-fix");
        let claude = claude_project_dir(home.path(), root.to_str().unwrap());
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("a.jsonl"), CLAUDE).unwrap();
        let day = home.path().join(".codex/sessions/2026/10/01");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(day.join("rollout-1.jsonl"), CODEX).unwrap();
        let since = 1_790_812_800_000; // 2026-10-01T00:00:00Z
        let report = usage(home.path(), root, since, since + 3_600_000);
        assert_eq!(report.claude.unwrap().output, 30);
        assert_eq!(report.codex.unwrap().output, 30);
        assert!(!report.truncated);
    }
}
```

- [ ] **Step 3: 실패 확인** — Run: `cargo test -p workspace-wsl --lib agent_usage` → FAIL
- [ ] **Step 4: 구현**
  - 한도: 파일 256개, 읽기 합계 128MiB, 한 줄 4MiB(넘는 줄은 건너뜀). 넘으면 멈추고 `truncated: true`.
  - `claude_project_dir`: `root`의 바이트 중 ASCII 영숫자가 아닌 것을 모두 `-`로 바꾼 이름을 `home/.claude/projects/` 아래에 붙인다. `claude_totals`: `type == "assistant"`이고 `message.id`가 문자열이며 `message.usage`가 객체인 줄만, `seen`에 없는 `message.id`만 더한다(`input_tokens`→input, `output_tokens`→output, `cache_creation_input_tokens`→cache_write, `cache_read_input_tokens`→cache_read; 없는 필드는 0). 파일마다 `sessions += 1`(더한 줄이 있을 때).
  - `codex_session`: 첫 `session_meta`의 `payload.cwd`가 `root`와 같거나 그 아래일 때만, 마지막 `event_msg`·`payload.type == "token_count"`의 `payload.info.total_token_usage`(`input_tokens`, `cached_input_tokens`→cache_read, `output_tokens`)를 돌려준다(`sessions: 1`).
  - `usage`: Claude 폴더의 `*.jsonl`(수정 시각이 `since_ms` 이후인 것) 전부, Codex는 `home/.codex/sessions/YYYY/MM/DD`를 `since_ms`의 UTC 날짜부터 `now_ms`의 날짜까지(최대 31일) 훑는다. 날짜 계산은 P0-07 `operation_log`의 Hinnant 함수와 같은 방식으로 `agent_usage.rs` 안에 둔다(`civil_from_days`). 폴더가 없으면 해당 도구는 `None`.
  - `engine.rs`: `"agent_usage"` 요청(인자 `{context, sinceMs}`, `agent_resources`와 같은 context 검사) → `usage(HOME, root, sinceMs, now)`. `HOME`이 없으면 `agent_usage_unavailable`. `control.rs` `project_method`에 `"agent_usage"`.
- [ ] **Step 5: 확인·커밋** — Run: `cargo test -p workspace-wsl --lib agent_usage` → PASS. `git add -A && git commit -m "feat(devbox-workspace): read Claude Code and Codex token usage in the WSL helper"`

---

### Task B3: host 메서드와 CPU 계산

**Files:** Create `apps/devbox-workspace/src-tauri/src/agent_hub/resources.rs`; Modify `ipc/agents.rs`, `agent_hub/mod.rs`, `tests/typescript.rs`, 허용 표 fixture

**Interfaces (Produces):** `AgentsCall::Resources {}` → `Vec<AgentResources { task_id, processes, rss_bytes, cpu_percent: Option<u32>, truncated }>`, `AgentsCall::Usage { task_id: String }` → `UsageReport`; `resources::CpuTracker::percent(&mut self, task_id: &str, cpu_ticks: u64, clock_ticks: u64, at_ms: u64) -> Option<u32>`; issue `agent_resources_unavailable`, `agent_usage_unavailable`

- [ ] **Step 1: 실패하는 테스트** — `resources.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_percent_needs_two_samples_and_ignores_counter_resets() {
        let mut cpu = CpuTracker::default();
        assert_eq!(cpu.percent("t1", 1_000, 100, 10_000), None);
        // 150 ticks over 1s at 100 Hz = 1.5 cores
        assert_eq!(cpu.percent("t1", 1_150, 100, 11_000), Some(150));
        assert_eq!(cpu.percent("t1", 10, 100, 12_000), None); // processes restarted
        assert_eq!(cpu.percent("t1", 60, 100, 12_000), None); // no elapsed time
    }

    #[test]
    fn finished_tasks_are_forgotten() {
        let mut cpu = CpuTracker::default();
        cpu.percent("t1", 1, 100, 1);
        cpu.retain(&["t2".to_string()]);
        assert_eq!(cpu.percent("t1", 101, 100, 1_001), None);
    }
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-workspace --lib agent_hub::resources` → FAIL
- [ ] **Step 3: 구현**
  - `CpuTracker { previous: HashMap<String, (u64, u64)> }`: `percent`는 이전 값이 있고 tick과 시각이 모두 늘었을 때만 `delta_ticks * 100_000 / (clock_ticks * delta_ms)`(정수, 한 코어 = 100). `retain(ids)`로 목록에 없는 작업을 지운다. Workspace `Runtime`에 `Mutex<CpuTracker>` 하나.
  - `Resources`: 현재 프로젝트의 `running` 작업(최대 8개)마다 Registry에서 작업 worktree context를 만들고 `owner.admit_wsl(host.helper_directory()?, &context)?.file_request_until(&context, "agent_resources", json!({}), deadline)`. 한 작업이 실패하면 그 작업만 `processes: 0, cpu_percent: None, truncated: true`로 넣고 계속한다. lane `Probes`, 예산 `LONG_BUDGET_MS`.
  - `Usage`: 작업을 찾아 같은 방법으로 `agent_usage {sinceMs: task.created_at_ms}`. lane `Probes`, 예산 `LONG_BUDGET_MS`.
  - 생성 TS와 허용 표 fixture를 갱신한다(두 행 추가만).
- [ ] **Step 4: 확인·커밋** — Run: `cargo test -p devbox-workspace --lib -- agent_hub ipc::agents && cargo test -p devbox-workspace --test typescript && bash .github/scripts/check-generated-bindings.sh` → PASS. `git add -A && git commit -m "feat(devbox-workspace): agent resource and usage commands"`

---

### Task B4: 화면 표시

**Files:** Modify `apps/devbox-workspace/src/agents/{AgentHub.tsx,AgentTaskRow.tsx,AgentHub.test.tsx,messages.ts}`; Create `apps/devbox-workspace/src/agents/format.ts`, `format.test.ts`

- [ ] **Step 1: 실패하는 테스트**
  - `format.test.ts`: `formatBytes(384 * 4096)` → `"1.5 MB"`, `formatBytes(3 * 1024 ** 3)` → `"3.0 GB"`, `formatCpu(null)` → `"측정 중"`, `formatCpu(150)` → `"150%"`, `formatTokens(1_234_567)` → `"123만"`, `formatTokens(999)` → `"999"`.
  - `AgentHub.test.tsx`: 실행 중 작업이 있으면 `resources`를 10초마다 부르고(가짜 타이머), 화면이 숨겨지면(`active=false`) 부르지 않는다. 행에 "CPU 150% · 메모리 1.5 MB · 프로세스 2개"가 보인다. "토큰 사용량 보기"를 누르면 `usage`를 한 번 부르고 "Claude Code 입력 10 · 출력 30 · 캐시 300", "Codex 입력 300 · 출력 30 · 캐시 120"을 보인다. `truncated`면 "일부 기록만 읽었습니다."를 덧붙인다.
- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter devbox-workspace exec vitest run src/agents` → FAIL
- [ ] **Step 3: 구현** — `format.ts`(1,000 단위가 아니라 1,024 단위 바이트, 토큰은 1만 이상이면 "만" 단위 반올림). `AgentHub`에서 `usePolling(loadResources, { intervalMs: 10_000, active: active && hasRunning })`. 사용량은 버튼을 누를 때만 읽는다(자동 갱신 없음).
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS. `git add -A && git commit -m "feat(devbox-workspace): show agent resources and token usage"`

---

### Task B5: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): Claude Code 작업이 실행 중일 때 CPU·메모리 값이 10초마다 바뀌고, 터미널 종료 후 0으로 돌아간다. "토큰 사용량 보기"의 Claude Code 값이 같은 기간 `~/.claude/projects/<폴더>`의 기록과 크게 다르지 않다.
