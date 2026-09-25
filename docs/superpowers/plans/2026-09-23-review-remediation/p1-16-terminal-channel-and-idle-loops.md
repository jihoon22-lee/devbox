# P1-16 터미널 출력 push와 유휴 루프 제거 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** 터미널 pane마다 50ms로 출력을 당겨 오던 폴링(유휴 pane 하나가 초당 약 20회 IPC)을 Tauri `ipc::Channel` push로 바꿔 출력이 없으면 IPC가 0이 되게 하고, Webhook 리스너의 10ms sleep accept 루프를 대기형으로 바꾼다(P2, P5).

**Architecture:**
- 터미널: 세션의 `OutputBuffer`에 `tokio::sync::watch` 알림(마지막 sequence·닫힘)을 붙인다. 새 Tauri command `terminal_output_stream(window, request, channel: Channel<OutputBatch>)`이 구독을 만들고, native 작업이 "읽기 → 보냄 → 렌더러 ack 대기 → 변화 대기"를 반복한다. 한 번에 한 batch만 비행 중이라(ack 기반 흐름 제어) 렌더러 큐가 불어나지 않고, 기존 batch 검증·64KiB 상한·gap 표시(`truncated`)는 그대로다. 구독은 렌더러의 `unsubscribe`, channel 전송 실패, 세션 닫힘, 10초 ack 없음 중 먼저 오는 것으로 끝난다.
- 흐름 제어 로직은 Tauri와 분리한 순수 async 함수 `pump`로 두어 테스트한다.
- Webhook: 리스너 스레드가 current-thread tokio runtime에서 `accept`와 종료 알림(`Notify`)을 `select!`로 기다린다. 받은 연결은 지금처럼 std 스트림(blocking)으로 바꿔 연결 스레드가 처리한다.
- P5의 나머지 두 항목은 이미 끝났다: 개인정보 정규식 캐시(P0-02), Workspace 인자 재직렬화 제거(P1-14 타입 IPC).

**Tech Stack:** Rust(tokio `watch`·`Notify`·`select!`), Tauri v2 `ipc::Channel`, TypeScript

**Spec:** `review.md` §5 P2·P5

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 터미널 출력 계약 불변: frame 순서·`truncated` gap 표시·batch 64KiB·frame 512개 상한·UTF-8 경계·닫힘 신호. 렌더러 검증 코드(`terminalReplay.ts`의 batch 검사)를 그대로 재사용한다.
- 기존 `terminal_output`(pull) 메서드는 이 PR에서 지운다(호출부가 모두 stream으로 바뀐다).
- 새 외부 의존성 없음(tokio는 workspace에 있음).

## Review Focus

1. 출력이 없는 터미널 pane 10개를 열어 둠 → 터미널 관련 IPC가 초당 0회(작업 관리자·운영 로그로 확인). (Task 2·4)
2. `yes` 같은 폭주 출력 → 렌더러가 느려도 native 버퍼가 512KiB로 제한되고, 렌더러는 gap 표시 후 따라잡는다(ack 흐름 제어). (Task 1 테스트)
3. 탭을 닫거나 창을 새로고침 → 구독이 끝나고 native 작업이 남지 않는다(channel 전송 실패·unsubscribe·ack 타임아웃). (Task 1·2)
4. 세션이 끝남 → 마지막 출력과 닫힘이 전달되고 구독이 정리된다. (Task 1)
5. Webhook 리스너 중지가 즉시(200ms 안) 끝나고, 켜 둔 채 유휴일 때 accept 루프가 깨어나지 않는다. (Task 3)

## Branch · PR

- 묶음: **B8** — 브랜치 `refactor/suite/hooks-streaming-store-undo`, PR 제목 `refactor(suite): shared hooks, terminal streaming, native API Studio store, undo and agent protocol`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `perf(devbox-workspace): push terminal output over a channel and stop idle polling loops`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: 변화 알림과 `pump`

**Files:** `crates/terminal-engine/src/core/terminal_output.rs`, Create `crates/terminal-engine/src/core/output_stream.rs`

**Interfaces (Produces):**
- `OutputBuffer::subscribe(&self) -> watch::Receiver<u64>`(append·close마다 sequence 알림; close는 `u64::MAX`가 아니라 같은 sequence로 한 번 더 알림 + `is_closed`)
- `pump<R, S>(read: R, changes: watch::Receiver<u64>, send: S, acks: mpsc::Receiver<u64>, stop: watch::Receiver<bool>, start: u64, ack_timeout: Duration) -> PumpEnd` where `R: Fn(u64) -> Result<OutputBatch, &'static str>`, `S: FnMut(&OutputBatch) -> bool`
- `enum PumpEnd { Closed, Stopped, SendFailed, AckTimeout, ReadFailed }`

- [ ] **Step 1: 실패하는 테스트** — `output_stream.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::terminal_output::OutputBuffer;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::{mpsc, watch};

    fn setup() -> (Arc<Mutex<OutputBuffer>>, watch::Receiver<u64>) {
        let buffer = Arc::new(Mutex::new(OutputBuffer::default()));
        let changes = buffer.lock().unwrap().subscribe();
        (buffer, changes)
    }

    #[tokio::test(start_paused = true)]
    async fn idle_sessions_send_nothing_and_output_is_acked_one_batch_at_a_time() {
        let (buffer, changes) = setup();
        let (ack_tx, ack_rx) = mpsc::channel(4);
        let (stop_tx, stop_rx) = watch::channel(false);
        let sent = Arc::new(Mutex::new(Vec::<u64>::new()));
        let reader = buffer.clone();
        let log = sent.clone();
        let task = tokio::spawn(pump(
            move |after| reader.lock().unwrap().read(after),
            changes,
            move |batch| { log.lock().unwrap().push(batch.cursor); true },
            ack_rx,
            stop_rx,
            0,
            Duration::from_secs(10),
        ));
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert!(sent.lock().unwrap().is_empty(), "idle pane must not send");
        buffer.lock().unwrap().append("hello");
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(*sent.lock().unwrap(), vec![1]);
        buffer.lock().unwrap().append("world");
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(sent.lock().unwrap().len(), 1, "waits for the ack before sending more");
        ack_tx.send(1).await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(*sent.lock().unwrap(), vec![1, 2]);
        stop_tx.send(true).unwrap();
        assert_eq!(task.await.unwrap(), PumpEnd::Stopped);
    }

    #[tokio::test(start_paused = true)]
    async fn closing_delivers_the_last_batch_and_ends() {
        let (buffer, changes) = setup();
        let (ack_tx, ack_rx) = mpsc::channel(4);
        let (_stop_tx, stop_rx) = watch::channel(false);
        buffer.lock().unwrap().append("bye");
        buffer.lock().unwrap().close();
        let reader = buffer.clone();
        let task = tokio::spawn(pump(move |after| reader.lock().unwrap().read(after), changes, |batch| batch.closed || !batch.frames.is_empty(), ack_rx, stop_rx, 0, Duration::from_secs(10)));
        ack_tx.send(1).await.unwrap();
        assert_eq!(task.await.unwrap(), PumpEnd::Closed);
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_renderer_times_out_and_a_failed_send_ends_the_stream() {
        let (buffer, changes) = setup();
        let (_ack_tx, ack_rx) = mpsc::channel(4);
        let (_stop_tx, stop_rx) = watch::channel(false);
        buffer.lock().unwrap().append("x");
        let reader = buffer.clone();
        let end = pump(move |after| reader.lock().unwrap().read(after), changes, |_| true, ack_rx, stop_rx, 0, Duration::from_secs(10)).await;
        assert_eq!(end, PumpEnd::AckTimeout);

        let (buffer, changes) = setup();
        let (_ack_tx, ack_rx) = mpsc::channel(4);
        let (_stop_tx, stop_rx) = watch::channel(false);
        buffer.lock().unwrap().append("x");
        let reader = buffer.clone();
        let end = pump(move |after| reader.lock().unwrap().read(after), changes, |_| false, ack_rx, stop_rx, 0, Duration::from_secs(10)).await;
        assert_eq!(end, PumpEnd::SendFailed);
    }
}
```

- [ ] **Step 2: 실패 확인** — `crates/terminal-engine/Cargo.toml` `[dev-dependencies]`에 `tokio = { workspace = true, features = ["test-util"] }`를 추가한다(`start_paused`에 필요). Run: `source ~/.cargo/env && cargo test -p devbox-terminal-engine --lib output_stream` → 컴파일 실패.

- [ ] **Step 3: 구현**
  - `terminal_output.rs`: `OutputBuffer`에 `changes: watch::Sender<u64>` 필드를 추가하고(`Default`에서 `watch::channel(0).0`), `append`가 frame을 넣은 뒤 `self.changes.send_replace(self.sequence)`, `close`가 `send_replace(self.sequence)`와 함께 알리도록 한다(닫힘은 `is_closed`로 판단). `pub fn subscribe(&self) -> watch::Receiver<u64> { self.changes.subscribe() }`.
  - `output_stream.rs`:

```rust
//! Push terminal output with one batch in flight. Idle sessions wait on the
//! buffer's change notification, so they cost no IPC.
use crate::core::terminal_output::OutputBatch;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

#[derive(Debug, PartialEq, Eq)]
pub enum PumpEnd {
    Closed,
    Stopped,
    SendFailed,
    AckTimeout,
    ReadFailed,
}

pub async fn pump<R, S>(
    read: R,
    mut changes: watch::Receiver<u64>,
    mut send: S,
    mut acks: mpsc::Receiver<u64>,
    mut stop: watch::Receiver<bool>,
    start: u64,
    ack_timeout: Duration,
) -> PumpEnd
where
    R: Fn(u64) -> Result<OutputBatch, &'static str>,
    S: FnMut(&OutputBatch) -> bool,
{
    let mut cursor = start;
    loop {
        if *stop.borrow() {
            return PumpEnd::Stopped;
        }
        changes.borrow_and_update();
        let Ok(batch) = read(cursor) else {
            return PumpEnd::ReadFailed;
        };
        if !batch.frames.is_empty() || batch.truncated || batch.closed {
            if !send(&batch) {
                return PumpEnd::SendFailed;
            }
            if batch.closed {
                return PumpEnd::Closed;
            }
            let expected = batch.cursor;
            loop {
                tokio::select! {
                    _ = stop.changed() => return PumpEnd::Stopped,
                    ack = tokio::time::timeout(ack_timeout, acks.recv()) => match ack {
                        Ok(Some(acked)) if acked >= expected => break,
                        Ok(Some(_)) => continue,
                        Ok(None) => return PumpEnd::Stopped,
                        Err(_) => return PumpEnd::AckTimeout,
                    },
                }
            }
            cursor = batch.cursor;
            if batch.more {
                continue;
            }
        } else {
            cursor = batch.cursor;
        }
        tokio::select! {
            _ = stop.changed() => return PumpEnd::Stopped,
            changed = changes.changed() => if changed.is_err() { return PumpEnd::Closed; },
        }
    }
}
```

- [ ] **Step 4: 통과 확인·커밋** — Run: `cargo test -p devbox-terminal-engine --lib` → PASS(기존 `terminal_output` 테스트 포함). `git add -A && git commit -m "perf(devbox-workspace): notify terminal output changes and pump them with acks"`

---

### Task 2: native 구독 command

**Files:** `crates/terminal-engine/src/terminal_owner.rs`, `apps/devbox-workspace/src-tauri/src/{terminal_host.rs,ipc/terminal.rs,lib.rs}`, `capabilities/*.json`

**Interfaces (Produces):**
- Tauri command `terminal_output_stream(window, request: ComponentRequest<TerminalStreamCall>, channel: Channel<OutputBatch>) -> Result<Reply, Problem>`; `TerminalStreamCall::Subscribe { session_id: String, after: u64 }` → 값 `{ subscriptionId }`
- `terminal` command의 새 메서드: `ack_terminal_output { subscription_id, cursor }`, `unsubscribe_terminal_output { subscription_id }`; 지우는 메서드: `terminal_output`
- `Subscriptions`(terminal host 상태): `insert(session, sender) -> id`, `ack(id, cursor)`, `remove(id)`

- [ ] **Step 1: 실패하는 테스트** — `terminal_host.rs`(또는 `ipc/terminal.rs`) 테스트 모듈

```rust
    #[test]
    fn subscriptions_route_acks_and_are_removed_once() {
        let subscriptions = Subscriptions::default();
        let (ack_tx, mut ack_rx) = tokio::sync::mpsc::channel(4);
        let (stop_tx, _stop_rx) = tokio::sync::watch::channel(false);
        let id = subscriptions.insert("session-1".into(), ack_tx, stop_tx).unwrap();
        subscriptions.ack(&id, 7).unwrap();
        assert_eq!(ack_rx.try_recv().unwrap(), 7);
        assert!(subscriptions.remove(&id));
        assert!(!subscriptions.remove(&id));
        assert!(subscriptions.ack(&id, 8).is_err());
    }

    #[test]
    fn a_session_has_a_bounded_number_of_streams() {
        let subscriptions = Subscriptions::default();
        for _ in 0..MAX_STREAMS_PER_SESSION {
            let (ack_tx, _) = tokio::sync::mpsc::channel(4);
            let (stop_tx, _) = tokio::sync::watch::channel(false);
            subscriptions.insert("s".into(), ack_tx, stop_tx).unwrap();
        }
        let (ack_tx, _) = tokio::sync::mpsc::channel(4);
        let (stop_tx, _) = tokio::sync::watch::channel(false);
        assert!(subscriptions.insert("s".into(), ack_tx, stop_tx).is_err());
    }
```

- [ ] **Step 2: 구현**
  - `Subscriptions`: `Mutex<HashMap<String, Entry { session: String, acks: mpsc::Sender<u64>, stop: watch::Sender<bool> }>>`, `MAX_STREAMS_PER_SESSION = 4`(같은 세션을 여러 창·pane에서 볼 수 있음), id는 UUID.
  - `terminal_output_stream`: `admit` → 세션의 `OutputBuffer` 조회 → `subscribe()` → `Subscriptions::insert` → `tauri::async_runtime::spawn(pump(read, changes, move |batch| channel.send(batch.clone()).is_ok(), ack_rx, stop_rx, after, Duration::from_secs(10)))` → 작업이 끝나면 `Subscriptions::remove(id)`. 응답 값은 `{ "subscriptionId": id }`. `Channel<OutputBatch>`는 `OutputBatch: Serialize + Clone`이면 보낼 수 있다(`#[derive(Clone)]` 추가).
  - `ack_terminal_output`·`unsubscribe_terminal_output`를 `terminal` command의 host 메서드로 추가하고(lane: `TerminalIo`), `terminal_output` 메서드와 engine의 pull 경로를 지운다.
  - `lib.rs` `invoke_handler`에 `ipc::terminal_output_stream`을 등록하고 capability에 권한을 추가한다.
- [ ] **Step 3: 통과 확인·커밋** — Run: `cargo test -p devbox-workspace -p devbox-terminal-engine --lib` → PASS. `git add -A && git commit -m "perf(devbox-workspace): stream terminal output to subscribers"`

---

### Task 3: Webhook accept 대기

**Files:** `crates/webhook-host/src/commands.rs`, `crates/webhook-host/Cargo.toml`

- [ ] **Step 1: 실패하는 테스트** — `commands.rs` 테스트 모듈(기존 리스너 테스트 도우미 사용)

```rust
    #[test]
    fn stopping_the_listener_returns_promptly() {
        let (state, address) = start_test_listener(); // 기존 테스트의 리스너 시작 도우미 이름으로 바꾼다
        std::thread::sleep(std::time::Duration::from_millis(50));
        let started = std::time::Instant::now();
        stop_test_listener(&state); // 기존 중지 도우미
        assert!(started.elapsed() < std::time::Duration::from_millis(200), "{:?}", started.elapsed());
        assert!(std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_millis(100)).is_err());
    }
```

- [ ] **Step 2: 구현** — 리스너 상태에 `shutdown: Arc<tokio::sync::Notify>`를 추가하고 `stop_server`가 `running=false` 다음에 `shutdown.notify_one()`을 부르게 한다. accept 스레드 본문을 아래 모양으로 바꾼다(연결 처리 본문은 지금 코드 그대로 옮긴다).

```rust
let runtime = tokio::runtime::Builder::new_current_thread().enable_io().build().map_err(|_| LISTENER_START_ERROR)?;
runtime.block_on(async {
    std_listener.set_nonblocking(true).ok();
    let Ok(listener) = tokio::net::TcpListener::from_std(std_listener) else { return; };
    loop {
        tokio::select! {
            _ = shutdown.notified() => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    let Ok(stream) = stream.into_std() else { continue; };
                    // (기존 코드: configure_connection → register_connection → 503 처리 → 연결 스레드 생성)
                }
                Err(_) => {
                    running.store(false, Ordering::Release);
                    shutdown_active_connections(&state);
                    break;
                }
            },
        }
        if !running.load(Ordering::Acquire) {
            break;
        }
    }
});
```

  `configure_connection`이 이미 blocking 모드로 되돌리므로(주석 참고) 연결 스레드는 바뀌지 않는다. `Cargo.toml`에 `tokio = { workspace = true }`를 추가한다. `thread::sleep(Duration::from_millis(10))` 분기는 사라진다.
- [ ] **Step 3: 통과 확인·커밋** — Run: `cargo test -p devbox-webhook-host --lib` → PASS. `git commit -am "perf(devbox-api-studio): wait for webhook connections instead of polling"`

---

### Task 4: 프런트 구독

**Files:** `packages/workspace-features/src/terminal/{api.ts,lib/terminalReplay.ts,lib/terminalStream.ts,lib/terminalStream.test.ts}`

**Interfaces (Produces):** `streamTerminalOutput(sessionId: string, write: (text: string, truncated: boolean) => Promise<void>, closed: () => void, failed: () => void): () => void`

- [ ] **Step 1: 실패하는 테스트** — `lib/terminalStream.test.ts`

```ts
import { afterEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => {
  class FakeChannel<T> { onmessage: (value: T) => void = () => {}; }
  return { Channel: FakeChannel, calls: [] as { method: string; args: unknown }[], channel: null as null | { onmessage: (value: unknown) => void } };
});
vi.mock("@tauri-apps/api/core", () => ({ Channel: native.Channel }));
vi.mock("../api-transport", () => ({
  terminalCall: async (method: string, args: unknown) => { native.calls.push({ method, args }); return {}; },
  subscribeOutput: async (_sessionId: string, _after: number, channel: { onmessage: (value: unknown) => void }) => { native.channel = channel; return { subscriptionId: "sub-1" }; },
}));
import { streamTerminalOutput } from "./terminalStream";

afterEach(() => { native.calls.length = 0; native.channel = null; });

describe("streamTerminalOutput", () => {
  it("writes each batch, acks its cursor and unsubscribes on dispose", async () => {
    const writes: string[] = [];
    const dispose = streamTerminalOutput("s", async (text) => { writes.push(text); }, vi.fn(), vi.fn());
    await vi.waitFor(() => expect(native.channel).not.toBeNull());
    native.channel!.onmessage({ frames: [{ sequence: 1, data: "hi" }], cursor: 1, truncated: false, closed: false, more: false });
    await vi.waitFor(() => expect(native.calls).toContainEqual({ method: "ack_terminal_output", args: { subscriptionId: "sub-1", cursor: 1 } }));
    expect(writes).toEqual(["hi"]);
    dispose();
    await vi.waitFor(() => expect(native.calls).toContainEqual({ method: "unsubscribe_terminal_output", args: { subscriptionId: "sub-1" } }));
  });

  it("fails on an invalid batch", async () => {
    const failed = vi.fn();
    streamTerminalOutput("s", async () => {}, vi.fn(), failed);
    await vi.waitFor(() => expect(native.channel).not.toBeNull());
    native.channel!.onmessage({ frames: [{ sequence: 5, data: "x" }], cursor: 1, truncated: false, closed: false, more: false });
    await vi.waitFor(() => expect(failed).toHaveBeenCalled());
  });
});
```

  (`../api-transport`는 이 Task에서 `api.ts`의 typed 호출 두 개를 떼어 둔 작은 모듈이다. 실제 이름은 P1-14의 terminal typed call 이름에 맞춘다.)

- [ ] **Step 2: 구현** — `terminalReplay.ts`의 batch 검증 부분을 `validateBatch(batch, cursor): string`(합쳐진 텍스트 반환, 잘못되면 throw) 함수로 떼어 내고, `terminalStream.ts`가 이를 쓴다. 메시지는 한 번에 하나씩 처리한다(쓰기 중 도착한 batch는 큐에 넣지만 native가 ack 전에는 보내지 않으므로 큐 길이는 1을 넘지 않는다). `closed`면 `closed()`를 부르고 끝낸다. dispose는 `unsubscribe_terminal_output`을 보낸다. `api.ts`의 `followTerminalOutput(...)` 호출을 `streamTerminalOutput(sessionId, …)`로 바꾸고, pull 전용 `followTerminalOutput`과 50ms `delay`를 지운다.
- [ ] **Step 3: 통과 확인·커밋** — Run: `pnpm --filter @devbox/workspace-features exec vitest run src/terminal && pnpm --filter @devbox/workspace-features exec tsc --noEmit` → PASS. `git add -A && git commit -m "perf(devbox-workspace): render terminal output from the stream"`

---

### Task 5: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): 터미널 pane 여러 개 유휴 상태에서 CPU·IPC가 0에 가깝다(운영 로그의 250ms 이상 호출이 없음), `Get-Content -Wait`처럼 계속 출력하는 명령이 끊김 없이 보인다, 긴 출력(예: `dir /s C:\Windows`) 후에도 입력이 즉시 반영된다, 탭 닫기 후 native 구독이 남지 않는다. Webhook 리스너 시작·중지가 즉시 반영된다.
