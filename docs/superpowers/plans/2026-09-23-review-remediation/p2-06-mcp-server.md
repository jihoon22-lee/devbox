# P2-06 Devbox MCP 서버 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4와 ADR 0015를 읽는다.

**Goal:** WSL의 Claude Code·Codex가 Devbox 데이터를 도구로 쓰게 하는 stdio MCP 서버를 만든다(D25 2순위). 읽기 도구(프로젝트·작업·실행 기록·실행 로그·검색·노트 읽기)는 켜면 바로 쓰고, 쓰기 도구(노트 기록·신뢰한 작업 실행)는 Control Center에서 따로 허용한다.

**Architecture:**
- 실행 파일: `devbox-agent.exe`의 사본 `<설치 루트>\bin\devbox-mcp.exe`를 `--mcp-stdio`로 실행한다. generation 경로는 업데이트마다 바뀌므로, agent가 시작할 때 자기 실행 파일을 이 고정 경로로 복사해 둔다(실행 중인 사본은 이름을 바꿔 치우고 새로 복사). WSL에서는 interop으로 이 `.exe`를 바로 실행한다.
- `--mcp-stdio` 모드는 Tauri 앱을 만들기 전에 갈라져 single-instance·트레이·pipe 서버를 띄우지 않는다. 표준 입출력에서 줄 단위 JSON-RPC를 처리하고, 도구 호출은 실행 중인 agent에 pipe로 보낸다(`agent-client`, peer 제품 id `mcp`). agent가 없으면 기존 규칙대로 띄운다.
- 프로토콜 처리는 새 crate `mcp-server`(Tauri·네트워크·파일 없음, 동기 `ToolHost` trait)에 둔다. 2025-11-25 방식(`initialize` 협상)과 2026-07-28 방식(요청마다 `_meta`의 버전, `server/discover`)을 모두 받는다 — API Studio의 MCP 클라이언트(`crates/api-protocols/src/core/mcp.rs`)가 검증하는 결과 모양(`resultType`, `ttlMs`, `cacheScope`)을 그대로 만족한다.
- agent 쪽 component `agent.mcp`(peer `mcp`만)가 도구를 실제 데이터로 바꾼다: Registry 읽기 전용(P2-02), runtime engine(P2-02에서 agent 소유), content index 검색(P2-04에서 agent 소유), vault 읽기와 `Inbox/` 새 파일 쓰기.
- 설정 `mcp-settings.json`(agent 데이터 폴더): `enabled`(기본 꺼짐), `allowNoteCapture`(기본 꺼짐), `allowTaskRun`(기본 꺼짐). Control Center 설정 화면에 한 절을 두고 Claude Code·Codex 등록 명령을 복사할 수 있게 보여 준다.
- 보안 범위(ADR 0016): pipe는 같은 사용자만 열 수 있고 peer 이미지 경로를 확인한다. 작업 실행은 runtime의 기존 신뢰 검사(`trusted && shell_trusted && available`)를 그대로 따른다. 따로 승인 대화상자를 두지 않는다.

**Tech Stack:** Rust(serde_json, tokio current-thread, rusqlite 읽기 전용), React 19, Vitest

**Spec:** `review.md` §8 신규 기능 표 2번 · `00-roadmap.md` D25 · ADR 0015 · ADR 0016

## Global Constraints

- `00-roadmap.md` §3 전부 적용. 새 의존성 없음(`mcp-server`는 `serde`, `serde_json`만).
- stdio 규칙: 한 줄에 JSON 하나, stdout에는 프로토콜 메시지만 쓰고 매번 flush, 진단은 stderr에 한 줄(치명적 오류만). 한 줄 최대 4MiB(넘으면 오류 응답 후 계속).
- 도구 설명·스키마 문자열은 영어(모델이 읽는 API 문서), Control Center 화면 문구는 한국어.
- 도구 결과는 한도 안으로 자른다: 목록 최대 50개, 로그 최대 64KiB, 노트 읽기 최대 256KiB, 검색 최대 20개.
- 쓰기 도구는 설정이 꺼져 있으면 `tools/list`에 나오지 않고, 호출하면 도구 오류(`isError: true`)로 답한다. 설정 변경은 다음 호출부터 적용한다(서버 재시작 불필요).
- 노트 기록은 기존 파일을 바꾸지 않는다. vault의 `Inbox/` 아래 새 파일만 만든다(같은 이름이 있으면 ` (2)`…를 붙인다).

## Review Focus

1. MCP가 꺼져 있을 때 클라이언트가 붙으면 `initialize`/`server/discover`가 "Control Center에서 켜 주세요" 오류로 끝나 클라이언트 화면에 이유가 보인다. (Task 1 테스트)
2. 2025-11-25 이전 버전(`2025-06-18`, `2025-03-26`, `2024-11-05`)을 요청하는 클라이언트는 그 버전으로, 모르는 버전은 `2025-11-25`로 협상된다. 2026-07-28 요청은 `initialize` 없이 처리되고 결과에 `resultType: "complete"`가 붙는다. (Task 1)
3. `devbox_note_read`는 vault 밖(`..`, 절대 경로, symlink)과 `.md`가 아닌 파일을 거부한다. (Task 4)
4. `devbox_task_run`은 신뢰하지 않았거나 사용할 수 없는 작업을 거부한다(runtime에 보내기 전). (Task 4)
5. 업데이트 뒤에도 등록한 명령이 그대로 동작한다: agent가 새 버전으로 시작하면 `bin\devbox-mcp.exe`를 갈아 끼우고, 실행 중인 옛 사본은 이름만 바뀌어 계속 돈다. (Task 3 테스트. 이번 계획의 릴리스는 v0.9.0 하나라 실제 업데이트로는 다음 버전 때 확인한다)

## Branch · PR

- 묶음: **B10** — 브랜치 `feat/suite/agent-hub-and-mcp`, PR 제목 `feat(suite): agent hub and Devbox MCP server`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(suite): expose Devbox tools to coding agents over MCP`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: `mcp-server` crate — 프로토콜

**Files:** Create `crates/mcp-server/{Cargo.toml,src/lib.rs,src/protocol.rs,src/tools.rs}`; Modify 루트 `Cargo.toml`(`members`, `[workspace.dependencies]`에 `mcp-server = { path = "crates/mcp-server" }`)

**Interfaces (Produces):**
- `pub trait ToolHost { fn settings(&mut self) -> Result<McpSettings, HostError>; fn call(&mut self, tool: &ToolCall) -> Result<serde_json::Value, HostError>; }`
- `pub struct McpSettings { pub enabled: bool, pub allow_note_capture: bool, pub allow_task_run: bool }`(camelCase, `Default` = 모두 false)
- `pub struct HostError { pub code: String, pub message: String }`
- `pub struct Server<H: ToolHost> { .. }`, `Server::new(host: H, server_version: &str) -> Self`, `Server::handle_line(&mut self, line: &str) -> Option<String>`(알림이면 `None`)
- 상수: `MODERN_VERSION = "2026-07-28"`, `LEGACY_VERSIONS = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"]`, `MAX_LINE_BYTES = 4 * 1024 * 1024`, 오류 코드 `PARSE_ERROR = -32700`, `INVALID_REQUEST = -32600`, `METHOD_NOT_FOUND = -32601`, `INVALID_PARAMS = -32602`, `SERVER_DISABLED = -32001`

- [ ] **Step 1: crate 뼈대** — `Cargo.toml`:

```toml
[package]
name = "mcp-server"
version = "0.1.0"
edition.workspace = true
publish = false

[dependencies]
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
```

- [ ] **Step 2: 실패하는 테스트** — `protocol.rs` 테스트 모듈

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    struct FakeHost { settings: McpSettings, calls: Vec<String> }
    impl ToolHost for FakeHost {
        fn settings(&mut self) -> Result<McpSettings, HostError> { Ok(self.settings.clone()) }
        fn call(&mut self, tool: &ToolCall) -> Result<Value, HostError> {
            self.calls.push(tool.name().to_string());
            match tool {
                ToolCall::Projects => Ok(json!({"projects": [{"id": "p1", "name": "devbox"}]})),
                ToolCall::NoteRead { path } if path == "missing.md" => Err(HostError { code: "note_missing".into(), message: "Note not found".into() }),
                _ => Ok(json!({"ok": true})),
            }
        }
    }
    fn server(enabled: bool) -> Server<FakeHost> {
        Server::new(FakeHost { settings: McpSettings { enabled, ..McpSettings::default() }, calls: vec![] }, "0.9.0")
    }
    fn reply(server: &mut Server<FakeHost>, message: Value) -> Value {
        serde_json::from_str(&server.handle_line(&message.to_string()).expect("a response")).unwrap()
    }

    #[test]
    fn legacy_initialize_negotiates_a_supported_version() {
        let mut s = server(true);
        let r = reply(&mut s, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"claude-code","version":"2"}}}));
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(r["result"]["serverInfo"]["name"], "devbox");
        assert!(r["result"]["capabilities"]["tools"].is_object());
        let r = reply(&mut s, json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2023-01-01","capabilities":{}}}));
        assert_eq!(r["result"]["protocolVersion"], "2025-11-25");
        assert!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
    }

    #[test]
    fn a_disabled_server_explains_itself_at_the_handshake() {
        let mut s = server(false);
        let r = reply(&mut s, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{}}}));
        assert_eq!(r["error"]["code"], SERVER_DISABLED);
        assert!(r["error"]["message"].as_str().unwrap().contains("Control Center"));
        let r = reply(&mut s, json!({"jsonrpc":"2.0","id":2,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion": MODERN_VERSION}}}));
        assert_eq!(r["error"]["code"], SERVER_DISABLED);
    }

    #[test]
    fn modern_requests_carry_their_version_and_get_complete_results() {
        let mut s = server(true);
        let meta = json!({"io.modelcontextprotocol/protocolVersion": MODERN_VERSION});
        let r = reply(&mut s, json!({"jsonrpc":"2.0","id":"d","method":"server/discover","params":{"_meta": meta}}));
        assert_eq!(r["result"]["resultType"], "complete");
        assert_eq!(r["result"]["supportedVersions"], json!([MODERN_VERSION, "2025-11-25"]));
        assert_eq!(r["result"]["cacheScope"], "private");
        assert_eq!(r["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"], "devbox");
        let r = reply(&mut s, json!({"jsonrpc":"2.0","id":"l","method":"tools/list","params":{"_meta": meta}}));
        assert_eq!(r["result"]["resultType"], "complete");
        assert_eq!(r["result"]["ttlMs"], 0);
        assert!(r["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "devbox_projects"));
    }

    #[test]
    fn write_tools_are_listed_only_when_allowed() {
        let mut s = server(true);
        let names = |r: &Value| r["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        let r = reply(&mut s, json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}));
        assert!(!names(&r).contains(&"devbox_note_capture".to_string()));
        assert!(!names(&r).contains(&"devbox_task_run".to_string()));
        s.host_mut().settings.allow_task_run = true;
        let r = reply(&mut s, json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}));
        assert!(names(&r).contains(&"devbox_task_run".to_string()));
    }

    #[test]
    fn tool_results_and_tool_failures_use_content_blocks() {
        let mut s = server(true);
        let r = reply(&mut s, json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"devbox_projects","arguments":{}}}));
        assert_eq!(r["result"]["isError"], false);
        assert_eq!(r["result"]["structuredContent"]["projects"][0]["name"], "devbox");
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("devbox"));
        let r = reply(&mut s, json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"devbox_note_read","arguments":{"path":"missing.md"}}}));
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("Note not found"));
        let r = reply(&mut s, json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"devbox_task_run","arguments":{"jobId":"j1"}}}));
        assert_eq!(r["result"]["isError"], true, "a disallowed write tool is a tool error, not a call");
        assert!(!s.host_mut().calls.contains(&"devbox_task_run".to_string()));
    }

    #[test]
    fn protocol_errors_keep_the_session_alive() {
        let mut s = server(true);
        let r: Value = serde_json::from_str(&s.handle_line("{not json").unwrap()).unwrap();
        assert_eq!((r["error"]["code"].as_i64(), r["id"].is_null()), (Some(PARSE_ERROR), true));
        let r = reply(&mut s, json!([{"jsonrpc":"2.0","id":1,"method":"ping"}]));
        assert_eq!(r["error"]["code"], INVALID_REQUEST);
        let r = reply(&mut s, json!({"jsonrpc":"2.0","id":2,"method":"resources/list","params":{}}));
        assert_eq!(r["error"]["code"], METHOD_NOT_FOUND);
        let r = reply(&mut s, json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"nope","arguments":{}}}));
        assert_eq!(r["error"]["code"], INVALID_PARAMS);
        let r = reply(&mut s, json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"devbox_run_log","arguments":{"runId":5}}}));
        assert_eq!(r["error"]["code"], INVALID_PARAMS);
        let long = format!("{{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"ping\",\"pad\":\"{}\"}}", "x".repeat(MAX_LINE_BYTES));
        let r: Value = serde_json::from_str(&s.handle_line(&long).unwrap()).unwrap();
        assert_eq!(r["error"]["code"], INVALID_REQUEST);
        assert_eq!(reply(&mut s, json!({"jsonrpc":"2.0","id":6,"method":"ping"}))["result"], json!({}));
    }
}
```

- [ ] **Step 3: 실패 확인** — Run: `source ~/.cargo/env && cargo test -p mcp-server` → FAIL
- [ ] **Step 4: 구현** — `protocol.rs`:
  - `handle_line`: 길이 > `MAX_LINE_BYTES` → `INVALID_REQUEST`(id null). JSON 파싱 실패 → `PARSE_ERROR`(id null). 배열 → `INVALID_REQUEST`. 객체가 아니거나 `jsonrpc != "2.0"`이거나 `method`가 문자열이 아니면 `INVALID_REQUEST`. `id`가 없으면 알림: 처리 후 `None`(`notifications/initialized`, `notifications/cancelled` 포함 모두 무시).
  - era 판정: `params._meta["io.modelcontextprotocol/protocolVersion"] == MODERN_VERSION`이면 modern(결과마다 `"resultType": "complete"`, 목록 결과에는 `"ttlMs": 0, "cacheScope": "private"`도), 아니면 legacy.
  - `initialize`: `settings()?.enabled`가 아니면 `SERVER_DISABLED`("Devbox MCP server is turned off. Turn it on in Devbox Control Center > Settings > MCP."). 요청 버전이 `LEGACY_VERSIONS`에 있으면 그 값, 아니면 `"2025-11-25"`. 결과 `{protocolVersion, capabilities: {tools: {listChanged: false}}, serverInfo: {name: "devbox", version}, instructions}`(`instructions`: "Devbox exposes the user's registered projects, task runs, logs and notes. Prefer devbox_search before reading notes.").
  - `server/discover`: 비활성이면 `SERVER_DISABLED`. 결과 `{resultType: "complete", supportedVersions: [MODERN_VERSION, "2025-11-25"], capabilities: {tools: {}}, instructions, ttlMs: 0, cacheScope: "private", _meta: {"io.modelcontextprotocol/serverInfo": {name: "devbox", version}}}`.
  - `ping` → `{}`. `tools/list` → `tools::catalog(&settings)`. `tools/call` → `ToolCall::parse(name, arguments)`(모르는 이름·잘못된 인자 → `INVALID_PARAMS`, 메시지에 필드 이름), 쓰기 도구가 허용되지 않았으면 호스트를 부르지 않고 도구 오류, 그 밖은 `host.call`. 성공 결과 `{content: [{type: "text", text: <pretty JSON>}], structuredContent: <value>, isError: false}`, `HostError`는 `{content: [{type: "text", text: message}], isError: true}`. 그 밖의 메서드 → `METHOD_NOT_FOUND`.
  - 비활성 상태에서 `tools/list`·`tools/call`이 오면(세션 중 끈 경우) `tools/list`는 빈 목록, `tools/call`은 도구 오류로 답한다.
  - `host_mut(&mut self) -> &mut H`(테스트·호출자용).
- [ ] **Step 5: 확인·커밋** — Run: `cargo test -p mcp-server` → PASS. `git add -A && git commit -m "feat(suite): add an MCP stdio protocol core"`

---

### Task 2: 도구 목록과 인자 검증

**Files:** `crates/mcp-server/src/tools.rs`

**Interfaces (Produces):** `pub enum ToolCall { Projects, Tasks { root: Option<String> }, Runs { job_id: Option<String>, limit: u32 }, RunLog { run_id: String, stream: LogStream, max_bytes: u32 }, Search { query: String, limit: u32 }, NoteRead { path: String }, NoteCapture { title: String, body: String }, TaskRun { job_id: String } }`, `pub enum LogStream { Stdout, Stderr }`, `ToolCall::parse(name: &str, arguments: &Value) -> Result<ToolCall, String>`, `ToolCall::name(&self) -> &'static str`, `ToolCall::is_write(&self) -> bool`, `pub fn catalog(settings: &McpSettings) -> Vec<Value>`

| 도구 | 인자(JSON Schema) | 기본값·한도 | 쓰기 |
|---|---|---|---|
| `devbox_projects` | `{}` | — | 아니오 |
| `devbox_tasks` | `{root?: string}` | root는 절대 경로 | 아니오 |
| `devbox_runs` | `{jobId?: string, limit?: integer 1–50}` | 20 | 아니오 |
| `devbox_run_log` | `{runId: string, stream?: "stdout"\|"stderr", maxBytes?: integer 1–65536}` | stdout, 16384 | 아니오 |
| `devbox_search` | `{query: string 1–256자, limit?: integer 1–20}` | 10 | 아니오 |
| `devbox_note_read` | `{path: string 1–1024자}` | vault 상대 경로 | 아니오 |
| `devbox_note_capture` | `{title: string 1–120자, body: string ≤ 65536바이트}` | — | 예(`allowNoteCapture`) |
| `devbox_task_run` | `{jobId: string}` | — | 예(`allowTaskRun`) |

- [ ] **Step 1: 실패하는 테스트**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn arguments_get_defaults_and_bounds() {
        assert_eq!(ToolCall::parse("devbox_runs", &json!({})).unwrap(), ToolCall::Runs { job_id: None, limit: 20 });
        assert!(ToolCall::parse("devbox_runs", &json!({"limit": 51})).is_err());
        assert_eq!(
            ToolCall::parse("devbox_run_log", &json!({"runId": "r1"})).unwrap(),
            ToolCall::RunLog { run_id: "r1".into(), stream: LogStream::Stdout, max_bytes: 16_384 }
        );
        assert!(ToolCall::parse("devbox_search", &json!({"query": ""})).is_err());
        assert!(ToolCall::parse("devbox_search", &json!({"query": "x", "extra": true})).is_err());
        assert!(ToolCall::parse("devbox_note_capture", &json!({"title": "t", "body": "x".repeat(65_537)})).is_err());
        assert!(ToolCall::parse("devbox_tasks", &json!({"root": "relative"})).is_err());
    }

    #[test]
    fn catalog_schemas_are_objects_and_hide_disallowed_writes() {
        let all = catalog(&McpSettings { enabled: true, allow_note_capture: true, allow_task_run: true });
        assert_eq!(all.len(), 8);
        assert!(all.iter().all(|tool| tool["inputSchema"]["type"] == "object" && tool["description"].as_str().is_some_and(|d| !d.is_empty())));
        let read_only = catalog(&McpSettings { enabled: true, ..McpSettings::default() });
        assert_eq!(read_only.len(), 6);
        assert!(ToolCall::parse("devbox_task_run", &json!({"jobId": "j"})).unwrap().is_write());
    }
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p mcp-server tools` → FAIL
- [ ] **Step 3: 구현** — 인자 객체는 알려진 키만 허용한다. 각 도구의 `inputSchema`는 위 표를 `{"type":"object","properties":{…},"required":[…],"additionalProperties":false}`로 쓴 값이고, `description`은 한 문장 영어(예: `devbox_run_log`: "Read the tail of a task run's stdout or stderr. Use devbox_runs to find run ids.", `devbox_task_run`: "Start a Devbox task that the user has already trusted in Workspace. Returns the run operation."). `catalog`는 설정에 따라 쓰기 도구를 뺀다.
- [ ] **Step 4: 확인·커밋** — Run: `cargo test -p mcp-server` → PASS. `git commit -am "feat(suite): define Devbox MCP tools"`

---

### Task 3: 고정 실행 경로와 `--mcp-stdio` 진입점

**Files:** Create `apps/devbox-agent/src/mcp/{mod.rs,launcher.rs,stdio.rs}`; Modify `apps/devbox-agent/src/{main.rs,lib.rs,server.rs}`, `crates/agent-client/src/lib.rs`, `crates/suite-runtime/src/lib.rs`(또는 peer 확인 모듈)

**Interfaces (Produces):**
- `launcher::refresh(install_root: &Path, current_exe: &Path) -> std::io::Result<PathBuf>`(고정 경로 `bin/devbox-mcp.exe`), `launcher::stable_path(install_root: &Path) -> PathBuf`
- `agent_client::AgentClient::for_install_root(root: &Path, product: &str) -> Result<AgentClient, AgentError>`
- `suite_runtime::current_agent_path(root: &Path) -> Result<PathBuf, &'static str>`
- agent peer 규칙: 클라이언트 이미지가 `launcher::stable_path(root)`와 같으면 peer 제품 id `mcp`
- `stdio::run(host: impl ToolHost, input: impl BufRead, output: impl Write) -> std::io::Result<()>`

- [ ] **Step 1: 실패하는 테스트**
  - `launcher.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_copies_once_and_replaces_a_changed_copy() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("agent-new.exe");
        std::fs::write(&exe, b"agent v2").unwrap();
        let stable = refresh(root.path(), &exe).unwrap();
        assert_eq!(stable, root.path().join("bin").join("devbox-mcp.exe"));
        assert_eq!(std::fs::read(&stable).unwrap(), b"agent v2");
        let before = std::fs::metadata(&stable).unwrap().modified().unwrap();
        refresh(root.path(), &exe).unwrap();
        assert_eq!(std::fs::metadata(&stable).unwrap().modified().unwrap(), before, "same content is not rewritten");
        std::fs::write(&exe, b"agent v3").unwrap();
        refresh(root.path(), &exe).unwrap();
        assert_eq!(std::fs::read(&stable).unwrap(), b"agent v3");
        let leftovers: Vec<_> = std::fs::read_dir(root.path().join("bin")).unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name != "devbox-mcp.exe").collect();
        assert!(leftovers.is_empty(), "old copies are removed when they are not in use: {leftovers:?}");
    }
}
```

  - `stdio.rs`: 입력 두 줄(`initialize`, `tools/list`)과 알림 한 줄을 넣으면 출력이 정확히 두 줄이고 각 줄이 JSON이다(`FakeHost`는 Task 1과 같은 모양으로 이 파일 테스트에 둔다).
  - `server.rs`(P2-01 파일) 테스트: `peer_product_for_image(Path::new("C:/suite/bin/devbox-mcp.exe"), Path::new("C:/suite"))`는 `Some("mcp")`, 같은 루트의 다른 경로는 기존 규칙(제품 실행 파일이면 그 제품, 아니면 `None`).
- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-agent --lib mcp && cargo test -p devbox-agent --lib peer_product` → FAIL
- [ ] **Step 3: 구현**
  - `launcher::refresh`: 대상이 있고 길이와 SHA-256이 현재 실행 파일과 같으면 그대로 둔다. 아니면 `bin/`을 만들고 `bin/.devbox-mcp.<uuid>.tmp`로 복사한 뒤, 대상이 있으면 `bin/.devbox-mcp.old-<uuid>.exe`로 이름을 바꾸고(Windows에서 실행 중인 파일도 이름은 바꿀 수 있다) tmp를 대상 이름으로 바꾼다. 마지막에 `.devbox-mcp.old-*.exe`를 지워 보고, 지우지 못한 것(실행 중)은 다음 시작 때 다시 지운다.
  - agent `lib.rs` setup: 설치형이면(`portable`이 아니면) `launcher::refresh(install_root, current_exe)`를 background로 실행하고 실패는 운영 로그(P0-07)에 `mcp_launcher_refresh_failed`로 남긴다.
  - `main.rs`: `std::env::args()`에 `--mcp-stdio`가 있으면 Tauri를 만들지 않고 `mcp::run_stdio()`를 부른 뒤 그 종료 코드로 끝낸다. `run_stdio`: 실행 파일이 `<root>\bin\devbox-mcp.exe`인지 확인해 root를 얻고(아니면 stderr 한 줄 후 코드 2), tokio current-thread runtime을 만든 뒤 `AgentClient::for_install_root(root, "mcp")`로 `AgentToolHost`(Task 4)를 만들어 `stdio::run(host, stdin.lock(), stdout.lock())`.
  - `stdio::run`: `read_line`으로 한 줄씩 읽어(빈 줄 무시) `server.handle_line`의 결과를 `writeln!` + `flush`. EOF면 정상 종료.
  - `suite_runtime::current_agent_path(root)`: 설치 manifest(`devbox-installation.json`)가 가리키는 현재 generation의 `products/control-center/resources/suite/devbox-agent.exe`(제품이 자기 generation을 찾을 때 쓰는 manifest 읽기 함수를 재사용한다).
  - `AgentClient::for_install_root`: 파이프 이름은 `current_agent_path`로 찾은 agent 경로에 P2-01의 `component_namespace(agent_exe, "control-center", version)`를 적용해 얻고, 실행은 같은 경로를 `spawn`한다. 나머지(재시도 표, 연결 유지)는 기존 `Transport` 구현을 쓴다.
  - agent `server.rs`: peer 이미지가 `<root>\bin\devbox-mcp.exe`이면 제품 id `mcp`. `check_hello`는 기존대로 `Hello.product == "mcp"`를 요구한다.
- [ ] **Step 4: 확인·커밋** — Run: `cargo test -p devbox-agent --lib && cargo test -p agent-client && cargo test -p suite-runtime` → PASS. `git add -A && git commit -m "feat(suite): run the MCP server from a stable devbox-mcp.exe"`

---

### Task 4: agent의 `agent.mcp` component

**Files:** Create `apps/devbox-agent/src/mcp/{route.rs,host.rs,notes.rs,settings.rs}`; Modify `apps/devbox-agent/src/routes.rs`, `crates/knowledge-stores/src/lib.rs`

**Interfaces (Produces):**
- agent 라우트 `agent.mcp`(peer `mcp`만), 메서드 `settings`, `projects`, `tasks {root?}`, `runs {jobId?, limit}`, `run_log {runId, stream, maxBytes}`, `search {query, limit}`, `note_read {path}`, `note_capture {title, body}`, `task_run {jobId}`
- `settings::load(dir: &Path) -> McpSettings`(없거나 깨지면 기본값), `settings::save(dir: &Path, value: &McpSettings) -> io::Result<()>`; agent 라우트 `agent.settings`에 `mcp_settings {}`·`set_mcp_settings {settings}`(`set`은 peer `control-center`만)
- `notes::read(vault: &Path, relative: &str) -> Result<NoteText, HostError>`(`{path, text, truncated}`), `notes::capture(vault: &Path, title: &str, body: &str, now_ms: u64) -> Result<String /*vault 상대 경로*/, HostError>`
- `knowledge_stores::vault_root(knowledge_root: &Path) -> Result<Option<PathBuf>, String>`
- `host::AgentToolHost { client: AgentClient, runtime: tokio::runtime::Runtime }`(`ToolHost` 구현: 각 도구를 `client.call("agent.mcp", {method, args})`로)

- [ ] **Step 1: 실패하는 테스트**
  - `notes.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_is_limited_to_markdown_inside_the_vault() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::create_dir(vault.path().join("Projects")).unwrap();
        std::fs::write(vault.path().join("Projects/devbox.md"), "# Devbox\n").unwrap();
        std::fs::write(vault.path().join("secret.txt"), "x").unwrap();
        assert_eq!(read(vault.path(), "Projects/devbox.md").unwrap().text, "# Devbox\n");
        for bad in ["../outside.md", "/etc/passwd", "secret.txt", "Projects/../../x.md", ""] {
            assert!(read(vault.path(), bad).is_err(), "{bad}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/hostname", vault.path().join("link.md")).unwrap();
            assert!(read(vault.path(), "link.md").is_err());
        }
    }

    #[test]
    fn large_notes_are_truncated_at_a_char_boundary() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::write(vault.path().join("big.md"), "가".repeat(100_000)).unwrap();
        let note = read(vault.path(), "big.md").unwrap();
        assert!(note.truncated && note.text.len() <= 256 * 1024);
    }

    #[test]
    fn capture_creates_new_inbox_files_and_never_overwrites() {
        let vault = tempfile::tempdir().unwrap();
        let at = 1_790_812_800_000; // 2026-10-01T00:00:00Z
        let first = capture(vault.path(), "Build: failing?", "body", at).unwrap();
        assert_eq!(first, "Inbox/2026-10-01 0000 Build- failing-.md");
        let second = capture(vault.path(), "Build: failing?", "other", at).unwrap();
        assert_eq!(second, "Inbox/2026-10-01 0000 Build- failing- (2).md");
        assert_eq!(std::fs::read_to_string(vault.path().join(&first)).unwrap(), "# Build: failing?\n\nbody\n");
    }
}
```

  - `settings.rs`: 없는 파일 → 기본값(모두 false), 저장 후 다시 읽으면 같은 값, 깨진 파일 → 기본값.
  - `route.rs`: `tasks`는 `source_root`가 요청 root와 같거나 그 아래(구성요소 단위)인 작업만 돌려준다 — 순수 함수 `filter_tasks(states: Vec<WorkspaceTaskState>, root: Option<&str>) -> Vec<TaskView>` 테스트. `task_run`은 `trusted && shell_trusted && available`이 아니면 `task_not_trusted`로 거부한다 — 순수 함수 `runnable(state: &WorkspaceTaskState) -> Result<(), HostError>` 테스트.
  - `knowledge_stores::vault_root`: 테스트용 notes 저장소 폴더에 `data.db`(`CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT)`, `root` 행)를 만들면 그 경로, 행이 없으면 `None`.
- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-agent --lib mcp && cargo test -p knowledge-stores vault_root` → FAIL
- [ ] **Step 3: 구현**
  - `notes::read`: `relative`가 비었거나 절대 경로이거나 `..` 구성요소가 있거나 확장자가 `.md`가 아니면 `note_path_invalid`. vault 기준으로 한 구성요소씩 `symlink_metadata`로 확인해 링크가 있으면 `note_path_invalid`. 256KiB까지 읽고 넘으면 char 경계에서 잘라 `truncated: true`. 없으면 `note_missing`.
  - `notes::capture`: 파일 이름 = `"{YYYY-MM-DD HHmm} {title}"`(UTC, P0-07의 날짜 계산 함수 재사용)에서 `\/:*?"<>|`와 제어 문자를 `-`로 바꾸고 앞뒤 공백·마침표를 없앤 뒤 80자로 자름. `Inbox/`가 없으면 만든다. `OpenOptions::new().write(true).create_new(true)`로 만들고 `AlreadyExists`면 ` (2)`부터 ` (99)`까지 붙인다. 내용 `"# {title}\n\n{body}\n"`.
  - `route.rs` 메서드 → 내부 호출:
    - `settings` → `settings::load(agent_data_dir)`.
    - `projects` → P2-02의 Registry 읽기 전용 snapshot을 `{projects: [{id, name, worktrees: [{id, root, target: "windows"|"wsl"}]}]}`(최대 50개)로.
    - `tasks` → runtime engine `list_workspace_tasks` 결과를 `filter_tasks`로 거른 `[{jobId, label, kind, sourceRoot, trusted, available}]`(최대 50개).
    - `runs` → runtime engine `list_run_history`(`{filter: {jobId, limit}}`) → `[{runId, jobId, status, startedAt, endedAt, exitCode}]`.
    - `run_log` → runtime engine `tail_log`(`{input: {runId, stream, cursor: null, maxBytes}}`) → `{text, truncated}`.
    - `search` → content index engine `search_content`(`{query, limit}`); agent가 content index를 아직 초기화하지 않았으면 `search_unavailable`("Open Devbox Knowledge once to set up search.").
    - `note_read`·`note_capture` → `knowledge_stores::vault_root(knowledge_data_dir)`가 `None`이면 `vault_unavailable`("Open Devbox Knowledge once to choose a notes folder."). `note_capture`는 `settings.allow_note_capture`가 꺼져 있으면 `tool_not_allowed`.
    - `task_run` → `settings.allow_task_run` 확인, `list_workspace_tasks`에서 작업을 찾아 `runnable`, runtime engine `run_workspace_task_operation {id: jobId, failFast: true}` → 결과 operation view.
    engine 호출은 P1-14의 typed API(`runtime_engine::api::dispatch`, `content_index_engine::api::dispatch_search`)를 agent의 AppHandle로 부른다.
  - `knowledge_stores::vault_root`: notes 저장소 폴더를 기존 manifest 함수로 찾고 `data.db`를 `rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY`로 열어 `SELECT value FROM settings WHERE key = 'root'`. 빈 값이면 `None`.
  - `host::AgentToolHost::call`: `ToolCall`을 `{method, args}`로 바꿔 `client.call("agent.mcp", …)`, `AgentError::Remote(v)`는 `HostError { code: v.issue, message: 영어 문장 표 }`, `Unavailable`은 "Devbox agent is not running and could not be started."
- [ ] **Step 4: 확인·커밋** — Run: `cargo test -p devbox-agent --lib && cargo test -p knowledge-stores` → PASS. `git add -A && git commit -m "feat(suite): answer MCP tools from devbox-agent"`

---

### Task 5: Control Center 설정

**Files:** Create `packages/control-center-features/src/settings/McpSettings.tsx`, `McpSettings.test.tsx`; Modify Control Center 설정 화면(P2-04에서 자동 시작 줄을 둔 곳), `apps/devbox-control-center/src-tauri/src/…`(agent 설정 전달이 P2-04에 있으면 그대로 사용)

- [ ] **Step 1: 실패하는 테스트** — `McpSettings.test.tsx`
  - 처음 열면 "MCP 서버 사용" 체크가 꺼져 있고 쓰기 권한 체크는 비활성이다.
  - 켜면 `set_mcp_settings {settings: {enabled: true, allowNoteCapture: false, allowTaskRun: false}}`를 보내고, 등록 안내가 보인다:
    - Windows 경로 `C:\Users\me\AppData\Local\Devbox\bin\devbox-mcp.exe`(agent `mcp_settings` 결과의 `launcherPath`)
    - Claude Code: `claude mcp add --scope user devbox -- "$(wslpath -u 'C:\Users\me\AppData\Local\Devbox\bin\devbox-mcp.exe')" --mcp-stdio`
    - Codex(`~/.codex/config.toml`): `[mcp_servers.devbox]` / `command = "/mnt/c/Users/me/AppData/Local/Devbox/bin/devbox-mcp.exe"` / `args = ["--mcp-stdio"]` 와 "WSL 자동 마운트 위치를 바꿨다면 `wslpath -u`로 변환한 경로를 쓰세요."
    - 각 블록 옆 "복사" 버튼(클립보드 mock 확인).
  - "노트 기록 허용"·"신뢰한 작업 실행 허용"을 켜면 각각 저장된다. 포터블이면 "설치형에서만 사용할 수 있습니다."만 보인다.
  - axe 위반 0.
- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter @devbox/control-center-features exec vitest run src/settings/McpSettings.test.tsx` → FAIL
- [ ] **Step 3: 구현** — agent `mcp_settings` 결과에 `launcherPath`(고정 경로, 포터블이면 `null`)를 넣는다(Task 4의 `settings` 메서드와 별도로 `agent.settings`에서). WSL 기본 경로는 `C:\a\b` → `/mnt/c/a/b` 규칙의 순수 함수 `defaultWslPath(windowsPath)`(테스트 포함)로 만든다. 설정 저장은 `useOperation().run`.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS. `git add -A && git commit -m "feat(devbox-control-center): MCP server settings"`

---

### Task 6: PR 완료

- [ ] `docs/windows-guide.md`에 "AI 도구 연결(MCP)" 절: 켜는 곳, Claude Code·Codex 등록, 도구 목록, 쓰기 권한의 의미, 끄는 방법.
- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): (1) 집 PC WSL에서 Claude Code에 등록 → `/mcp`에 devbox 연결됨, "내 devbox 프로젝트의 최근 실패한 작업 로그 보여 줘"에 `devbox_runs`·`devbox_run_log`가 쓰인다. (2) Codex에 등록 → `devbox_search`로 노트 검색. (3) MCP를 끄면 두 클라이언트 모두 "Control Center에서 켜 주세요" 오류. (4) 작업 실행 허용 후 신뢰한 작업이 실행되고, 신뢰하지 않은 작업은 거부. 이 항목들은 P3-01 후보 점검표로 모은다.
