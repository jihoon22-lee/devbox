# P1-19 devbox-agent 설계 ADR과 RPC 프로토콜 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** Phase 2에서 만들 사용자별 headless `devbox-agent`(런타임 작업·웹훅·수집기를 UI 수명과 분리)의 설계를 ADR 0015로 확정하고, UI와 agent가 주고받을 RPC 프레임·핸드셰이크를 `crates/agent-protocol`로 먼저 만든다(D24: Phase 1은 설계와 준비만).

**Architecture:** agent RPC 메시지의 본문은 P1-11~14에서 만든 component 타입 요청(`{header, method, args}`)을 그대로 싣는다. 그래서 어떤 component를 agent로 옮길 때 바뀌는 것은 "그 component의 dispatch가 도는 프로세스"와 "UI transport의 목적지(Tauri command → agent pipe)"뿐이다. 프레임은 기존 Suite bus와 같은 4바이트 LE 길이 + JSON이고, 연결 하나에서 요청 여러 개와 스트림(터미널 출력 등)을 id로 다중화한다. 이 PR은 코드가 순수 codec·메시지 타입·핸드셰이크 검증뿐이라 제품 동작을 바꾸지 않는다.

**Tech Stack:** Rust(serde, 순수 std; tokio 없음), Markdown(ADR)

**Spec:** `review.md` §8 C안 · `00-roadmap.md` D24·D25 · ADR `docs/adr/0015-devbox-agent.md`

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 프레임 상한 1MiB(터미널 batch 64KiB, 최대 요청 인자는 파일 저장 본문이라 1MiB를 넘는 파일 저장은 agent로 옮기지 않는다 — 파일 편집은 UI 프로세스에 남는다).
- 보안 수준은 ADR 0016을 따른다: pipe 이름은 사용자·설치별, 연결 상대는 기존 Suite bus와 같은 peer 이미지 확인(`suite-runtime` `peer_identity`)으로 같은 설치 generation의 제품만 받는다. 별도 암호화·토큰은 두지 않는다.

## Review Focus

1. 한 번에 여러 바이트로 쪼개 도착한 프레임, 두 프레임이 한 번에 도착한 경우 모두 정확히 풀린다. (Task 2)
2. 길이 0, 1MiB 초과, JSON이 아닌 본문, 알 수 없는 필드 → 연결을 끊을 수 있는 오류로 끝난다(panic·무한 대기 없음). (Task 2)
3. 프로토콜 버전이 다른 UI와 agent(업데이트 중간) → Welcome 단계에서 `protocol_mismatch`로 거절되고, UI는 "agent를 다시 시작합니다"로 복구를 시작한다(ADR 절차). (Task 2)
4. 같은 id로 두 요청 → agent가 두 번째를 `duplicate_request`로 거절한다(검증 도우미). (Task 2)
5. ADR의 업데이트 절차가 현재 writer lease·activation 단계와 모순되지 않는다(근거 절에 파일 경로). (Task 1)

## Branch · PR

- 묶음: **B8** — 브랜치 `refactor/suite/hooks-streaming-store-undo`, PR 제목 `refactor(suite): shared hooks, terminal streaming, native API Studio store, undo and agent protocol`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `docs(suite): design devbox-agent and add its RPC protocol crate`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: ADR 0015 확정

**Files:** Modify `docs/adr/0015-devbox-agent.md`(P1-01에서 "제안"으로 만든 파일)

- [ ] 아래 내용을 모두 담아 상태를 "채택(구현은 Phase 2)"으로 바꾼다.
  - **맥락**: 터미널·작업·서비스·웹훅 리스너·활동 수집이 UI 프로세스 안에 있어 창을 닫거나 UI가 죽으면 함께 끝난다. 트레이 아이콘이 제품마다 있다(최대 3개). 제품 연결·권한 주체가 네 곳이다. 리뷰 §8 C안.
  - **결정**:
    1. 사용자 세션마다 `devbox-agent.exe` 하나(서명 없음 — ADR 0016). **창 없는 Tauri 앱**으로 만든다: engine들이 `tauri::AppHandle`·상태 관리에 기대고 있어, 같은 plugin·상태를 그대로 올리면 engine 코드를 다시 쓰지 않고 옮길 수 있다(WebView는 만들지 않으므로 WebView2 프로세스가 없다). 소스는 `apps/devbox-agent`, 배포는 Control Center 패키지의 구성요소 `resources/suite/devbox-agent.exe`(Suite bootstrap과 같은 방식, 공개 자산 7개 계약 유지). 트레이 아이콘 하나(제품 열기·종료 메뉴).
    2. 시작: 어느 제품이든 시작할 때 agent가 없으면 띄운다. 로그인 자동 시작은 설정(기본 꺼짐, Workspace의 기존 `--background` 동작을 대체).
    3. 소유: Phase 2에서 순서대로 옮긴다 — 런타임 작업·서비스·스케줄(P2-02) → 웹훅 리스너(P2-03) → 활동 수집·검색 색인(P2-04, 트레이 통합·자동 시작과 함께). **터미널은 옮기지 않는다**: Workspace 터미널은 WSL 배포판 안의 multiplexer(zellij·tmux) 세션에 붙는 구조라 세션이 이미 UI 수명과 분리돼 있고, pane은 창(peer)마다 세션 guard를 가져 옮기는 비용이 크다. 파일 편집·LSP·Git 검토·UI 상태도 UI 프로세스에 남는다.
    4. IPC: named pipe `\\.\pipe\devbox-agent-<설치 접미사>`(ADR 0007의 접미사). 연결 상대는 같은 설치 generation의 제품 실행 파일인지 peer 이미지로 확인한다(`crates/suite-runtime/src/platform/peer_identity.rs`). 프레임·메시지는 `crates/agent-protocol`.
    5. 요청 본문은 component 타입 요청(`ComponentRequest<C>`)을 그대로 쓰고, agent 안에서도 `admit`와 같은 입장 검사(세션·route·deadline·replay·lane)를 한다. 스트림(로그 tail·작업 출력)은 P1-16의 ack 흐름 제어를 그대로 pipe 위에서 쓴다.
    6. 업데이트: Control Center 업데이터가 generation을 바꾸기 전에 agent에 `Shutdown`을 보내고(진행 중 PTY·작업은 기존 종료 절차로 정리), writer lease를 얻은 뒤 교체한다. 새 generation의 제품이 새 agent를 띄운다. 프로토콜 버전이 다르면 Welcome에서 거절되고 UI는 agent 재시작을 요청한다.
    7. 장애: UI는 pipe가 끊기면 1초·2초·4초 간격으로 다시 연결하고, 세 번 실패하면 "백그라운드 서비스를 다시 시작" 버튼을 보인다. agent는 비정상 종료 시 다음 UI 요청에서 다시 시작된다(항상 떠 있게 하는 감시 서비스는 두지 않는다).
  - **결과**: 좋은 점(창을 닫아도 작업 유지, 트레이 하나, 권한 주체 하나, 제품 연결 승인 불필요), 비용(프로세스 하나 추가, 업데이트 절차에 agent 종료 단계, 디버깅 대상 증가).
  - **근거**: `crates/suite-runtime/src/platform/{component_bus,peer_identity}.rs`, `crates/product-shell-tauri/src/installation.rs`(writer lease), P1-11~14(typed 요청), P1-16(stream), review §8.
- [ ] 커밋: `git commit -am "docs(suite): decide the devbox-agent process model"`

---

### Task 2: `crates/agent-protocol`

**Files:** Create `crates/agent-protocol/{Cargo.toml,src/lib.rs}`; Modify 루트 `Cargo.toml`

**Interfaces (Produces):**
- 상수 `PROTOCOL_VERSION: u32 = 1`, `MAX_FRAME_BYTES: usize = 1024 * 1024`
- `encode<T: Serialize>(&T) -> Result<Vec<u8>, ProtocolError>`
- `FrameDecoder::default()`, `push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, ProtocolError>`(완성된 본문들), `decode<T: DeserializeOwned>(&[u8]) -> Result<T, ProtocolError>`
- `ClientMessage { Hello { protocol, product, session }, Call { id: u64, component: String, request: serde_json::Value }, Cancel { id }, Ack { stream: u64, cursor: u64 }, Unsubscribe { stream: u64 }, Shutdown {} }`(serde tag `type`)
- `AgentMessage { Welcome { protocol, agent_version, generation }, Rejected { reason }, Reply { id: u64, response: serde_json::Value }, Stream { stream: u64, payload: serde_json::Value }, StreamEnd { stream: u64, reason: String } }`
- `ProtocolError { Empty, TooLarge, Malformed, Mismatch, Duplicate }` + `code(&self) -> &'static str`
- `check_hello(&ClientMessage, expected_product: &str) -> Result<(), ProtocolError>`, `RequestIds::default().insert(id) -> Result<(), ProtocolError>`

- [ ] **Step 1: 실패하는 테스트**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_survive_arbitrary_splits_and_batching() {
        let one = encode(&ClientMessage::Cancel { id: 7 }).unwrap();
        let two = encode(&ClientMessage::Ack { stream: 1, cursor: 9 }).unwrap();
        let mut joined = one.clone();
        joined.extend_from_slice(&two);
        for split in 1..joined.len() {
            let mut decoder = FrameDecoder::default();
            let mut bodies = decoder.push(&joined[..split]).unwrap();
            bodies.extend(decoder.push(&joined[split..]).unwrap());
            let messages: Vec<ClientMessage> = bodies.iter().map(|body| decode(body).unwrap()).collect();
            assert_eq!(messages, vec![ClientMessage::Cancel { id: 7 }, ClientMessage::Ack { stream: 1, cursor: 9 }], "split at {split}");
        }
    }

    #[test]
    fn empty_oversized_and_malformed_frames_fail() {
        let mut decoder = FrameDecoder::default();
        assert_eq!(decoder.push(&0u32.to_le_bytes()).unwrap_err(), ProtocolError::Empty);
        let mut decoder = FrameDecoder::default();
        assert_eq!(decoder.push(&((MAX_FRAME_BYTES as u32) + 1).to_le_bytes()).unwrap_err(), ProtocolError::TooLarge);
        assert_eq!(decode::<ClientMessage>(b"not json").unwrap_err(), ProtocolError::Malformed);
        assert_eq!(decode::<ClientMessage>(br#"{"type":"cancel","id":1,"extra":true}"#).unwrap_err(), ProtocolError::Malformed);
    }

    #[test]
    fn hello_checks_version_and_product() {
        let hello = ClientMessage::Hello { protocol: PROTOCOL_VERSION, product: "workspace".into(), session: "s".into() };
        assert!(check_hello(&hello, "workspace").is_ok());
        let old = ClientMessage::Hello { protocol: PROTOCOL_VERSION + 1, product: "workspace".into(), session: "s".into() };
        assert_eq!(check_hello(&old, "workspace").unwrap_err(), ProtocolError::Mismatch);
        assert_eq!(check_hello(&ClientMessage::Cancel { id: 1 }, "workspace").unwrap_err(), ProtocolError::Malformed);
        assert_eq!(ProtocolError::Mismatch.code(), "protocol_mismatch");
    }

    #[test]
    fn request_ids_are_unique_per_connection() {
        let mut ids = RequestIds::default();
        ids.insert(1).unwrap();
        assert_eq!(ids.insert(1).unwrap_err(), ProtocolError::Duplicate);
        ids.remove(1);
        assert!(ids.insert(1).is_ok());
    }
}
```

- [ ] **Step 2: 실패 확인** — `Cargo.toml`(`serde`, `serde_json`만 workspace 의존)과 루트 member 추가 후 Run: `source ~/.cargo/env && cargo test -p agent-protocol` → 컴파일 실패.

- [ ] **Step 3: 구현**

```rust
//! Frames and messages between product UIs and the per-user devbox-agent.
//! A frame is a 4-byte little-endian length followed by a JSON body.
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashSet;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    Empty,
    TooLarge,
    Malformed,
    Mismatch,
    Duplicate,
}

impl ProtocolError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Empty => "frame_empty",
            Self::TooLarge => "frame_too_large",
            Self::Malformed => "frame_malformed",
            Self::Mismatch => "protocol_mismatch",
            Self::Duplicate => "duplicate_request",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum ClientMessage {
    Hello { protocol: u32, product: String, session: String },
    Call { id: u64, component: String, request: serde_json::Value },
    Cancel { id: u64 },
    Ack { stream: u64, cursor: u64 },
    Unsubscribe { stream: u64 },
    Shutdown {},
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum AgentMessage {
    Welcome { protocol: u32, agent_version: String, generation: String },
    Rejected { reason: String },
    Reply { id: u64, response: serde_json::Value },
    Stream { stream: u64, payload: serde_json::Value },
    StreamEnd { stream: u64, reason: String },
}

pub fn encode<T: Serialize>(message: &T) -> Result<Vec<u8>, ProtocolError> {
    let body = serde_json::to_vec(message).map_err(|_| ProtocolError::Malformed)?;
    if body.is_empty() {
        return Err(ProtocolError::Empty);
    }
    if body.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

pub fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T, ProtocolError> {
    serde_json::from_slice(body).map_err(|_| ProtocolError::Malformed)
}

#[derive(Default)]
pub struct FrameDecoder {
    buffer: Vec<u8>,
}

impl FrameDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, ProtocolError> {
        self.buffer.extend_from_slice(bytes);
        let mut bodies = Vec::new();
        loop {
            if self.buffer.len() < 4 {
                return Ok(bodies);
            }
            let length = u32::from_le_bytes([self.buffer[0], self.buffer[1], self.buffer[2], self.buffer[3]]) as usize;
            if length == 0 {
                return Err(ProtocolError::Empty);
            }
            if length > MAX_FRAME_BYTES {
                return Err(ProtocolError::TooLarge);
            }
            if self.buffer.len() < 4 + length {
                return Ok(bodies);
            }
            bodies.push(self.buffer[4..4 + length].to_vec());
            self.buffer.drain(..4 + length);
        }
    }
}

pub fn check_hello(message: &ClientMessage, expected_product: &str) -> Result<(), ProtocolError> {
    match message {
        ClientMessage::Hello { protocol, product, session } => {
            if *protocol != PROTOCOL_VERSION {
                return Err(ProtocolError::Mismatch);
            }
            if product != expected_product || session.is_empty() || session.len() > 64 {
                return Err(ProtocolError::Malformed);
            }
            Ok(())
        }
        _ => Err(ProtocolError::Malformed),
    }
}

#[derive(Default)]
pub struct RequestIds(HashSet<u64>);

impl RequestIds {
    pub fn insert(&mut self, id: u64) -> Result<(), ProtocolError> {
        if self.0.insert(id) { Ok(()) } else { Err(ProtocolError::Duplicate) }
    }

    pub fn remove(&mut self, id: u64) {
        self.0.remove(&id);
    }
}
```

- [ ] **Step 4: 통과 확인·커밋** — Run: `cargo test -p agent-protocol && cargo clippy -p agent-protocol --all-targets -- -D warnings` → PASS. `git add -A && git commit -m "feat(crates): add the devbox-agent frame and message protocol"`

---

### Task 3: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9. PR 본문에 "제품 동작 변경 없음(설계 문서와 순수 crate)"을 적는다.
