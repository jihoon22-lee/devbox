# P2-01 devbox-agent 뼈대 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4와 ADR `docs/adr/0015-devbox-agent.md`를 읽는다.

**Goal:** ADR 0015의 사용자별 백그라운드 프로세스 `devbox-agent`를 만든다: 창 없는 Tauri 앱, 설치별 named pipe 서버(`agent-protocol`), 제품이 필요할 때 띄우고 연결하는 클라이언트, Control Center 패키지 포함, 업데이트 전 종료. 이 PR에서는 어떤 기능도 옮기지 않고 `agent.status` 하나로 끝까지(시작·연결·호출·종료·업데이트) 동작을 증명한다(D24).

**Architecture:**
- `apps/devbox-agent`(Cargo 패키지 `devbox-agent`, Tauri v2, `app.windows = []`, 단일 인스턴스). 데이터 폴더 identifier는 `com.devbox.v08.agent.i<설치 접미사>`이고, 접미사는 같은 generation의 Control Center 실행 파일로 계산한다(ADR 0007 — 네 제품과 같은 접미사).
- 서버: `\\.\pipe\devbox-agent-<접미사>`. 연결마다 `Hello`(프로토콜·제품·세션) → peer 이미지 확인(같은 설치 generation의 제품 실행 파일, `suite-runtime` `peer_identity` 재사용) → `Welcome` → `Call`/`Reply` 반복. component 라우팅 표에는 이 PR에서 `agent.status`(메서드 `status`: 버전·generation·가동 시간·보유 component 목록)만 있다.
- 클라이언트: `crates/agent-client`(제품 host가 씀). pipe가 없으면 agent 실행 파일을 띄우고 재시도(0.2·0.5·1·2·4초), 연결을 제품 수명 동안 유지, 끊기면 다시 연결. `call(component, request_json) -> Value`.
- 배포: Control Center 패키지에 `resources/suite/devbox-agent.exe`를 넣는다(Suite bootstrap과 같은 방식). 업데이터는 generation 교체 전에 agent에 `Shutdown`을 보내고 종료를 기다린다.

**Tech Stack:** Rust(Tauri v2, tokio named pipe), `agent-protocol`(P1-19), PowerShell·Python(패키징 스크립트)

**Spec:** ADR 0015 · `00-roadmap.md` D24 · `review.md` §8 C안

## Global Constraints

- `00-roadmap.md` §3 전부 적용. 보안 수준 ADR 0016.
- 공개 자산 7개 계약 유지(agent는 Control Center ZIP 안의 구성요소).
- agent는 창·WebView를 만들지 않는다. 트레이는 P2-04에서 추가한다(이 PR은 트레이 없음).
- portable·개발 빌드: 접미사가 제품마다 다르므로 agent를 쓰지 않는다(클라이언트가 "agent 사용 불가"를 돌려주고 제품은 기존처럼 자체 처리). P2-02 이후 옮기는 기능도 portable에서는 제품 안에서 돈다.

## Review Focus

1. 제품 두 개가 동시에 시작해 agent를 동시에 띄우려 함 → 단일 인스턴스로 하나만 남고 둘 다 연결된다. (Task 3)
2. agent가 비정상 종료 → 다음 호출에서 클라이언트가 다시 띄우고 연결한다(ADR 재시도 규칙). (Task 3)
3. 다른 설치(다른 접미사)의 제품이나 임의 프로세스가 pipe에 연결 → peer 확인에서 거절(`Rejected`). (Task 2)
4. 업데이트 적용 → agent가 먼저 종료되고 writer lease를 막지 않는다. 새 generation 제품이 새 agent를 띄운다. (Task 4, 후보 acceptance)
5. portable 빌드 → agent 없이 기존처럼 동작한다(클라이언트 `unsupported`). (Task 3)

## Branch · PR

- 묶음: **B9** — 브랜치 `feat/suite/devbox-agent`, PR 제목 `feat(suite): run runtime, webhooks and collectors in devbox-agent`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(suite): add the devbox-agent background process`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: agent 앱과 설치 접미사

**Files:** Create `apps/devbox-agent/{Cargo.toml,build.rs,tauri.conf.json,capabilities/default.json,icons/(Control Center 아이콘 복사),src/main.rs,src/lib.rs,src/identity.rs}`; Modify 루트 `Cargo.toml`, `crates/product-shell-tauri/src/{installation.rs,lib.rs}`, `.github/scripts/check-source-cutover.py`

**Interfaces (Produces):** `product_shell_tauri::component_namespace(component_exe: &Path, owner_product: &str, version: &str) -> Result<String, &'static str>`(구성요소 실행 파일의 owner 제품 실행 파일로 접미사 계산), `devbox_agent::identity::pipe_name(suffix: &str) -> String`(`\\.\pipe\devbox-agent-<suffix>`)

- [ ] **Step 1: 실패하는 테스트** — `crates/product-shell-tauri/src/installation.rs` 테스트 모듈(기존 `Fixture` 사용)

```rust
    #[test]
    fn a_suite_component_shares_its_owner_products_namespace() {
        let root = Fixture::new();
        let owner = root.generation("one"); // products/workspace/devbox-workspace.exe (fixture product)
        let component = owner.parent().unwrap().join("resources/suite/devbox-agent.exe");
        std::fs::create_dir_all(component.parent().unwrap()).unwrap();
        std::fs::write(&component, b"agent").unwrap();
        assert_eq!(
            component_namespace(&component, "workspace", "0.8.0").unwrap(),
            namespace(&owner, "workspace", "0.8.0").unwrap()
        );
        let stray = root.0.join("devbox-agent.exe");
        std::fs::write(&stray, b"agent").unwrap();
        assert!(component_namespace(&stray, "workspace", "0.8.0").is_err());
    }
```

  (fixture가 만드는 제품이 `workspace`이므로 테스트는 workspace를 owner로 쓴다. 실제 agent의 owner는 `control-center`.)

- [ ] **Step 2: 구현**
  - `installation.rs`: `pub(crate) fn component_namespace(component_exe, owner_product, version)` — `component_exe`가 `…/products/<owner>/resources/suite/<file>.exe` 모양인지 확인하고, owner 실행 파일 `…/products/<owner>/devbox-<owner>.exe`로 `namespace(...)`를 호출한다. 모양이 다르면 `Err("component_path_invalid")`. `lib.rs`에서 `pub fn component_namespace`로 연다.
  - `apps/devbox-agent`: `tauri.conf.json`은 `identifier: "com.devbox.v08.agent"`, `app.windows: []`, `bundle.active: false`. `main.rs`는 `devbox_agent::run()`. `lib.rs`의 `run()`은 `tauri::Builder::default().plugin(tauri_plugin_single_instance::init(|_, _, _| {}))`로 시작하고, setup에서 `component_namespace(current_exe, "control-center", version)`로 접미사를 구해 identifier를 `com.devbox.v08.agent.i<suffix>`로 바꾼 context를 쓴다(제품의 `isolate_installation`과 같은 순서: context를 만든 뒤 `run` 전에 identifier 교체). 접미사를 구하지 못하면(portable) 즉시 종료 코드 2로 끝낸다.
  - 버전: `apps/devbox-agent/Cargo.toml`·`tauri.conf.json`의 version은 네 제품의 현재 값과 같게 둔다(작성 시점 `0.8.1`; 이 PR에서 올리지 않는다 — 버전은 P3-01에서 한 번만 올린다). `check-product-foundation.py`에 agent의 Cargo·tauri.conf 버전이 제품 버전과 같은지 보는 단언을 추가한다.
  - `identity.rs`: `pipe_name(suffix)`.
  - `check-source-cutover.py`: `apps/`의 폴더 집합을 `products | {"devbox-agent"}`로 바꾸고, `devbox-agent`가 `apps/catalog.json`에 없어야 한다는 단언을 추가한다(제품이 아니라 구성요소).
- [ ] **Step 3: 확인·커밋** — Run: `source ~/.cargo/env && cargo test -p product-shell-tauri --lib installation && cargo check -p devbox-agent && python3 .github/scripts/check-source-cutover.py` → PASS. `git add -A && git commit -m "feat(suite): add the windowless devbox-agent app"`

---

### Task 2: pipe 서버와 `agent.status`

**Files:** Create `apps/devbox-agent/src/{server.rs,routes.rs,status.rs}`; Modify `apps/devbox-agent/{Cargo.toml,src/lib.rs}`

**Interfaces (Produces):** `server::serve(app, pipe_name)`(tokio task), `server::handle_connection<S: AsyncRead + AsyncWrite + Unpin>(stream: S, peer_product: Option<String>, routes: &Routes) -> Result<(), ProtocolError>`(peer 확인으로 알아낸 제품 id, 확인 실패면 `None`), `Routes::dispatch(component, request_json) -> Future<Result<Value, String>>`, status 응답 `{ version, generation, uptimeMs, components: ["agent.status"] }`

- [ ] **Step 1: 실패하는 테스트** — `server.rs`(tokio `duplex`로 연결을 흉내 낸다)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::{decode, encode, AgentMessage, ClientMessage, FrameDecoder, PROTOCOL_VERSION};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn read_message(client: &mut tokio::io::DuplexStream, decoder: &mut FrameDecoder) -> AgentMessage {
        loop {
            let mut buffer = [0u8; 4096];
            let read = client.read(&mut buffer).await.unwrap();
            let bodies = decoder.push(&buffer[..read]).unwrap();
            if let Some(body) = bodies.into_iter().next() {
                return decode(&body).unwrap();
            }
        }
    }

    #[tokio::test]
    async fn hello_welcome_and_status_round_trip() {
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        let routes = Routes::for_tests();
        let task = tokio::spawn(async move { handle_connection(server, Some("workspace".into()), &routes).await });
        client.write_all(&encode(&ClientMessage::Hello { protocol: PROTOCOL_VERSION, product: "workspace".into(), session: "s".into() }).unwrap()).await.unwrap();
        let mut decoder = FrameDecoder::default();
        assert!(matches!(read_message(&mut client, &mut decoder).await, AgentMessage::Welcome { .. }));
        let request = serde_json::json!({"header": {}, "method": "status", "args": {}});
        client.write_all(&encode(&ClientMessage::Call { id: 1, component: "agent.status".into(), request }).unwrap()).await.unwrap();
        match read_message(&mut client, &mut decoder).await {
            AgentMessage::Reply { id, response } => {
                assert_eq!(id, 1);
                assert_eq!(response["value"]["components"], serde_json::json!(["agent.status"]));
            }
            other => panic!("{other:?}"),
        }
        drop(client);
        let _ = task.await;
    }

    #[tokio::test]
    async fn unverified_peers_and_old_protocols_are_rejected() {
        for (peer, protocol) in [(None, PROTOCOL_VERSION), (Some("workspace".to_string()), PROTOCOL_VERSION + 1), (Some("knowledge".to_string()), PROTOCOL_VERSION)] {
            let (mut client, server) = tokio::io::duplex(4096);
            let routes = Routes::for_tests();
            let task = tokio::spawn(async move { handle_connection(server, peer, &routes).await });
            client.write_all(&encode(&ClientMessage::Hello { protocol, product: "workspace".into(), session: "s".into() }).unwrap()).await.unwrap();
            let mut decoder = FrameDecoder::default();
            assert!(matches!(read_message(&mut client, &mut decoder).await, AgentMessage::Rejected { .. }));
            assert!(task.await.unwrap().is_err());
        }
    }
}
```

  (`Routes::for_tests()`는 `agent.status`만 가진 라우팅 표. status 응답의 `header` 검사는 이 PR에서 빈 객체를 허용하고, P2-02에서 component 입장 검사와 함께 강화한다.)

- [ ] **Step 2: 구현**
  - `handle_connection`: `Hello` 읽기(2초 타임아웃) → `peer_product`가 `None`이면 `Rejected{reason:"peer_unverified"}` 후 `Err` → `check_hello(&hello, &peer_product)` 실패(버전 불일치·Hello의 product가 peer 이미지의 제품과 다름)면 `Rejected{reason: error.code()}` 후 `Err` → `Welcome{protocol, agent_version, generation}` → 루프: `Call{id, component, request}`면 `RequestIds::insert(id)` 후 `Routes::dispatch`를 spawn해 결과를 `Reply{id, response: {"operation":…, "value":…}}`로 쓴다(응답 모양은 제품 command의 `Reply`와 같게 `{"operation":{"outcome":…}, "value":…}`), `Cancel{id}`는 해당 작업을 취소, `Shutdown{}`은 앱 종료 요청(`app.exit(0)`)을 보낸다. 쓰기는 연결당 한 writer task + mpsc로 직렬화한다.
  - `serve`: tokio `ServerOptions::new().first_pipe_instance(true)`로 첫 인스턴스를 만들고(이미 있으면 다른 agent가 떠 있으므로 종료), 연결을 받을 때마다 다음 인스턴스를 만든다. peer 확인은 `suite_runtime::platform::peer_identity`의 `ProcessPeer::from_pipe`와 같은 절차를 쓰되, agent 쪽 scope는 "같은 설치 루트·같은 generation의 제품 실행 파일"이다. 공개 API가 없으면 `suite-runtime`에 `pub fn verify_suite_peer(pipe: HANDLE, generation_root: &Path) -> Result<String /*product*/, &'static str>`를 추가한다.
  - `status.rs`: `status()` 응답.
  - `lib.rs` setup에서 `tauri::async_runtime::spawn(server::serve(app.handle().clone(), identity::pipe_name(&suffix)))`.
- [ ] **Step 3: 확인·커밋** — Run: `cargo test -p devbox-agent --lib` → PASS(Windows pipe 부분은 `#[cfg(windows)]`, duplex 테스트는 모든 OS). `git add -A && git commit -m "feat(suite): serve the agent protocol over a per-installation pipe"`

---

### Task 3: 제품 쪽 클라이언트

**Files:** Create `crates/agent-client/{Cargo.toml,src/lib.rs}`; Modify `crates/product-shell-tauri/src/lib.rs`(상태 관리), `packages/product-shell/src/index.tsx`(연결 표시)

**Interfaces (Produces):** `AgentClient::new(app: &AppHandle) -> AgentClient`(portable이면 `Unsupported` 상태), `async AgentClient::call(&self, component: &str, request: Value) -> Result<Value, AgentError>`, `AgentError { Unsupported, Unavailable, Rejected(String), Remote(Value) }`, 재시도 표 `const RETRY_DELAYS_MS: [u64; 5] = [200, 500, 1000, 2000, 4000]`; product-shell describe에 `agent: "connected" | "starting" | "unavailable" | "unsupported"` 추가

- [ ] **Step 1: 실패하는 테스트** — `crates/agent-client/src/lib.rs`(연결·실행을 trait으로 추상화해 테스트)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn launches_once_then_connects_with_backoff() {
        let transport = FakeTransport::new(vec![Err(Connect::Missing), Err(Connect::Missing), Ok(())]);
        let client = AgentClient::with_transport(transport.clone());
        client.ensure_connected().await.unwrap();
        assert_eq!(transport.launches(), 1, "the agent is launched once, not per retry");
        assert_eq!(transport.attempts(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_the_retry_table() {
        let transport = FakeTransport::new(vec![Err(Connect::Missing); 10]);
        let client = AgentClient::with_transport(transport.clone());
        assert_eq!(client.ensure_connected().await.unwrap_err(), AgentError::Unavailable);
        assert_eq!(transport.attempts(), RETRY_DELAYS_MS.len() + 1);
    }

    #[tokio::test]
    async fn portable_builds_are_unsupported() {
        let client = AgentClient::unsupported();
        assert_eq!(client.call("agent.status", serde_json::json!({})).await.unwrap_err(), AgentError::Unsupported);
    }
}
```

- [ ] **Step 2: 구현** — `Transport` trait(`connect() -> Result<Connection, Connect>`, `launch() -> Result<(), ()>`)과 실제 구현(Windows: `ClientOptions::new().open(pipe_name)`, agent 경로는 현재 제품 실행 파일의 generation에서 `products/control-center/resources/suite/devbox-agent.exe`, `std::process::Command::new(path).spawn()`), `FakeTransport`(테스트). 연결은 하나를 유지하고 요청 id를 늘려 가며 `Call`을 보내고 `Reply`를 id로 매칭한다(읽기 task + `HashMap<u64, oneshot::Sender>`). 끊기면 대기 중 요청은 `Unavailable`로 끝내고 다음 호출에서 `ensure_connected`를 다시 한다. product-shell-tauri setup에서 `AgentClient::new(app)`를 manage하고, `describe` 응답에 `agent` 상태를 넣는다(연결 시도는 첫 사용 때; describe는 현재 상태만 보고 기다리지 않는다).
  - 프런트: 제품 셸 툴바의 "작업 상태" 옆에 agent 상태가 `unavailable`이면 "백그라운드 서비스 연결 안 됨" 표시를 둔다(P2-02부터 의미가 생긴다).
- [ ] **Step 3: 확인·커밋** — Run: `cargo test -p agent-client && cargo test -p product-shell-tauri --lib && pnpm --filter @devbox/product-shell exec vitest run` → PASS. `git add -A && git commit -m "feat(suite): connect products to devbox-agent on demand"`

---

### Task 4: 패키징과 업데이트

**Files:** `.github/scripts/{build-windows-packages.ps1,assemble-suite-candidate.ps1,rebuild-suite-control-center.ps1}`, `apps/devbox-control-center/src-tauri/src/core/suite_package.rs`, `apps/devbox-control-center/src-tauri/src/bootstrap/update.rs`, candidate acceptance 스크립트(`windows-suite-delivery*.{ps1,mjs}`)

- [ ] **Step 1: 실패하는 테스트** — `suite_package.rs` 테스트 모듈

```rust
    #[test]
    fn control_center_packages_include_the_agent() {
        assert!(product_file("control-center", "resources/suite/devbox-agent.exe"));
        assert!(!product_file("workspace", "resources/suite/devbox-agent.exe"));
    }
```

  그리고 기존 `validate_products` 테스트 fixture의 Control Center 파일 목록에 `resources/suite/devbox-agent.exe`를 추가한다(없으면 `suite_package_incomplete`가 되는지도 확인).

- [ ] **Step 2: 구현**
  - `suite_package.rs`: `product_file`과 `validate_products`의 Control Center 예상 파일에 `resources/suite/devbox-agent.exe`를 추가한다.
  - `build-windows-packages.ps1`: Control Center를 빌드할 때 `cargo build --locked --release -p devbox-agent`와 `Copy-Item target/release/devbox-agent.exe "$destination/resources/suite/"`를 bootstrap 줄 옆에 추가한다. `assemble-suite-candidate.ps1`의 필수 파일 목록과 `rebuild-suite-control-center.ps1`도 같게.
  - `.github/workflows/product-foundation.yml`의 `pull_request.paths`에 `apps/devbox-agent/**`와 `crates/agent-*/**`를 더한다(agent만 바뀐 PR에서도 Windows acceptance가 돌게).
  - `bootstrap/update.rs`: generation 교체(writer lease 획득) 전에 agent pipe에 연결해 `Shutdown{}`을 보내고 최대 10초 동안 pipe가 사라지기를 기다린다(연결 불가면 이미 없는 것으로 본다). 기다림이 끝나도 남아 있으면 `update_agent_busy`로 업데이트를 멈추고 "백그라운드 서비스를 종료하지 못했습니다. 모든 제품을 닫고 다시 시도해 주세요."를 보인다.
  - candidate acceptance: 설치 뒤 agent 실행 파일이 있고, 제품 하나를 띄우면 `devbox-agent.exe` 프로세스가 생기며, 업데이트 적용 뒤 이전 generation의 agent가 없어지는지 확인하는 단계를 `windows-suite-delivery-native.mjs`(또는 해당 PowerShell)에 추가한다.
- [ ] **Step 3: 확인·커밋** — Run: `cargo test -p devbox-control-center --lib && python3 .github/scripts/test-build-suite-installer.py && python3 .github/scripts/test-windows-package-candidate-config.py` → PASS. `git add -A && git commit -m "build(suite): ship devbox-agent with Control Center and stop it before updates"`

---

### Task 5: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9. 머지 후 main에서 후보 workflow를 점검용으로 한 번 실행해(공개하지 않음) agent 포함 패키지와 acceptance가 통과하는지 ledger에 남긴다.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): 제품을 열면 작업 관리자에 `devbox-agent.exe`가 하나 생기고, 두 번째 제품을 열어도 하나다. agent를 작업 관리자에서 끝낸 뒤 제품에서 상태 표시가 "연결 안 됨" → 다음 작업 때 다시 생긴다.
