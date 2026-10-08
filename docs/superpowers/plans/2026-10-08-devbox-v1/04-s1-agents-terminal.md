# S1 에이전트·터미널·프로젝트 (1/2: 데몬) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

- 상태: 계획 · 미착수 · 시작 조건: S0b 완료(S0b PR 머지, 확인 목록)

**Goal:** 데몬이 다음을 제공하게 한다.
- tmux 기반 터미널 세션과 바이트 스트림(흐름 제어 포함)
- 에이전트 작업의 전 과정: 준비 → 실행 → hook 상태 감지 → 검토 → 기준 반영 → 병합·PR·폐기·정리·재개
- 그에 필요한 Git 하위 기능
- 프로젝트 설정 파일(`[agent]`·`[launch]`)
- MCP 서버

화면과 Windows 통합은 [04b-s1-app](04b-s1-app.md)에서 한다.

**Architecture:**
- `protocol`에 스트림(종류 1 프레임·신용 창)을 더하고, 데몬 송신을 채널별 공정 큐로 바꾼다.
- `crates/terminal`은 tmux 서버(`-L devbox-<i>`)와 PTY attach를 관리한다.
- `crates/git`은 git CLI를 감싼다.
- `crates/agents`는 상태 기계·생성 흐름·hook 이벤트·작업 행동을 맡는다.
- 에이전트 도구는 tmux 세션 안의 시작 스크립트가 `systemd-run --user --scope`로 실행한다. hook은 `devbox agent event`로 데몬에 알린다.

**Tech Stack:** S0b 스택 + portable-pty 0.9, regex, shell-quote(직접 구현), git CLI, tmux ≥ 3.2, gh CLI.

**Spec:** [01-design.md](01-design.md) §2.1·§4.3·§5.1·§7·§9(terminal·git·agents·projects·cli)·§9.1(8·9·11)·§9.2, [09-s0a-results.md](09-s0a-results.md)(hook 세부·Codex 대기 패턴)

## Global Constraints

- S0b의 Global Constraints를 그대로 따른다.
- 작업 위치: S1 전체(04·04b의 Task 1–24)가 브랜치 `v1/s1-agents-terminal`(origin/main에서) 하나와 worktree `../devbox-wt/v1-s1-agents-terminal` 하나다. 시작할 때 `git worktree add ../devbox-wt/v1-s1-agents-terminal -b v1/s1-agents-terminal origin/main`. PR은 04b Task 24에서 한 번 연다(00-roadmap §3).
- 외부 명령은 `tokio::process::Command`로 인자 배열을 넘긴다. 셸 문자열을 조립하지 않는다. 예외는 에이전트 시작 스크립트이며, 모든 값은 `sh_quote`로 감싼다.
- tmux는 항상 `tmux -L devbox-<인스턴스> -f <devbox 소유 conf>`로 부른다. 사용자의 기본 tmux 서버를 건드리지 않는다.
- 에이전트 worktree는 `<프로젝트 상위>/<프로젝트 이름>-agents/<slug>`, 브랜치는 `agent/<slug>`다.
- 에이전트 hook은 실행 인자로만 주입한다. `~/.claude/settings.json`·`~/.codex/config.toml`을 쓰지 않는다.
- 상태는 `preparing·running·waiting·idle·failed·review·closed` 일곱 개다(01-design §9.2). 화면의 "입력 대기"는 `waiting`과 `idle`을 묶어 부른 이름이다.
- 테스트는 `DEVBOX_INSTANCE=test-<난수>`와 그 인스턴스의 tmux 소켓만 쓴다. 테스트에서 systemd unit을 만들지 않는다. tmux 서버는 직접 띄우고, scope 대신 `DEVBOX_NO_SCOPE=1`로 도구를 바로 실행한다.

## Review Focus

| 상황 | 기대 동작 | 시험 위치 |
|---|---|---|
| 터미널 출력이 쏟아지는데 화면 하나가 느림 | 그 스트림만 멈추고(신용 소진) 다른 채널의 응답·이벤트는 계속 흐름 | Task 1 |
| 다시 붙었을 때 스크롤백 | 화면에 보이던 줄은 중복 없이, 그 위 기록은 선채움으로 보임 | Task 4 |
| 병합 중 충돌 | 기준 체크아웃을 원래 상태로 되돌리고 충돌 파일 목록을 돌려줌 | Task 6 |
| 병합하지 않은 커밋이 있는 작업 폐기 | 개수를 확인 정보로 돌려주고, 확인 뒤에만 브랜치 삭제 | Task 9 |
| 병합 확인 뒤, 실행 전에 작업 폴더에 커밋이 하나 더 생김(AR-F01) | `agents.state_changed`로 거절하고 기준 브랜치는 그대로. 병합 원본은 확인한 커밋 OID | Task 9 Step 3b |
| PC 재부팅 뒤 데몬 시작 | `running·waiting·idle` 작업 중 tmux 세션이 없는 것을 `failed(session_lost)`로 바꾸고 재개 가능 | Task 8 |

---

## 작업 묶음

S1 전체가 브랜치 `v1/s1-agents-terminal` 하나, PR 하나다(00-roadmap §3). 작업 위치는 worktree `../devbox-wt/v1-s1-agents-terminal`다.
묶음은 그 안의 중간 지점이다. 묶음이 끝나면 로컬 검사(`pnpm check`, 그 시점에 없으면 있는 검사만) → `PROGRESS.md` 묶음 행 갱신 커밋 → push 한다. PR·CI는 없다. 세션 인계 지점이 된다.
PR은 마지막 묶음이 끝난 뒤 한 번 열고, 그 CI가 S1 전체를 한 번 검사한다.

| 묶음 | 과제 | 끝나면 |
|---|---|---|
| F | Task 1–2 | push |
| G | Task 3–4 | push |
| H | Task 5–6 | push |
| I | Task 7–10 | push |
| J | Task 11–12 | push |

---

### Task 1: 스트림과 채널별 공정 송신 (데몬)

**Files:**
- Create: `crates/protocol/src/stream.rs`
- Modify: `crates/protocol/src/router.rs`(`Ctx`에 `streams`), `crates/protocol/src/lib.rs`
- Create: `crates/cli/src/daemon/outbox.rs`, `crates/cli/src/daemon/streams.rs`
- Modify: `crates/cli/src/daemon/server.rs`(mpsc → `Outbox`, 종류 1·`StreamCredit` 처리, `ChannelClose` 시 스트림 정리)
- Test: `crates/protocol/src/stream.rs`(단위), `crates/cli/tests/streams.rs`(통합, 시험용 `debug.echo` 스트림 메서드)

**Interfaces:**
- Produces:
  - 상수 `STREAM_WINDOW = 256 KiB`, `FLAG_FIN = 1`
  - `stream_frame(channel, stream, flags, data) -> Frame`, `parse_stream(&[u8]) -> Option<(u32, u8, &[u8])>`
  - `trait StreamSink { fn push(&self, Frame) -> bool }`
  - `Credit`: `add(u32)`, `async take(n)`
  - `StreamTx`: `id`, `async write(&[u8]) -> Result<(), StreamError>`, `finish()`, `closed() -> CancellationToken`
  - `StreamEnds { id, tx: StreamTx, input: mpsc::Receiver<Bytes> }`
  - `trait StreamOpener { fn open(&self) -> Result<StreamEnds, StreamError> }`
  - `Ctx::open_stream()`
  - 스트림 ID는 데몬 전체에서 유일하다(`AtomicU32`). 터미널·LSP·로그 서비스가 ID로 상태를 찾는다.
  - 시험용 메서드 `debug.echo`(stream): instance가 `test-*`일 때만 라우터에 등록한다.

- [ ] **Step 1: 실패하는 단위 테스트**

```rust
// crates/protocol/src/stream.rs 끝
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Sink(Mutex<Vec<Frame>>);
    impl StreamSink for Sink {
        fn push(&self, f: Frame) -> bool {
            self.0.lock().unwrap().push(f);
            true
        }
    }

    #[tokio::test]
    async fn writer_blocks_when_credit_is_exhausted_and_resumes_after_credit() {
        let sink = Arc::new(Sink::default());
        let credit = Arc::new(Credit::new(8));
        let tx = StreamTx::new(5, 1, credit.clone(), sink.clone(), CancellationToken::new());
        tx.write(b"12345678").await.unwrap();
        let blocked = tokio::time::timeout(std::time::Duration::from_millis(50), tx.write(b"9")).await;
        assert!(blocked.is_err(), "no credit left, write must wait");
        credit.add(4);
        tx.write(b"9").await.unwrap();
        let frames = sink.0.lock().unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(parse_stream(&frames[1].body), Some((5, 0, &b"9"[..])));
    }

    #[tokio::test]
    async fn closed_stream_fails_pending_writes() {
        let sink = Arc::new(Sink::default());
        let closed = CancellationToken::new();
        let tx = StreamTx::new(1, 1, Arc::new(Credit::new(0)), sink, closed.clone());
        let w = tokio::spawn(async move { tx.write(b"x").await });
        closed.cancel();
        assert!(matches!(w.await.unwrap(), Err(StreamError::Closed)));
    }

    #[test]
    fn large_writes_split_into_chunks() {
        let f = stream_frame(1, 2, FLAG_FIN, b"abc");
        assert_eq!(parse_stream(&f.body), Some((2, FLAG_FIN, &b"abc"[..])));
        assert_eq!(parse_stream(&[0, 0]), None);
    }
}
```

- [ ] **Step 2: 실패 확인** — `cargo test -p devbox-protocol stream`

- [ ] **Step 3: 구현 — protocol::stream**

```rust
// crates/protocol/src/stream.rs (테스트 위)
//! 바이트 스트림: 종류 1 프레임 = u32 스트림 ID + u8 플래그 + 데이터. 송신은 신용 창으로 흐름을 제어한다.
use crate::frame::{Frame, FrameKind, MAX_STREAM_CHUNK};
use bytes::{BufMut, Bytes, BytesMut};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Notify};
use tokio_util::sync::CancellationToken;

pub const STREAM_WINDOW: u32 = 256 * 1024;
pub const FLAG_FIN: u8 = 1;

pub fn stream_frame(channel: u32, stream: u32, flags: u8, data: &[u8]) -> Frame {
    let mut b = BytesMut::with_capacity(5 + data.len());
    b.put_u32_le(stream);
    b.put_u8(flags);
    b.put_slice(data);
    Frame { kind: FrameKind::Stream, channel, body: b.freeze() }
}

pub fn parse_stream(body: &[u8]) -> Option<(u32, u8, &[u8])> {
    (body.len() >= 5).then(|| (u32::from_le_bytes([body[0], body[1], body[2], body[3]]), body[4], &body[5..]))
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StreamError {
    #[error("stream closed")]
    Closed,
    #[error("this connection cannot open streams")]
    Unsupported,
}

pub trait StreamSink: Send + Sync {
    /// 큐에 넣는다. 연결이 사라졌으면 false.
    fn push(&self, frame: Frame) -> bool;
}

pub struct Credit {
    avail: AtomicU32,
    notify: Notify,
}

impl Credit {
    pub fn new(initial: u32) -> Self {
        Self { avail: AtomicU32::new(initial), notify: Notify::new() }
    }
    pub fn add(&self, n: u32) {
        self.avail.fetch_add(n, Ordering::AcqRel);
        self.notify.notify_waiters();
    }
    /// n바이트를 쓸 수 있을 때까지 기다린 뒤 차감한다.
    pub async fn take(&self, n: u32, closed: &CancellationToken) -> Result<(), StreamError> {
        loop {
            let notified = self.notify.notified();
            let cur = self.avail.load(Ordering::Acquire);
            if cur >= n {
                if self.avail.compare_exchange(cur, cur - n, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                    return Ok(());
                }
                continue;
            }
            tokio::select! {
                _ = notified => {}
                _ = closed.cancelled() => return Err(StreamError::Closed),
            }
        }
    }
}

#[derive(Clone)]
pub struct StreamTx {
    pub id: u32,
    channel: u32,
    credit: Arc<Credit>,
    sink: Arc<dyn StreamSink>,
    closed: CancellationToken,
}

impl StreamTx {
    pub fn new(id: u32, channel: u32, credit: Arc<Credit>, sink: Arc<dyn StreamSink>, closed: CancellationToken) -> Self {
        Self { id, channel, credit, sink, closed }
    }
    pub async fn write(&self, data: &[u8]) -> Result<(), StreamError> {
        for chunk in data.chunks(MAX_STREAM_CHUNK) {
            if self.closed.is_cancelled() {
                return Err(StreamError::Closed);
            }
            self.credit.take(chunk.len() as u32, &self.closed).await?;
            if !self.sink.push(stream_frame(self.channel, self.id, 0, chunk)) {
                self.closed.cancel();
                return Err(StreamError::Closed);
            }
        }
        Ok(())
    }
    pub fn finish(&self) {
        let _ = self.sink.push(stream_frame(self.channel, self.id, FLAG_FIN, &[]));
        self.closed.cancel();
    }
    pub fn closed(&self) -> CancellationToken {
        self.closed.clone()
    }
}

pub struct StreamEnds {
    pub id: u32,
    pub tx: StreamTx,
    /// 클라이언트가 보낸 입력. 클라이언트가 FIN을 보내거나 채널이 닫히면 None.
    pub input: mpsc::Receiver<Bytes>,
}

pub trait StreamOpener: Send + Sync {
    fn open(&self) -> Result<StreamEnds, StreamError>;
}
```

```rust
// crates/protocol/src/router.rs 의 Ctx 교체
#[derive(Clone)]
pub struct Ctx {
    pub channel: u32,
    pub cancel: CancellationToken,
    pub streams: Option<Arc<dyn crate::stream::StreamOpener>>,
}

impl Ctx {
    pub fn open_stream(&self) -> Result<crate::stream::StreamEnds, crate::stream::StreamError> {
        self.streams.as_ref().ok_or(crate::stream::StreamError::Unsupported)?.open()
    }
}

impl std::fmt::Debug for Ctx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ctx").field("channel", &self.channel).finish()
    }
}
```

S0b 테스트의 `Ctx { channel, cancel }` 생성 코드에 `streams: None`을 추가한다. `lib.rs`에 `pub mod stream;`을 추가한다.

- [ ] **Step 4: 실패하는 통합 테스트(공정성·신용·채널 닫힘)**

```rust
// crates/cli/tests/streams.rs
mod support;
use devbox_cli::client::Client;
use support::TestDaemon;

#[tokio::test]
async fn a_stalled_stream_does_not_block_rpc_on_the_same_connection() {
    let d = TestDaemon::start();
    let c = d.client().await;
    // debug.echo: 요청한 바이트 수만큼 'x'를 계속 쓴다. 클라이언트는 신용을 주지 않는다.
    let s = c.open_stream("debug.echo", serde_json::json!({ "bytes": 4 * 1024 * 1024 })).await.unwrap();
    let got = s.read_available(std::time::Duration::from_millis(300)).await;
    assert_eq!(got.len(), devbox_protocol::stream::STREAM_WINDOW as usize, "only the initial window arrives");
    let pong = tokio::time::timeout(std::time::Duration::from_secs(2), c.call::<devbox_protocol::system::Ping>(&devbox_protocol::system::PingParams {})).await;
    assert!(pong.is_ok(), "rpc must not wait behind the stalled stream");
    s.credit(1024).await;
    let more = s.read_available(std::time::Duration::from_millis(300)).await;
    assert_eq!(more.len(), 1024);
}

#[tokio::test]
async fn input_reaches_the_handler_and_channel_close_ends_the_stream() {
    let d = TestDaemon::start();
    let c = d.client().await;
    let s = c.open_stream("debug.echo", serde_json::json!({ "bytes": 0 })).await.unwrap();
    s.credit(1024).await;
    s.write(b"hello").await;
    assert_eq!(s.read_available(std::time::Duration::from_millis(300)).await, b"hello");
    c.close_channel().await.unwrap();
    assert!(s.wait_end(std::time::Duration::from_secs(2)).await, "stream must end when its channel closes");
}

#[allow(dead_code)]
fn _uses(_: Client) {}
```

`Client`에 스트림용 보조 API를 추가한다(테스트·CLI 전용).
- `open_stream(method, params) -> ClientStream`
- `ClientStream::read_available(timeout) -> Vec<u8>`
- `credit(n)`
- `write(bytes)`
- `wait_end(timeout) -> bool`

동작:
- 종류 1 프레임은 스트림 ID별 버퍼에 쌓는다. 응답보다 먼저 온 조각은 ID별로 보관했다가 등록 시 넘긴다.
- `credit(n)`은 `Control::StreamCredit`을 보낸다.

`debug.echo` 핸들러(cli `daemon/debug.rs`):
1. `bytes > 0`이면 `tx.write`로 그만큼 `x`를 쓴다.
2. 입력을 받으면 그대로 되돌려 쓴다.
3. 입력이 끝나면 `finish()`.

```rust
// crates/cli/src/daemon/debug.rs
use devbox_protocol::{domain_error, method, Fail};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, ts_rs::TS)]
pub struct EchoParams { pub bytes: u32 }
#[derive(Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct StreamOpened { pub stream_id: u32 }
domain_error! { pub enum DebugError { Unsupported = "debug.unsupported" } }
method!(pub DebugEcho = "debug.echo", stream, EchoParams => StreamOpened, DebugError);

pub async fn echo(ctx: devbox_protocol::Ctx, p: EchoParams) -> Result<StreamOpened, Fail<DebugError>> {
    let mut ends = ctx.open_stream().map_err(|_| DebugError::Unsupported)?;
    let id = ends.id;
    tokio::spawn(async move {
        if p.bytes > 0 && ends.tx.write(&vec![b'x'; p.bytes as usize]).await.is_err() {
            return;
        }
        while let Some(data) = ends.input.recv().await {
            if ends.tx.write(&data).await.is_err() {
                return;
            }
        }
        ends.tx.finish();
    });
    Ok(StreamOpened { stream_id: id })
}
```

`StreamOpened`은 `protocol`로 옮겨 모든 스트림 메서드의 결과로 함께 쓴다(`devbox_protocol::StreamOpened`).

- [ ] **Step 5: 구현 — Outbox와 스트림 등록**

```rust
// crates/cli/src/daemon/outbox.rs
//! 채널별 큐를 라운드로빈으로 비운다. 각 채널 안에서는 JSON을 스트림보다 먼저 보낸다.
use devbox_protocol::frame::{Frame, FrameKind};
use devbox_protocol::stream::StreamSink;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use tokio::sync::Notify;

#[derive(Default)]
struct Queues {
    order: VecDeque<u32>,
    json: HashMap<u32, VecDeque<Frame>>,
    stream: HashMap<u32, VecDeque<Frame>>,
    closed: bool,
}

#[derive(Default)]
pub struct Outbox {
    q: Mutex<Queues>,
    notify: Notify,
}

impl Outbox {
    pub fn close(&self) {
        self.q.lock().expect("outbox").closed = true;
        self.notify.notify_one();
    }

    pub async fn next(&self) -> Option<Frame> {
        loop {
            let notified = self.notify.notified();
            {
                let mut q = self.q.lock().expect("outbox");
                if q.closed {
                    return None;
                }
                for _ in 0..q.order.len() {
                    let ch = q.order.pop_front().expect("non-empty");
                    let f = q.json.get_mut(&ch).and_then(VecDeque::pop_front).or_else(|| q.stream.get_mut(&ch).and_then(VecDeque::pop_front));
                    let has_more = q.json.get(&ch).is_some_and(|v| !v.is_empty()) || q.stream.get(&ch).is_some_and(|v| !v.is_empty());
                    if has_more {
                        q.order.push_back(ch);
                    }
                    if let Some(f) = f {
                        return Some(f);
                    }
                }
            }
            notified.await;
        }
    }
}

impl StreamSink for Outbox {
    fn push(&self, f: Frame) -> bool {
        let mut q = self.q.lock().expect("outbox");
        if q.closed {
            return false;
        }
        let ch = f.channel;
        let was_idle = q.json.get(&ch).is_none_or(VecDeque::is_empty) && q.stream.get(&ch).is_none_or(VecDeque::is_empty);
        match f.kind {
            FrameKind::Stream => q.stream.entry(ch).or_default().push_back(f),
            _ => q.json.entry(ch).or_default().push_back(f),
        }
        if was_idle {
            q.order.push_back(ch);
        }
        drop(q);
        self.notify.notify_one();
        true
    }
}
```

```rust
// crates/cli/src/daemon/streams.rs
//! 연결 하나의 스트림 등록부. ID는 데몬 전체에서 유일하다.
use super::outbox::Outbox;
use bytes::Bytes;
use devbox_protocol::stream::{Credit, StreamEnds, StreamError, StreamOpener, StreamTx, STREAM_WINDOW};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

static NEXT_STREAM: AtomicU32 = AtomicU32::new(1);

struct Entry {
    channel: u32,
    credit: Arc<Credit>,
    input: Option<mpsc::Sender<Bytes>>,
    closed: CancellationToken,
}

#[derive(Clone, Default)]
pub struct Streams(Arc<Mutex<HashMap<u32, Entry>>>);

impl Streams {
    pub fn opener(&self, channel: u32, outbox: Arc<Outbox>) -> Arc<dyn StreamOpener> {
        Arc::new(Opener { streams: self.clone(), channel, outbox })
    }
    pub fn input(&self, id: u32, fin: bool, data: Bytes) {
        let mut map = self.0.lock().expect("streams");
        let Some(e) = map.get_mut(&id) else { return };
        if !data.is_empty() {
            if let Some(tx) = &e.input {
                if tx.try_send(data).is_err() {
                    e.closed.cancel(); // 입력이 256조각 넘게 밀리면 끊는다(정상 타자로는 생기지 않음)
                }
            }
        }
        if fin {
            e.input = None;
        }
    }
    pub fn credit(&self, id: u32, bytes: u32) {
        if let Some(e) = self.0.lock().expect("streams").get(&id) {
            e.credit.add(bytes);
        }
    }
    pub fn close_channel(&self, channel: u32) {
        self.0.lock().expect("streams").retain(|_, e| {
            if e.channel == channel {
                e.closed.cancel();
                false
            } else {
                true
            }
        });
    }
    pub fn close_all(&self) {
        for (_, e) in self.0.lock().expect("streams").drain() {
            e.closed.cancel();
        }
    }
}

struct Opener {
    streams: Streams,
    channel: u32,
    outbox: Arc<Outbox>,
}

impl StreamOpener for Opener {
    fn open(&self) -> Result<StreamEnds, StreamError> {
        let id = NEXT_STREAM.fetch_add(1, Ordering::Relaxed);
        let credit = Arc::new(Credit::new(STREAM_WINDOW));
        let closed = CancellationToken::new();
        let (tx, rx) = mpsc::channel(256);
        self.streams.0.lock().expect("streams").insert(id, Entry { channel: self.channel, credit: credit.clone(), input: Some(tx), closed: closed.clone() });
        let streams = self.streams.clone();
        let c2 = closed.clone();
        tokio::spawn(async move {
            c2.cancelled().await;
            streams.0.lock().expect("streams").remove(&id);
        });
        Ok(StreamEnds { id, tx: StreamTx::new(id, self.channel, credit, self.outbox.clone(), closed), input: rx })
    }
}
```

`server.rs`를 이렇게 바꾼다.
- `mpsc::Sender<Frame>` 대신 `Arc<Outbox>`를 쓰고, 쓰기 작업은 `while let Some(f) = outbox.next().await { sink.send(f).await }`로 돈다.
- `json_frame`을 보낼 때 `outbox.push(...)`를 쓴다. 구독 전달 작업도 `push`가 false면 끝낸다.
- 종류 1을 받으면 `parse_stream`으로 `streams.input(id, flags & FLAG_FIN != 0, data)`를 부른다.
- `Control::StreamCredit { stream, bytes }`는 `streams.credit(stream, bytes)`로 넘긴다.
- `close_channel`은 `streams.close_channel(ch)`도 부른다. 연결이 끝나면 `streams.close_all()` + `outbox.close()`.
- 요청을 처리할 때 `Ctx { channel, cancel, streams: Some(streams.opener(ch, outbox.clone())) }`를 만든다.

라우터의 메서드 종류가 `stream`인 경우도 `add`로 등록한다(핸들러가 `ctx.open_stream()`을 부른다).

- [ ] **Step 6: 통과·커밋**

```bash
cargo test -p devbox-protocol stream && cargo test -p devbox-cli --test streams && cargo test -p devbox-cli
git add crates && git commit -m "feat(daemon): add flow-controlled byte streams and fair per-channel sending"
```

---

### Task 2: 화면 클라이언트의 스트림

**Files:**
- Modify: `app/src/rpc/client.ts`, `app/src/rpc/frame.ts`
- Test: `app/src/rpc/stream.test.ts`

**Interfaces:**
- Consumes: Task 1의 프레임 형식, 메서드 종류 `stream`, 결과 `{ streamId }`
- Produces:
  - `RpcClient.openStream(method, params, { onData(bytes, ack), onEnd() })`
  - 반환값 `Promise<{ result, streamId, write(bytes), end(), }>`
  - 규칙:
    - `ack()`를 불러야 그만큼 신용을 돌려준다(xterm `write` 콜백에서 부른다).
    - 응답보다 먼저 온 조각은 보관했다가 넘긴다(ID당 최대 256 KiB).
    - 연결이 끊기면 모든 스트림에 `onEnd()`를 호출한다.

- [ ] **Step 1: 실패하는 테스트**

```ts
// app/src/rpc/stream.test.ts
import { RpcClient, type Transport, type TransportState } from "./client";
import { decodeFrame, encodeFrame, FRAME, jsonFrame } from "./frame";

class T implements Transport {
  sent: Uint8Array[] = [];
  private f?: (x: Uint8Array) => void;
  private s?: (x: TransportState) => void;
  connect() { this.s?.("open"); }
  close() {}
  send(x: Uint8Array) { this.sent.push(x); }
  onFrame(cb: (x: Uint8Array) => void) { this.f = cb; return () => {}; }
  onState(cb: (x: TransportState) => void) { this.s = cb; return () => {}; }
  deliver(raw: Uint8Array) { this.f?.(raw); }
  drop() { this.s?.("closed"); }
}
const json = (raw: Uint8Array) => JSON.parse(new TextDecoder().decode(decodeFrame(raw).body));
const chunk = (stream: number, data: number[], flags = 0) => {
  const body = new Uint8Array(5 + data.length);
  new DataView(body.buffer).setUint32(0, stream, true);
  body[4] = flags;
  body.set(data, 5);
  return encodeFrame({ kind: FRAME.stream, channel: 0, body });
};

test("buffers early chunks, delivers data, returns credit on ack and ends on FIN", async () => {
  const t = new T();
  const c = new RpcClient(t, { version: "v", instance: "i", client: "test" });
  c.start();
  t.deliver(jsonFrame({ type: "welcome", version: "v", protocol: 1, daemonId: "d" }));
  const got: number[][] = [];
  let ended = false;
  const p = c.openStream("debug.echo", { bytes: 0 } as never, {
    onData: (b, ack) => { got.push([...b]); ack(); },
    onEnd: () => { ended = true; },
  });
  const req = json(t.sent.at(-1)!);
  t.deliver(chunk(9, [1, 2]));
  t.deliver(jsonFrame({ type: "response", id: req.id, result: { streamId: 9 } }));
  const s = await p;
  expect(s.streamId).toBe(9);
  expect(got).toEqual([[1, 2]]);
  const credit = t.sent.map((x) => decodeFrame(x)).find((f) => f.kind === FRAME.control);
  expect(JSON.parse(new TextDecoder().decode(credit!.body))).toEqual({ type: "streamCredit", stream: 9, bytes: 2 });
  s.write(new Uint8Array([7]));
  const out = decodeFrame(t.sent.at(-1)!);
  expect(out.kind).toBe(FRAME.stream);
  t.deliver(chunk(9, [], 1));
  expect(ended).toBe(true);
});

test("a dropped connection ends open streams", async () => {
  const t = new T();
  const c = new RpcClient(t, { version: "v", instance: "i", client: "test" });
  c.start();
  t.deliver(jsonFrame({ type: "welcome", version: "v", protocol: 1, daemonId: "d" }));
  let ended = false;
  const p = c.openStream("debug.echo", { bytes: 0 } as never, { onData: () => {}, onEnd: () => { ended = true; } });
  const req = json(t.sent.at(-1)!);
  t.deliver(jsonFrame({ type: "response", id: req.id, result: { streamId: 3 } }));
  await p;
  t.drop();
  expect(ended).toBe(true);
});
```

`debug.echo`는 테스트 인스턴스에서만 등록되지만 생성 타입에는 있으므로 TS에서 이름을 쓸 수 있다.

- [ ] **Step 2: 실패 확인** — `pnpm --filter app exec vitest run src/rpc/stream.test.ts`

- [ ] **Step 3: 구현(client.ts에 추가)**

```ts
// app/src/rpc/client.ts — 클래스 필드와 메서드 추가
type StreamHandlers = { onData: (bytes: Uint8Array, ack: () => void) => void; onEnd: () => void };
type StreamMethod = { [M in keyof Methods]: Methods[M]["kind"] extends "stream" ? M : never }[keyof Methods];

// RpcClient 안
private streams = new Map<number, StreamHandlers>();
private early = new Map<number, Uint8Array[]>();

async openStream<M extends StreamMethod>(method: M, params: Methods[M]["params"], handlers: StreamHandlers) {
  const result = (await this.call(method, params)) as Methods[M]["result"] & { streamId: number };
  const id = result.streamId;
  this.streams.set(id, handlers);
  for (const b of this.early.get(id) ?? []) this.deliverChunk(id, 0, b);
  this.early.delete(id);
  return {
    result,
    streamId: id,
    write: (bytes: Uint8Array) => this.transport.send(streamFrame(id, 0, bytes)),
    end: () => {
      this.transport.send(streamFrame(id, 1, new Uint8Array()));
      this.streams.delete(id);
    },
  };
}

private deliverChunk(id: number, flags: number, data: Uint8Array) {
  const h = this.streams.get(id);
  if (!h) {
    const list = this.early.get(id) ?? [];
    if (list.reduce((n, b) => n + b.length, 0) + data.length <= 256 * 1024) list.push(data);
    this.early.set(id, list);
    return;
  }
  if (data.length > 0) h.onData(data, () => this.transport.send(jsonFrame({ type: "streamCredit", stream: id, bytes: data.length }, FRAME.control)));
  if (flags & 1) {
    this.streams.delete(id);
    h.onEnd();
  }
}
```

- `onFrame`에서 `f.kind === FRAME.stream`이면 본문 앞 5바이트로 ID·플래그를 읽고 `deliverChunk`를 부른다.
- `onClosed()`에서 모든 스트림 핸들러에 `onEnd()`를 부르고 비운다(`early`도 비운다).
- `frame.ts`에 `streamFrame(id, flags, data)`를 추가한다. 형식은 `encodeFrame({kind: FRAME.stream, channel: 0, body: [u32 id LE, u8 flags, data]})`.

- [ ] **Step 4: 통과·커밋·묶음 F 끝**

```bash
pnpm --filter app exec vitest run src/rpc && pnpm --filter app typecheck && cargo run -q -p xtask -- gen-ts --check
git add app crates && git commit -m "feat(app): consume flow-controlled streams in the RPC client"
# 묶음 F 끝: PROGRESS.md의 묶음 F 행과 현재 위치를 고쳐 커밋한 뒤 push한다. PR·CI는 없다(00-roadmap §3)
pnpm check && git push origin v1/s1-agents-terminal
```

---

### Task 3: tmux 세션 관리 (`crates/terminal`)

**Files:**
- Create: `crates/terminal/Cargo.toml`, `crates/terminal/src/lib.rs`, `crates/terminal/src/tmux.rs`, `crates/terminal/src/model.rs`, `crates/terminal/src/store.rs`, `crates/terminal/assets/tmux.conf`
- Modify: `crates/cli/src/setup.rs`(`devbox-tmux@.service`와 tmux.conf 설치), `crates/cli/src/daemon/{mod,routes}.rs`, `xtask/src/main.rs`
- Modify: `crates/core/src/lib.rs`(`sh_quote` 추가)
- Modify: `.github/workflows/ci.yml`(`rust` job에 tmux 설치와 Git 신원 설정)
- Test: `crates/terminal/tests/tmux.rs`

**Interfaces:**
- Consumes: `devbox_core::{Paths, Db, Hub}`
- Produces:
  - `Tmux::new(paths)`
    - 소켓 이름 `devbox-<i>`
    - conf `~/.config/devbox/<i>/tmux.conf`
    - `ensure_server`: 테스트 인스턴스는 직접 실행, 그 밖은 `systemctl --user start devbox-tmux@<i>`, 실패하면 직접 실행
    - `new_session(id, cwd, command: Option<&str>, env)`, `kill_session`, `list() -> Vec<TmuxSession{ name, attached, command, path }>`
    - `capture_history(id, lines) -> Vec<u8>`(보이는 화면 위 기록만, ANSI 포함)
    - `capture_tail(id, lines) -> String`
    - `paste(id, text, enter: bool)`(bracketed paste)
  - `TerminalSession { id, projectId?, title, cwd, kind: shell|agent, alive, command, createdMs }`
  - 터미널 프로필: `config.toml`의 `[[terminal.profiles]] name, command`(예: `zsh -l`, `nu`). `terminal.profiles{}`(query) → `{ items: [{ name, command }] }`(맨 앞은 기본 로그인 셸 `bash -l`)
  - 메서드: `terminal.list{projectId?}`(query), `terminal.create{projectId?, cwd?, title?, command?, attachZellij?}`(mutation), `terminal.rename{id, title}`(mutation), `terminal.close{id}`(mutation, always), `terminal.zellij_list{}`(query) → `{ items: [이름] }`(zellij가 없으면 빈 목록)
  - 주제: `terminal.changed`
  - `TerminalError { NotFound{id}, CwdInvalid{path}, Tmux{reason} }`
  - `TerminalService::create_internal(kind, project_id, cwd, title, command, env) -> TerminalSession`: 에이전트가 쓴다.

- [ ] **Step 1: tmux.conf와 unit, `sh_quote`**

```rust
// crates/core/src/lib.rs 에 추가 — 셸 한 단어로 감싼다(작은따옴표 안의 작은따옴표만 바꿈)
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod sh_quote_tests {
    #[test]
    fn quotes_spaces_quotes_and_newlines() {
        assert_eq!(super::sh_quote("a b"), "'a b'");
        assert_eq!(super::sh_quote("it's"), "'it'\\''s'");
        assert_eq!(super::sh_quote("x\ny"), "'x\ny'");
    }
}
```


```
# crates/terminal/assets/tmux.conf — devbox 전용 tmux 서버 설정
set -g prefix None
unbind -a
set -g mouse off
set -g status off
set -ga terminal-overrides ',*:smcup@:rmcup@'
set -g default-terminal "tmux-256color"
set -ga terminal-overrides ',xterm-256color:Tc'
set -g window-size latest
set -g history-limit 50000
set -g allow-passthrough on
set -g set-clipboard on
set -g exit-empty off
set -g escape-time 0
```

`setup.rs`의 `render_units()`에 다음을 추가한다.

```rust
(
    "devbox-tmux@.service",
    "[Unit]\nDescription=devbox tmux server (%i)\n\n[Service]\nType=forking\nEnvironmentFile=-%h/.config/devbox/%i/env\nExecStart=/usr/bin/env tmux -L devbox-%i -f %h/.config/devbox/%i/tmux.conf start-server\nExecStop=/usr/bin/env tmux -L devbox-%i kill-server\nRemainAfterExit=yes\nKillMode=none\n\n[Install]\nWantedBy=default.target\n".into(),
),
```

- S0a에서 `-D`가 지원되면 `Type=exec`, `ExecStart=… -D …`로 바꾼다(09-s0a-results 참조).
- setup은 `include_str!("../../terminal/assets/tmux.conf")`를 `~/.config/devbox/<i>/tmux.conf`에 쓰고, `devbox-tmux@<i>.service`를 enable한다.
- 테스트(`units_reference_instance_env_file_and_socket`)의 기대 이름 목록에 이 unit을 추가한다.

- [ ] **Step 2: 실패하는 통합 테스트(실제 tmux)**

```rust
// crates/terminal/tests/tmux.rs
use devbox_terminal::tmux::Tmux;
use std::time::Duration;

fn tmux_for_test() -> (tempfile::TempDir, Tmux) {
    let home = tempfile::tempdir().unwrap();
    let conf = home.path().join("tmux.conf");
    std::fs::write(&conf, include_str!("../assets/tmux.conf")).unwrap();
    let socket = format!("devbox-test-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    (home, Tmux::with(socket, conf))
}

#[tokio::test]
async fn creates_lists_captures_pastes_and_kills_sessions() {
    let (home, t) = tmux_for_test();
    t.start_direct().await.unwrap();
    t.new_session("s1", home.path(), Some("bash --norc --noprofile"), &[("DEVBOX_X".into(), "1".into())]).await.unwrap();
    t.paste("s1", "for i in $(seq 1 120); do echo line-$i; done", true).await.unwrap();
    t.paste("s1", "echo var=$DEVBOX_X", true).await.unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;

    let list = t.list().await.unwrap();
    let s = list.iter().find(|s| s.name == "s1").expect("session listed");
    assert_eq!(s.path, home.path().canonicalize().unwrap().display().to_string());

    let tail = t.capture_tail("s1", 5).await.unwrap();
    assert!(tail.contains("var=1"), "{tail}");
    let history = String::from_utf8(t.capture_history("s1", 2000).await.unwrap()).unwrap();
    assert!(history.contains("line-1\n") || history.contains("line-1\r\n"), "history must contain lines above the screen");

    t.kill_session("s1").await.unwrap();
    assert!(t.list().await.unwrap().iter().all(|s| s.name != "s1"));
    t.kill_server().await;
}
```

`Cargo.toml` `[dev-dependencies]`에 `tempfile`, `uuid`, `tokio(macros)`를 추가한다. 이 시험은 실제 `tmux`가 필요하다. CI의 `rust` job에 `sudo apt-get install -y tmux`를 추가한다. 같은 자리에 Git 신원 설정도 넣는다(`git config --global user.name ci && git config --global user.email ci@devbox.invalid && git config --global init.defaultBranch main`). Task 6의 `crates/git` 시험은 시험 프로세스의 HOME으로 `git commit`을 하므로 CI 러너에 신원이 없으면 실패한다.

- [ ] **Step 3: 실패 확인** — `cargo test -p devbox-terminal --test tmux`

- [ ] **Step 4: 구현 — tmux.rs**

```rust
// crates/terminal/src/tmux.rs
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum TmuxError {
    #[error("tmux {args}: {stderr}")]
    Failed { args: String, stderr: String },
    #[error("tmux not runnable: {0}")]
    Spawn(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TmuxSession {
    pub name: String,
    pub attached: bool,
    pub command: String,
    pub path: String,
}

#[derive(Clone)]
pub struct Tmux {
    socket: String,
    conf: PathBuf,
}

impl Tmux {
    pub fn with(socket: String, conf: PathBuf) -> Self {
        Self { socket, conf }
    }
    pub fn for_paths(paths: &devbox_core::Paths) -> Self {
        Self::with(format!("devbox-{}", paths.instance.as_str()), paths.instance_config_dir.join("tmux.conf"))
    }
    pub fn socket(&self) -> &str {
        &self.socket
    }
    pub fn conf(&self) -> &Path {
        &self.conf
    }

    fn cmd(&self) -> Command {
        let mut c = Command::new("tmux");
        c.arg("-L").arg(&self.socket).arg("-f").arg(&self.conf);
        c
    }

    pub async fn run(&self, args: &[&str]) -> Result<String, TmuxError> {
        let out = self.cmd().args(args).output().await?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(TmuxError::Failed { args: args.join(" "), stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned() })
        }
    }

    pub async fn start_direct(&self) -> Result<(), TmuxError> {
        self.run(&["start-server", ";", "set", "-g", "exit-empty", "off"]).await.map(|_| ())
    }

    pub async fn ensure_server(&self, instance: &devbox_core::Instance) -> Result<(), TmuxError> {
        if self.run(&["list-sessions"]).await.is_ok() || self.run(&["show", "-g", "exit-empty"]).await.is_ok() {
            return Ok(());
        }
        if !instance.is_test() {
            let unit = format!("devbox-tmux@{}.service", instance.as_str());
            let ok = Command::new("systemctl").args(["--user", "start", &unit]).status().await.map(|s| s.success()).unwrap_or(false);
            if ok {
                return Ok(());
            }
        }
        self.start_direct().await
    }

    pub async fn new_session(&self, name: &str, cwd: &Path, command: Option<&str>, env: &[(String, String)]) -> Result<(), TmuxError> {
        let cwd = cwd.display().to_string();
        let mut args: Vec<String> = vec!["new-session".into(), "-d".into(), "-s".into(), name.into(), "-c".into(), cwd, "-x".into(), "200".into(), "-y".into(), "50".into()];
        for (k, v) in env {
            args.push("-e".into());
            args.push(format!("{k}={v}"));
        }
        if let Some(c) = command {
            args.push(c.into());
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(&refs).await.map(|_| ())
    }

    pub async fn kill_session(&self, name: &str) -> Result<(), TmuxError> {
        self.run(&["kill-session", "-t", &format!("={name}")]).await.map(|_| ())
    }

    pub async fn kill_server(&self) {
        let _ = self.run(&["kill-server"]).await;
    }

    pub async fn list(&self) -> Result<Vec<TmuxSession>, TmuxError> {
        let out = match self.run(&["list-sessions", "-F", "#{session_name}\t#{session_attached}\t#{pane_current_command}\t#{pane_current_path}"]).await {
            Ok(o) => o,
            Err(TmuxError::Failed { stderr, .. }) if stderr.contains("no server running") || stderr.contains("no sessions") => return Ok(vec![]),
            Err(e) => return Err(e),
        };
        Ok(out
            .lines()
            .filter_map(|l| {
                let mut p = l.split('\t');
                Some(TmuxSession { name: p.next()?.into(), attached: p.next()? != "0", command: p.next()?.into(), path: p.next()?.into() })
            })
            .collect())
    }

    /// 보이는 화면 위의 기록만(중복 없이) ANSI와 함께 가져온다.
    pub async fn capture_history(&self, name: &str, lines: u32) -> Result<Vec<u8>, TmuxError> {
        let out = self.cmd().args(["capture-pane", "-p", "-e", "-J", "-S", &format!("-{lines}"), "-E", "-1", "-t", &format!("={name}:")]).output().await?;
        if out.status.success() {
            Ok(out.stdout.split(|b| *b == b'\n').flat_map(|l| l.iter().copied().chain(*b"\r\n")).collect())
        } else {
            Err(TmuxError::Failed { args: "capture-pane".into(), stderr: String::from_utf8_lossy(&out.stderr).into_owned() })
        }
    }

    pub async fn capture_tail(&self, name: &str, lines: u32) -> Result<String, TmuxError> {
        self.run(&["capture-pane", "-p", "-J", "-S", &format!("-{lines}"), "-t", &format!("={name}:")]).await
    }

    /// bracketed paste로 붙여 넣고, 원하면 Enter를 보낸다.
    pub async fn paste(&self, name: &str, text: &str, enter: bool) -> Result<(), TmuxError> {
        let buffer = format!("devbox-{}", uuid::Uuid::new_v4().simple());
        let mut child = self.cmd().args(["load-buffer", "-b", &buffer, "-"]).stdin(std::process::Stdio::piped()).spawn()?;
        child.stdin.take().expect("stdin").write_all(text.as_bytes()).await?;
        let st = child.wait().await?;
        if !st.success() {
            return Err(TmuxError::Failed { args: "load-buffer".into(), stderr: String::new() });
        }
        self.run(&["paste-buffer", "-p", "-d", "-b", &buffer, "-t", &format!("={name}:")]).await?;
        if enter {
            self.run(&["send-keys", "-t", &format!("={name}:"), "Enter"]).await?;
        }
        Ok(())
    }
}
```

`Cargo.toml` `[dependencies]`에 `uuid`, `tokio(process, io-util)`, `thiserror`를 추가한다.

- [ ] **Step 5: 구현 — 모델·저장소·서비스·메서드**

```rust
// crates/terminal/src/model.rs
use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SessionKind { Shell, Agent }

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSession {
    pub id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub cwd: String,
    pub kind: SessionKind,
    pub alive: bool,
    pub command: String,
    pub created_ms: i64,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct SessionList { pub rev: String, pub items: Vec<TerminalSession> }

#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ListSessions { pub project_id: Option<String> }

#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreateSession { pub project_id: Option<String>, pub cwd: Option<String>, pub title: Option<String>, pub command: Option<String>, pub attach_zellij: Option<String> }

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct RenameSession { pub id: String, pub title: String }

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct CloseSession { pub id: String }

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct ZellijSessions { pub items: Vec<String> }

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TerminalChanged { pub rev: String }

domain_error! {
    pub enum TerminalError {
        NotFound { id: String } = "terminal.not_found",
        CwdInvalid { path: String } = "terminal.cwd_invalid",
        Tmux { reason: String } = "terminal.tmux_failed",
        StreamUnavailable = "terminal.stream_unavailable",
    }
}
```

```rust
// crates/terminal/src/store.rs
pub const MIGRATION_1: &str = "
CREATE TABLE terminal_sessions (
  id TEXT PRIMARY KEY,
  project_id TEXT,
  title TEXT NOT NULL,
  cwd TEXT NOT NULL,
  kind TEXT NOT NULL,
  created_ms INTEGER NOT NULL
);";
```

`lib.rs`의 `TerminalService { db, hub, tmux, paths, projects: Arc<ProjectsService>, attachments }` 동작:
- **`list`:** DB 행과 `tmux.list()`를 이름(id)으로 합쳐 `alive`·`command`를 채운다. tmux에만 있고 DB에 없는 세션은 보이지 않는다(사용자가 `tmux -L devbox-<i>`로 직접 만든 것은 무시).
- **`create`:**
  1. cwd를 정한다(지정 > 프로젝트 경로 > HOME). `canonicalize`한 뒤 디렉터리인지 확인한다.
  2. id는 `t` + uuid 앞 8자다. 제목은 지정값이 없으면 폴더 이름이다.
  3. `ensure_server` → `new_session(id, cwd, command 또는 기본 로그인 셸 bash -l)` → DB insert → `terminal.changed` 발행.
  4. `attach_zellij`가 있으면 command 대신 `zellij attach <sh_quote(이름)>`을 쓰고 제목 기본값을 `zellij: <이름>`으로 한다. 기존 zellij 세션에 붙는 단순화 경로다(새 세션은 tmux로만 만든다).
- **`zellij_list`:** `zellij list-sessions --short --no-formatting`을 실행한다. 명령이 없거나 실패하면 빈 목록이다.
- **`close`:** `kill_session`(이미 없으면 무시) → DB delete → 발행.
- **`rename`:** DB update → 발행.

메서드 선언:

```rust
method!(pub TerminalList = "terminal.list", query, ListSessions => SessionList, TerminalError);
method!(pub TerminalCreate = "terminal.create", mutation, CreateSession => TerminalSession, TerminalError);
method!(pub TerminalRename = "terminal.rename", mutation, RenameSession => devbox_protocol::Done, TerminalError);
method!(pub TerminalClose = "terminal.close", mutation(always), CloseSession => devbox_protocol::Done, TerminalError);
method!(pub TerminalZellijList = "terminal.zellij_list", query, devbox_protocol::system::PingParams => ZellijSessions, TerminalError);
topic!(pub TerminalChangedTopic = "terminal.changed", TerminalChanged);
```

`sh_quote`는 `devbox_core::sh_quote`로 두고(Task 7의 `agents::plan::sh_quote`도 이것을 다시 내보낸다) 터미널과 에이전트가 함께 쓴다.

`ProjectsService`에 `pub async fn get(&self, id: &str) -> Result<Option<Project>, Fail<ProjectsError>>`를 추가한다. 데몬 `build_services`에 TerminalService를 만들어 넣고, 라우터와 xtask에 등록한다.

- [ ] **Step 6: 서비스 단위 테스트**

```rust
// crates/terminal/tests/service.rs — 서비스 수준: 만들기 → 목록(alive) → 닫기
use devbox_terminal::*;

#[tokio::test]
async fn create_list_close_round_trip() {
    let h = support::Harness::new().await; // 임시 HOME + test 인스턴스 + Db + Hub + ProjectsService + tmux 직접 실행
    let s = h.terminal.create(CreateSession { project_id: None, cwd: Some(h.home.display().to_string()), title: None, command: Some("bash --norc".into()) }).await.unwrap();
    let list = h.terminal.list(ListSessions { project_id: None }).await.unwrap();
    assert!(list.items.iter().any(|x| x.id == s.id && x.alive));
    h.terminal.close(CloseSession { id: s.id.clone() }).await.unwrap();
    assert!(h.terminal.list(ListSessions { project_id: None }).await.unwrap().items.iter().all(|x| x.id != s.id));
    let e = h.terminal.create(CreateSession { project_id: None, cwd: Some("/no/such".into()), title: None, command: None }).await.unwrap_err();
    assert!(matches!(e, devbox_protocol::Fail::Domain(TerminalError::CwdInvalid { .. })));
}
```

`tests/support/mod.rs`의 `Harness`는 S0b `TestDaemon`처럼 임시 디렉터리를 만들고 각 서비스를 직접 생성한다. Drop할 때 tmux 서버를 끈다.

- [ ] **Step 7: 통과·생성·커밋**

```bash
cargo test -p devbox-terminal && cargo test -p devbox-cli && cargo run -q -p xtask -- gen-ts
git add crates xtask app/src/rpc/gen && git commit -m "feat(terminal): manage tmux sessions on a devbox-owned server"
```

---

### Task 4: 터미널 attach 스트림 (PTY)

**Files:**
- Create: `crates/terminal/src/attach.rs`
- Modify: `crates/terminal/src/lib.rs`(메서드 `terminal.attach`·`terminal.resize`), `crates/terminal/Cargo.toml`(`portable-pty = "0.9"`)
- Test: `crates/cli/tests/terminal_attach.rs`

**Interfaces:**
- Consumes: Task 1의 `Ctx::open_stream`, Task 3의 `Tmux`
- Produces:
  - `terminal.attach{ id, cols, rows, readOnly }`(stream) → `StreamOpened { streamId }`
    1. 기록 선채움(`capture_history` 2,000줄)을 먼저 보낸다.
    2. 그 뒤 `tmux attach` PTY 출력을 보낸다. 입력 스트림은 PTY에 쓴다.
    3. `readOnly`면 `attach -f read-only,ignore-size`로 붙고 입력은 버린다.
  - `terminal.resize{ streamId, cols, rows }`(mutation)
  - attach는 세션이 없으면 `terminal.not_found`, 스트림을 못 열면 `terminal.stream_unavailable`이다.
  - PTY 클라이언트는 스트림이 닫히면 종료한다(tmux 세션은 유지).

- [ ] **Step 1: 실패하는 통합 테스트**

```rust
// crates/cli/tests/terminal_attach.rs
mod support;
use support::TestDaemon;
use std::time::Duration;

#[tokio::test]
async fn attach_streams_output_accepts_input_and_prefills_history_on_reattach() {
    let d = TestDaemon::start();
    let c = d.client().await;
    let s: serde_json::Value = serde_json::from_str(&c.raw_call("terminal.create", serde_json::json!({ "cwd": d.home.path(), "command": "bash --norc --noprofile" }), None).await.unwrap()).unwrap();
    let id = s["id"].as_str().unwrap().to_owned();

    let a = c.open_stream("terminal.attach", serde_json::json!({ "id": id, "cols": 100, "rows": 30, "readOnly": false })).await.unwrap();
    a.credit(1 << 20).await;
    a.write(b"for i in $(seq 1 80); do echo row-$i; done\r").await;
    let out = a.read_until(b"row-80", Duration::from_secs(5)).await;
    assert!(out.windows(6).any(|w| w == b"row-80"));
    a.end().await;

    let b = c.open_stream("terminal.attach", serde_json::json!({ "id": id, "cols": 100, "rows": 30, "readOnly": true })).await.unwrap();
    b.credit(1 << 20).await;
    let again = b.read_available(Duration::from_millis(800)).await;
    let text = String::from_utf8_lossy(&again);
    assert!(text.contains("row-1\r\n"), "history above the screen must be prefilled");
    assert_eq!(text.matches("row-80").count(), 1, "the visible screen must not be duplicated");
}
```

`ClientStream`에 `read_until(pattern, timeout)`과 `end()`를 추가한다.

- [ ] **Step 2: 실패 확인** — `cargo test -p devbox-cli --test terminal_attach`

- [ ] **Step 3: 구현 — attach.rs**

```rust
// crates/terminal/src/attach.rs
use crate::tmux::Tmux;
use bytes::Bytes;
use devbox_protocol::stream::StreamEnds;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Clone, Default)]
pub struct Attachments(Arc<Mutex<HashMap<u32, Box<dyn MasterPty + Send>>>>);

impl Attachments {
    pub fn resize(&self, stream: u32, cols: u16, rows: u16) -> bool {
        self.0.lock().expect("attachments").get(&stream).is_some_and(|m| m.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).is_ok())
    }
}

pub async fn attach(tmux: &Tmux, atts: &Attachments, id: &str, cols: u16, rows: u16, read_only: bool, mut ends: StreamEnds) -> Result<(), String> {
    let history = tmux.capture_history(id, 2000).await.map_err(|e| e.to_string())?;
    let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string())?;
    let mut cmd = CommandBuilder::new("tmux");
    cmd.args(["-L", tmux.socket(), "-f"]);
    cmd.arg(tmux.conf());
    cmd.arg("attach");
    if read_only {
        cmd.args(["-f", "read-only,ignore-size"]);
    }
    cmd.args(["-t", &format!("={id}")]);
    cmd.env("TERM", "xterm-256color");
    let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let stream_id = ends.id;
    atts.0.lock().expect("attachments").insert(stream_id, pair.master);

    // PTY 읽기(블로킹) → 4조각 버퍼 → 신용을 기다리는 비동기 송신. 신용이 없으면 PTY 읽기도 멈춘다.
    let (out_tx, mut out_rx) = mpsc::channel::<Bytes>(4);
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 32 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if out_tx.blocking_send(Bytes::copy_from_slice(&buf[..n])).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let tx = ends.tx.clone();
    let closed = tx.closed();
    let atts2 = atts.clone();
    tokio::spawn(async move {
        if !history.is_empty() && tx.write(&history).await.is_err() {
            return;
        }
        loop {
            tokio::select! {
                chunk = out_rx.recv() => match chunk {
                    Some(b) => if tx.write(&b).await.is_err() { break },
                    None => break,
                },
                input = ends.input.recv() => match input {
                    Some(b) if !read_only => { let _ = writer.write_all(&b); let _ = writer.flush(); }
                    Some(_) => {}
                    None => break,
                },
                _ = closed.cancelled() => break,
            }
        }
        let _ = child.kill();
        atts2.0.lock().expect("attachments").remove(&stream_id);
        tx.finish();
    });
    Ok(())
}
```

`TerminalService::attach(ctx, p)`:
1. 세션 존재를 확인한다(`tmux.list()`).
2. `ctx.open_stream()`으로 스트림을 연다.
3. `attach(...)`를 부르고 `StreamOpened { stream_id }`를 돌려준다.

`resize`는 `attachments.resize`를 부른다(없으면 `not_found`). 둘을 `method!`로 선언하고 라우터에 등록한다.

```rust
#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AttachSession { pub id: String, pub cols: u16, pub rows: u16, pub read_only: bool }
#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ResizeStream { pub stream_id: u32, pub cols: u16, pub rows: u16 }
method!(pub TerminalAttach = "terminal.attach", stream, AttachSession => devbox_protocol::StreamOpened, TerminalError);
method!(pub TerminalResize = "terminal.resize", mutation, ResizeStream => devbox_protocol::Done, TerminalError);
```

- [ ] **Step 4: 통과·커밋·묶음 G 끝**

```bash
cargo test -p devbox-cli --test terminal_attach && cargo test --workspace && cargo run -q -p xtask -- gen-ts
git add crates xtask app/src/rpc/gen && git commit -m "feat(terminal): stream tmux attach clients through a PTY with prefilled history"
# 묶음 G 끝: PROGRESS.md의 묶음 G 행과 현재 위치를 고쳐 커밋한 뒤 push한다. PR·CI는 없다(00-roadmap §3)
pnpm check && git push origin v1/s1-agents-terminal
```

---

### Task 5: Git 조회 (`crates/git`)

**Files:**
- Create: `crates/git/Cargo.toml`, `crates/git/src/lib.rs`, `crates/git/src/run.rs`, `crates/git/src/status.rs`, `crates/git/src/diff.rs`
- Test: `crates/git/tests/query.rs`(임시 저장소)

**Interfaces:**
- Produces(모두 async, `GitError`):
  - `current_branch(dir) -> Option<String>`
  - `status(dir) -> StatusSummary { branch, ahead, behind, staged, unstaged, untracked, conflicted: Vec<String>, clean }`
  - `ahead_behind(dir, base, head) -> (u32, u32)`
  - `changed_files(dir, base) -> Vec<FileChange { path, old_path?, status: A|M|D|R|U, added, removed }>`: `base...HEAD` + 작업 트리 + 추적 안 되는 파일
  - `file_patch(dir, base, path) -> String`(unified diff, `--no-ext-diff --no-color`)
  - `log_range(dir, base) -> Vec<Commit { sha, subject, author, time_ms }>`
  - `branches(dir) -> Vec<String>`
  - `GitError { NotRepo, Failed { op, stderr }, BaseDirty { files }, BaseNotCheckedOut { branch }, Conflict { files }, BranchExists { branch } }`: `domain_error!`, 코드는 `git.*`

- [ ] **Step 1: 실패하는 테스트(임시 저장소)**

```rust
// crates/git/tests/query.rs
use devbox_git as git;
use std::process::Command;

fn sh(dir: &std::path::Path, args: &[&str]) {
    let st = Command::new("git").args(args).current_dir(dir).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t").status().unwrap();
    assert!(st.success(), "git {args:?}");
}

fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    sh(d.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(d.path().join("a.txt"), "one\n").unwrap();
    sh(d.path(), &["add", "."]);
    sh(d.path(), &["commit", "-qm", "init"]);
    d
}

#[tokio::test]
async fn reports_branch_changes_commits_and_ahead_behind() {
    let d = repo();
    sh(d.path(), &["checkout", "-qb", "agent/x"]);
    std::fs::write(d.path().join("a.txt"), "one\ntwo\n").unwrap();
    sh(d.path(), &["commit", "-qam", "add two"]);
    std::fs::write(d.path().join("b.txt"), "new\n").unwrap();
    std::fs::write(d.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();

    assert_eq!(git::current_branch(d.path()).await.unwrap().as_deref(), Some("agent/x"));
    let st = git::status(d.path()).await.unwrap();
    assert!(!st.clean);
    assert_eq!(st.untracked, 1);
    assert_eq!(git::ahead_behind(d.path(), "main", "HEAD").await.unwrap(), (1, 0));

    let files = git::changed_files(d.path(), "main").await.unwrap();
    let a = files.iter().find(|f| f.path == "a.txt").unwrap();
    assert_eq!((a.added, a.removed), (2, 0));
    assert!(files.iter().any(|f| f.path == "b.txt" && f.status == git::ChangeStatus::Added));

    let patch = git::file_patch(d.path(), "main", "a.txt").await.unwrap();
    assert!(patch.contains("+three"));
    let log = git::log_range(d.path(), "main").await.unwrap();
    assert_eq!(log.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(), vec!["add two"]);
}

#[tokio::test]
async fn non_repositories_are_reported() {
    let d = tempfile::tempdir().unwrap();
    assert!(matches!(git::status(d.path()).await, Err(git::GitError::NotRepo)));
}
```

- [ ] **Step 2: 실패 확인** — `cargo test -p devbox-git --test query`

- [ ] **Step 3: 구현**

```rust
// crates/git/src/run.rs
use crate::GitError;
use std::path::Path;
use tokio::process::Command;

pub async fn git(dir: &Path, args: &[&str]) -> Result<String, GitError> {
    let out = Command::new("git")
        .arg("-C").arg(dir)
        .args(["-c", "core.quotepath=off", "-c", "color.ui=false"])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .output()
        .await
        .map_err(|e| GitError::Failed { op: args.first().copied().unwrap_or("git").into(), stderr: e.to_string() })?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    if stderr.contains("not a git repository") {
        return Err(GitError::NotRepo);
    }
    Err(GitError::Failed { op: args.first().copied().unwrap_or("git").into(), stderr })
}
```

```rust
// crates/git/src/lib.rs
pub mod diff;
pub mod run;
pub mod status;

use devbox_protocol::domain_error;
pub use diff::{changed_files, file_patch, log_range, ChangeStatus, Commit, FileChange};
pub use status::{ahead_behind, branches, current_branch, status, StatusSummary};

domain_error! {
    pub enum GitError {
        NotRepo = "git.not_repo",
        Failed { op: String, stderr: String } = "git.failed",
        BaseDirty { files: Vec<String> } = "git.base_dirty",
        WorktreeDirty { files: Vec<String> } = "git.worktree_dirty",
        BaseNotCheckedOut { branch: String } = "git.base_not_checked_out",
        Conflict { files: Vec<String> } = "git.conflict",
        BranchExists { branch: String } = "git.branch_exists",
        PushFailed { stderr: String } = "git.push_failed",
        GhMissing = "git.gh_missing",
    }
}
```

`Failed.stderr`는 사용자에게 보여 줘도 되는 git 메시지다. 경로가 들어 있을 수 있지만 개인 앱이고 화면 표시용이라 허용한다. 로그에는 남기지 않는다.

```rust
// crates/git/src/status.rs
use crate::run::git;
use crate::GitError;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ts_rs::TS)]
pub struct StatusSummary {
    pub branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    pub conflicted: Vec<String>,
    pub clean: bool,
}

pub async fn current_branch(dir: &Path) -> Result<Option<String>, GitError> {
    let s = git(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"]).await;
    match s {
        Ok(b) => Ok(Some(b.trim().to_owned())),
        Err(GitError::Failed { .. }) => Ok(None), // detached
        Err(e) => Err(e),
    }
}

/// `git status --porcelain=v2 --branch` 해석.
pub async fn status(dir: &Path) -> Result<StatusSummary, GitError> {
    let out = git(dir, &["status", "--porcelain=v2", "--branch", "-z"]).await?;
    let mut s = StatusSummary::default();
    let mut parts = out.split('\0').peekable();
    while let Some(entry) = parts.next() {
        if let Some(rest) = entry.strip_prefix("# branch.head ") {
            s.branch = (rest != "(detached)").then(|| rest.to_owned());
        } else if let Some(rest) = entry.strip_prefix("# branch.ab ") {
            let mut it = rest.split(' ');
            s.ahead = it.next().and_then(|a| a.trim_start_matches('+').parse().ok()).unwrap_or(0);
            s.behind = it.next().and_then(|b| b.trim_start_matches('-').parse().ok()).unwrap_or(0);
        } else if entry.starts_with("1 ") || entry.starts_with("2 ") {
            let xy = &entry[2..4];
            if xy.as_bytes()[0] != b'.' { s.staged += 1; }
            if xy.as_bytes()[1] != b'.' { s.unstaged += 1; }
            if entry.starts_with("2 ") { parts.next(); } // 이름 바꾸기의 원래 경로
        } else if let Some(rest) = entry.strip_prefix("u ") {
            s.conflicted.push(rest.rsplit(' ').next().unwrap_or_default().to_owned());
        } else if entry.starts_with("? ") {
            s.untracked += 1;
        }
    }
    s.clean = s.staged == 0 && s.unstaged == 0 && s.untracked == 0 && s.conflicted.is_empty();
    Ok(s)
}

pub async fn ahead_behind(dir: &Path, base: &str, head: &str) -> Result<(u32, u32), GitError> {
    let out = git(dir, &["rev-list", "--left-right", "--count", &format!("{base}...{head}")]).await?;
    let mut it = out.split_whitespace().map(|x| x.parse::<u32>().unwrap_or(0));
    let behind = it.next().unwrap_or(0);
    let ahead = it.next().unwrap_or(0);
    Ok((ahead, behind))
}

pub async fn branches(dir: &Path) -> Result<Vec<String>, GitError> {
    Ok(git(dir, &["for-each-ref", "--format=%(refname:short)", "refs/heads"]).await?.lines().map(String::from).collect())
}
```

```rust
// crates/git/src/diff.rs
use crate::run::git;
use crate::GitError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum ChangeStatus { Added, Modified, Deleted, Renamed, Unmerged }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct FileChange { pub path: String, pub old_path: Option<String>, pub status: ChangeStatus, pub added: u32, pub removed: u32 }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Commit { pub sha: String, pub subject: String, pub author: String, pub time_ms: i64 }

/// 기준 브랜치와의 공통 조상부터 작업 트리까지의 변경(커밋 + 커밋 안 한 것 + 추적 안 되는 파일).
pub async fn changed_files(dir: &Path, base: &str) -> Result<Vec<FileChange>, GitError> {
    let merge_base = git(dir, &["merge-base", base, "HEAD"]).await?.trim().to_owned();
    let mut map: BTreeMap<String, FileChange> = BTreeMap::new();
    let ns = git(dir, &["diff", "--numstat", "-z", "-M", &merge_base]).await?;
    let names = git(dir, &["diff", "--name-status", "-z", "-M", &merge_base]).await?;
    let mut it = names.split('\0').filter(|s| !s.is_empty());
    while let Some(code) = it.next() {
        let (status, old_path) = match code.as_bytes()[0] {
            b'A' => (ChangeStatus::Added, None),
            b'D' => (ChangeStatus::Deleted, None),
            b'R' => (ChangeStatus::Renamed, it.next().map(String::from)),
            b'U' => (ChangeStatus::Unmerged, None),
            _ => (ChangeStatus::Modified, None),
        };
        let Some(path) = it.next() else { break };
        map.insert(path.into(), FileChange { path: path.into(), old_path, status, added: 0, removed: 0 });
    }
    let mut it = ns.split('\0').filter(|s| !s.is_empty());
    while let Some(line) = it.next() {
        let mut f = line.splitn(3, '\t');
        let (a, r, p) = (f.next().unwrap_or("0"), f.next().unwrap_or("0"), f.next().unwrap_or(""));
        let path = if p.is_empty() { it.next(); it.next().unwrap_or_default().to_owned() } else { p.to_owned() };
        if let Some(fc) = map.get_mut(&path) {
            fc.added = a.parse().unwrap_or(0);
            fc.removed = r.parse().unwrap_or(0);
        }
    }
    for path in git(dir, &["ls-files", "--others", "--exclude-standard", "-z"]).await?.split('\0').filter(|s| !s.is_empty()) {
        let lines = std::fs::read(dir.join(path)).map(|b| b.iter().filter(|c| **c == b'\n').count() as u32).unwrap_or(0);
        map.insert(path.into(), FileChange { path: path.into(), old_path: None, status: ChangeStatus::Added, added: lines, removed: 0 });
    }
    Ok(map.into_values().collect())
}

pub async fn file_patch(dir: &Path, base: &str, path: &str) -> Result<String, GitError> {
    let merge_base = git(dir, &["merge-base", base, "HEAD"]).await?.trim().to_owned();
    let tracked = git(dir, &["diff", "--no-ext-diff", "-M", &merge_base, "--", path]).await?;
    if !tracked.is_empty() {
        return Ok(tracked);
    }
    // 추적 안 되는 새 파일: /dev/null 과 비교
    let out = tokio::process::Command::new("git").arg("-C").arg(dir).args(["diff", "--no-index", "--no-ext-diff", "--", "/dev/null", path]).output().await
        .map_err(|e| GitError::Failed { op: "diff".into(), stderr: e.to_string() })?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub async fn log_range(dir: &Path, base: &str) -> Result<Vec<Commit>, GitError> {
    let out = git(dir, &["log", "--format=%H%x1f%s%x1f%an%x1f%at", &format!("{base}..HEAD")]).await?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let mut p = l.split('\x1f');
            Some(Commit { sha: p.next()?.into(), subject: p.next()?.into(), author: p.next()?.into(), time_ms: p.next()?.parse::<i64>().ok()? * 1000 })
        })
        .collect())
}
```

`--numstat -z`의 이름 바꾸기 줄 형식(경로 칸이 비고 뒤에 옛 경로·새 경로가 따로 옴)은 위처럼 처리한다. 형식이 다르면 테스트에 이름 바꾸기 사례를 추가해 맞춘다.

- [ ] **Step 4: 통과·커밋**

```bash
cargo test -p devbox-git
git add crates && git commit -m "feat(git): read status, changes, patches and commits through git CLI"
```

---

### Task 6: Git 변경 (worktree·병합·기준 반영·PR·정리)

**Files:**
- Create: `crates/git/src/mutate.rs`
- Test: `crates/git/tests/mutate.rs`

**Interfaces:**
- Produces:
  - `worktree_add(repo, dir, branch, base)`(새 브랜치 + worktree)
  - `worktree_remove(repo, dir, force)`
  - `delete_branch(repo, branch, force)`
  - `unmerged_count(repo, branch, base) -> u32`
  - `merge_into(base_dir, base_branch, branch, strategy: Merge|Squash{message}|Rebase{worktree}) -> MergeOutcome { sha }`:
    - 사전 조건: 기준 체크아웃이 `base_branch`이고 깨끗해야 한다.
    - 충돌이 나면 되돌린 뒤 `Conflict{files}`를 돌려준다.
  - `rebase_onto(worktree, base) -> RebaseOutcome { Done | Conflict { files } }`: 충돌이 나면 rebase를 진행 중인 상태로 둔다(에이전트가 해결).
  - `rebase_abort(worktree)`
  - `push_and_pr(worktree, branch, base, title, body) -> String(url)`: `gh`가 없으면 `GhMissing`
  - `merged_agent_branches(repo, base) -> Vec<String>`

- [ ] **Step 1: 실패하는 테스트**

```rust
// crates/git/tests/mutate.rs
use devbox_git as git;
use git::mutate::{self, MergeStrategy};
// sh()·repo()는 tests/query.rs와 같다(복사).

#[tokio::test]
async fn worktree_merge_and_cleanup() {
    let d = repo();
    let wt = d.path().parent().unwrap().join(format!("{}-agents/x", d.path().file_name().unwrap().to_string_lossy()));
    mutate::worktree_add(d.path(), &wt, "agent/x", "main").await.unwrap();
    std::fs::write(wt.join("c.txt"), "c\n").unwrap();
    sh(&wt, &["add", "."]);
    sh(&wt, &["commit", "-qm", "add c"]);
    assert_eq!(mutate::unmerged_count(d.path(), "agent/x", "main").await.unwrap(), 1);

    let out = mutate::merge_into(d.path(), "main", "agent/x", MergeStrategy::Squash { message: "agent: add c".into() }).await.unwrap();
    assert_eq!(out.sha.len(), 40);
    assert!(d.path().join("c.txt").exists());
    mutate::worktree_remove(d.path(), &wt, true).await.unwrap();
    mutate::delete_branch(d.path(), "agent/x", true).await.unwrap();
    assert!(!wt.exists());
}

#[tokio::test]
async fn conflicting_merge_restores_the_base_and_lists_files() {
    let d = repo();
    let wt = d.path().with_extension("wt");
    mutate::worktree_add(d.path(), &wt, "agent/y", "main").await.unwrap();
    std::fs::write(wt.join("a.txt"), "theirs\n").unwrap();
    sh(&wt, &["commit", "-qam", "theirs"]);
    std::fs::write(d.path().join("a.txt"), "ours\n").unwrap();
    sh(d.path(), &["commit", "-qam", "ours"]);
    let e = mutate::merge_into(d.path(), "main", "agent/y", MergeStrategy::Merge).await.unwrap_err();
    assert!(matches!(&e, git::GitError::Conflict { files } if files == &vec!["a.txt".to_string()]), "{e:?}");
    assert!(git::status(d.path()).await.unwrap().clean, "base checkout must be restored");
}

#[tokio::test]
async fn dirty_base_refuses_to_merge() {
    let d = repo();
    let wt = d.path().with_extension("wt2");
    mutate::worktree_add(d.path(), &wt, "agent/z", "main").await.unwrap();
    std::fs::write(d.path().join("dirty.txt"), "x").unwrap();
    let e = mutate::merge_into(d.path(), "main", "agent/z", MergeStrategy::Merge).await.unwrap_err();
    assert!(matches!(e, git::GitError::BaseDirty { .. }));
}

#[tokio::test]
async fn rebase_conflict_is_left_in_progress_for_the_agent() {
    let d = repo();
    let wt = d.path().with_extension("wt3");
    mutate::worktree_add(d.path(), &wt, "agent/r", "main").await.unwrap();
    std::fs::write(wt.join("a.txt"), "agent\n").unwrap();
    sh(&wt, &["commit", "-qam", "agent"]);
    std::fs::write(d.path().join("a.txt"), "main\n").unwrap();
    sh(d.path(), &["commit", "-qam", "main"]);
    let r = mutate::rebase_onto(&wt, "main").await.unwrap();
    assert_eq!(r, mutate::RebaseOutcome::Conflict { files: vec!["a.txt".into()] });
    mutate::rebase_abort(&wt).await.unwrap();
    assert!(git::status(&wt).await.unwrap().conflicted.is_empty());
}
```

- [ ] **Step 2: 실패 확인** — `cargo test -p devbox-git --test mutate`

- [ ] **Step 3: 구현**

```rust
// crates/git/src/mutate.rs
use crate::run::git;
use crate::{current_branch, status, GitError};
use std::path::Path;

#[derive(Debug, Clone)]
pub enum MergeStrategy {
    Merge,
    Squash { message: String },
    Rebase { worktree: std::path::PathBuf },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MergeOutcome {
    pub sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebaseOutcome {
    Done,
    Conflict { files: Vec<String> },
}

pub async fn worktree_add(repo: &Path, dir: &Path, branch: &str, base: &str) -> Result<(), GitError> {
    if crate::branches(repo).await?.iter().any(|b| b == branch) {
        return Err(GitError::BranchExists { branch: branch.into() });
    }
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| GitError::Failed { op: "mkdir".into(), stderr: e.to_string() })?;
    }
    git(repo, &["worktree", "add", "-b", branch, &dir.display().to_string(), base]).await.map(|_| ())
}

pub async fn worktree_remove(repo: &Path, dir: &Path, force: bool) -> Result<(), GitError> {
    let d = dir.display().to_string();
    let args: Vec<&str> = if force { vec!["worktree", "remove", "--force", &d] } else { vec!["worktree", "remove", &d] };
    git(repo, &args).await?;
    git(repo, &["worktree", "prune"]).await.map(|_| ())
}

pub async fn delete_branch(repo: &Path, branch: &str, force: bool) -> Result<(), GitError> {
    git(repo, &["branch", if force { "-D" } else { "-d" }, branch]).await.map(|_| ())
}

pub async fn unmerged_count(repo: &Path, branch: &str, base: &str) -> Result<u32, GitError> {
    Ok(git(repo, &["rev-list", "--count", &format!("{base}..{branch}")]).await?.trim().parse().unwrap_or(0))
}

async fn conflicted(dir: &Path) -> Result<Vec<String>, GitError> {
    Ok(git(dir, &["diff", "--name-only", "--diff-filter=U", "-z"]).await?.split('\0').filter(|s| !s.is_empty()).map(String::from).collect())
}

async fn ensure_clean_base(base_dir: &Path, base_branch: &str) -> Result<(), GitError> {
    if current_branch(base_dir).await?.as_deref() != Some(base_branch) {
        return Err(GitError::BaseNotCheckedOut { branch: base_branch.into() });
    }
    let st = git(base_dir, &["status", "--porcelain", "-z"]).await?;
    let files: Vec<String> = st.split('\0').filter(|s| s.len() > 3).map(|s| s[3..].to_owned()).collect();
    if files.is_empty() { Ok(()) } else { Err(GitError::BaseDirty { files }) }
}

pub async fn merge_into(base_dir: &Path, base_branch: &str, branch: &str, strategy: MergeStrategy) -> Result<MergeOutcome, GitError> {
    ensure_clean_base(base_dir, base_branch).await?;
    match strategy {
        MergeStrategy::Merge => {
            if git(base_dir, &["merge", "--no-ff", "--no-edit", branch]).await.is_err() {
                let files = conflicted(base_dir).await?;
                let _ = git(base_dir, &["merge", "--abort"]).await;
                return Err(GitError::Conflict { files });
            }
        }
        MergeStrategy::Squash { message } => {
            if git(base_dir, &["merge", "--squash", branch]).await.is_err() {
                let files = conflicted(base_dir).await?;
                let _ = git(base_dir, &["reset", "--merge"]).await;
                return Err(GitError::Conflict { files });
            }
            git(base_dir, &["commit", "-m", &message]).await?;
        }
        MergeStrategy::Rebase { worktree } => {
            if let RebaseOutcome::Conflict { files } = rebase_onto(&worktree, base_branch).await? {
                rebase_abort(&worktree).await?;
                return Err(GitError::Conflict { files });
            }
            git(base_dir, &["merge", "--ff-only", branch]).await?;
        }
    }
    Ok(MergeOutcome { sha: git(base_dir, &["rev-parse", "HEAD"]).await?.trim().to_owned() })
}

pub async fn rebase_onto(worktree: &Path, base: &str) -> Result<RebaseOutcome, GitError> {
    match git(worktree, &["rebase", base]).await {
        Ok(_) => Ok(RebaseOutcome::Done),
        Err(_) => {
            let files = conflicted(worktree).await?;
            if files.is_empty() {
                return Err(GitError::Failed { op: "rebase".into(), stderr: "rebase failed without conflicts".into() });
            }
            Ok(RebaseOutcome::Conflict { files })
        }
    }
}

pub async fn rebase_abort(worktree: &Path) -> Result<(), GitError> {
    git(worktree, &["rebase", "--abort"]).await.map(|_| ())
}

pub async fn push_and_pr(worktree: &Path, branch: &str, base: &str, title: &str, body: &str) -> Result<String, GitError> {
    if which::which("gh").is_err() {
        return Err(GitError::GhMissing);
    }
    git(worktree, &["push", "-u", "origin", branch]).await.map_err(|e| match e {
        GitError::Failed { stderr, .. } => GitError::PushFailed { stderr },
        other => other,
    })?;
    let out = tokio::process::Command::new("gh")
        .args(["pr", "create", "--base", base, "--head", branch, "--title", title, "--body", body])
        .current_dir(worktree)
        .output()
        .await
        .map_err(|e| GitError::Failed { op: "gh".into(), stderr: e.to_string() })?;
    if !out.status.success() {
        return Err(GitError::Failed { op: "gh pr create".into(), stderr: String::from_utf8_lossy(&out.stderr).trim().into() });
    }
    Ok(String::from_utf8_lossy(&out.stdout).lines().last().unwrap_or_default().trim().to_owned())
}

pub async fn merged_agent_branches(repo: &Path, base: &str) -> Result<Vec<String>, GitError> {
    Ok(git(repo, &["branch", "--list", "agent/*", "--merged", base, "--format=%(refname:short)"]).await?.lines().map(String::from).collect())
}
```

`Cargo.toml`에 `which = "7"`을 추가한다. `gh` 탐색 PATH는 데몬 환경(EnvironmentFile)을 따른다.

- [ ] **Step 4: 통과·커밋·묶음 H 끝**

```bash
cargo test -p devbox-git
git add crates && git commit -m "feat(git): add worktree, merge, rebase, pull request and cleanup operations"
# 묶음 H 끝: PROGRESS.md의 묶음 H 행과 현재 위치를 고쳐 커밋한 뒤 push한다. PR·CI는 없다(00-roadmap §3)
pnpm check && git push origin v1/s1-agents-terminal
```

---

### Task 7: 에이전트 모델·상태 기계·저장소·도구 프로필

**Files:**
- Create: `crates/agents/Cargo.toml`, `crates/agents/src/lib.rs`, `crates/agents/src/model.rs`, `crates/agents/src/state.rs`, `crates/agents/src/plan.rs`, `crates/agents/src/store.rs`, `crates/agents/src/profile.rs`
- Modify: `crates/core/src/config.rs`(`[agents]` 절)
- Test: 각 파일 단위 테스트

**Interfaces:**
- Produces:
  - `AgentState { Preparing, Running, Waiting, Idle, Failed, Review, Closed }`
  - `AgentEvent { SetupOk, SetupFailed { code }, SessionStarted { session_id }, PromptSubmitted, NeedsInput { message }, TurnComplete { message }, OutputWaiting, OutputResumed, Exited { code }, SessionLost, UserInstructed, UserReview, Closed { how: CloseHow } }`
  - `CloseHow { Merged, PullRequest, Discarded }`
  - `fn next(state, &event) -> Option<AgentState>`, `fn is_attention(state) -> bool`
  - `AgentTask`(직렬화 camelCase): id, projectId, title, profile, baseBranch, branch?, worktreePath?, workdir, sessionId?, terminalId?, state, reason?, attention?{ kind, message }, port, prompt, followUps[], tokens?{ input, output, cacheRead, cacheWrite }, prUrl?, createdMs, updatedMs, closedMs?
  - plan.rs:
    - `checked_title`, `slug(title, now)`, `unique_slug(base, taken)`, `branch_for(slug)`
    - `worktree_dir(project_path, slug) -> PathBuf`(`<parent>/<name>-agents/<slug>`)
    - `sh_quote(&str) -> String`
  - `ToolProfile { id, label, command, hooks: Claude|Codex|None, waitingPatterns: Vec<String> }`. 기본 프로필: `claude`, `codex`. 사용자 정의는 `config.toml [[agents.profiles]]`.

- [ ] **Step 1: 실패하는 테스트 — 상태 기계**

```rust
// crates/agents/src/state.rs 끝
#[cfg(test)]
mod tests {
    use super::*;
    use AgentEvent as E;
    use AgentState as S;

    #[test]
    fn follows_the_design_table() {
        let cases = [
            (S::Preparing, E::SetupOk, Some(S::Running)),
            (S::Preparing, E::SetupFailed { code: 1 }, Some(S::Failed)),
            (S::Running, E::NeedsInput { message: "Bash?".into() }, Some(S::Waiting)),
            (S::Running, E::TurnComplete { message: None }, Some(S::Idle)),
            (S::Idle, E::PromptSubmitted, Some(S::Running)),
            (S::Waiting, E::PromptSubmitted, Some(S::Running)),
            (S::Running, E::OutputWaiting, Some(S::Waiting)),
            (S::Waiting, E::OutputResumed, Some(S::Running)),
            (S::Idle, E::OutputWaiting, None),
            (S::Running, E::Exited { code: 0 }, Some(S::Review)),
            (S::Waiting, E::Exited { code: 2 }, Some(S::Failed)),
            (S::Idle, E::SessionLost, Some(S::Failed)),
            (S::Failed, E::UserInstructed, Some(S::Running)),
            (S::Review, E::UserInstructed, Some(S::Running)),
            (S::Idle, E::UserReview, Some(S::Review)),
            (S::Review, E::Closed { how: CloseHow::Merged }, Some(S::Closed)),
            (S::Closed, E::PromptSubmitted, None),
            (S::Closed, E::SessionLost, None),
        ];
        for (from, ev, want) in cases {
            assert_eq!(next(from, &ev), want, "{from:?} + {ev:?}");
        }
    }

    #[test]
    fn attention_states_are_waiting_idle_and_failed() {
        assert!(is_attention(S::Waiting) && is_attention(S::Idle) && is_attention(S::Failed));
        assert!(!is_attention(S::Running) && !is_attention(S::Review) && !is_attention(S::Closed));
    }
}
```

- [ ] **Step 2: 실패하는 테스트 — plan**

```rust
// crates/agents/src/plan.rs 끝 (v0.9.0 agent_hub/plan.rs 테스트 이식 + 추가)
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn slugs_are_ascii_and_bounded() {
        assert_eq!(slug("Fix login bug!", 0), "fix-login-bug");
        assert_eq!(slug("한글만", 1_700_000_000_000), "task-1700000000");
        assert!(slug(&"x".repeat(100), 0).len() <= MAX_SLUG_BYTES);
    }

    #[test]
    fn unique_slugs_get_numbered_suffixes() {
        let taken: HashSet<String> = ["fix".into(), "fix-2".into()].into();
        assert_eq!(unique_slug("fix", &taken).unwrap(), "fix-3");
    }

    #[test]
    fn worktree_dir_sits_next_to_the_project() {
        assert_eq!(worktree_dir(std::path::Path::new("/home/u/projects/devbox"), "fix"), std::path::PathBuf::from("/home/u/projects/devbox-agents/fix"));
    }

    #[test]
    fn shell_quoting_survives_quotes_and_newlines() {
        assert_eq!(sh_quote("a b"), "'a b'");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
        assert_eq!(sh_quote("x\ny"), "'x\ny'");
    }

    #[test]
    fn titles_must_be_short_and_printable() {
        assert!(checked_title("  ok  ").is_ok());
        assert!(checked_title("").is_err());
        assert!(checked_title("a\u{7}b").is_err());
        assert!(checked_title(&"가".repeat(121)).is_err());
    }
}
```

- [ ] **Step 3: 실패 확인** — `cargo test -p devbox-agents`

- [ ] **Step 4: 구현 — state.rs·plan.rs**

```rust
// crates/agents/src/state.rs (테스트 위)
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentState { Preparing, Running, Waiting, Idle, Failed, Review, Closed }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CloseHow { Merged, PullRequest, Discarded }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEvent {
    SetupOk,
    SetupFailed { code: i32 },
    SessionStarted { session_id: String },
    PromptSubmitted,
    NeedsInput { message: String },
    TurnComplete { message: Option<String> },
    OutputWaiting,
    OutputResumed,
    Exited { code: i32 },
    SessionLost,
    UserInstructed,
    UserReview,
    Closed { how: CloseHow },
}

pub fn next(s: AgentState, e: &AgentEvent) -> Option<AgentState> {
    use AgentEvent as E;
    use AgentState as S;
    if s == S::Closed {
        return None;
    }
    let live = matches!(s, S::Running | S::Waiting | S::Idle);
    let to = match e {
        E::SetupOk if s == S::Preparing => S::Running,
        E::SetupFailed { .. } if s == S::Preparing => S::Failed,
        E::SessionStarted { .. } if s == S::Preparing => S::Running,
        E::PromptSubmitted if live => S::Running,
        E::NeedsInput { .. } if live => S::Waiting,
        E::TurnComplete { .. } if live => S::Idle,
        E::OutputWaiting if s == S::Running => S::Waiting,
        E::OutputResumed if s == S::Waiting => S::Running,
        E::Exited { code: 0 } if live || s == S::Preparing => S::Review,
        E::Exited { .. } if live || s == S::Preparing => S::Failed,
        E::SessionLost if live || s == S::Preparing => S::Failed,
        E::UserInstructed if matches!(s, S::Idle | S::Waiting | S::Failed | S::Review) => S::Running,
        E::UserReview => S::Review,
        E::Closed { .. } => S::Closed,
        _ => return None,
    };
    (to != s).then_some(to)
}

pub fn is_attention(s: AgentState) -> bool {
    matches!(s, AgentState::Waiting | AgentState::Idle | AgentState::Failed)
}

/// `failed`로 갈 때 남길 이유. 셸의 `$?`는 신호로 끝나면 128+신호 번호다.
/// 외부 종료(이 PC의 wsl-resource-guard 화면의 세션 종료, 커널 OOM 등)를 일반 실패와 구분해 보여 주기 위해서다.
pub fn fail_reason(e: &AgentEvent) -> Option<String> {
    match e {
        AgentEvent::Exited { code } if (129..=192).contains(code) => Some(match code - 128 {
            9 => "killed:KILL".into(),
            15 => "killed:TERM".into(),
            n => format!("killed:{n}"),
        }),
        AgentEvent::Exited { code } if *code != 0 => Some(format!("exit:{code}")),
        AgentEvent::SetupFailed { code } => Some(format!("setup:{code}")),
        AgentEvent::SessionLost => Some("session_lost".into()),
        _ => None,
    }
}

#[cfg(test)]
mod fail_reason_tests {
    use super::*;
    #[test]
    fn signal_exits_are_reported_as_external_kills() {
        assert_eq!(fail_reason(&AgentEvent::Exited { code: 137 }).as_deref(), Some("killed:KILL"));
        assert_eq!(fail_reason(&AgentEvent::Exited { code: 143 }).as_deref(), Some("killed:TERM"));
        assert_eq!(fail_reason(&AgentEvent::Exited { code: 2 }).as_deref(), Some("exit:2"));
        assert_eq!(fail_reason(&AgentEvent::Exited { code: 0 }), None);
    }
}
```

상태를 `failed`로 바꿀 때 `task.reason = fail_reason(&event)`을 저장한다(`session_lost`도 이 함수가 만든다). 화면은 `killed:*`을 "외부에서 종료됨(메모리 부족이나 다른 도구가 끝냈을 수 있습니다)"으로 보이고 [재개]를 권한다(04b Task 17의 행 메시지. 오류 코드가 아니라 상태 이유라 `messages.ts`가 아닌 목록 화면이 문구를 정한다).

```rust
// crates/agents/src/plan.rs (테스트 위) — v0.9.0 apps/devbox-workspace/src-tauri/src/agent_hub/plan.rs의 slug 규칙 이식
use crate::model::AgentsError;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const MAX_TITLE_CHARS: usize = 120;
pub const MAX_SLUG_BYTES: usize = 40;

pub fn checked_title(title: &str) -> Result<&str, AgentsError> {
    let t = title.trim();
    if t.is_empty() || t.chars().count() > MAX_TITLE_CHARS || t.chars().any(char::is_control) {
        Err(AgentsError::TitleInvalid)
    } else {
        Ok(t)
    }
}

pub fn slug(title: &str, now_ms: u64) -> String {
    let mut out = String::new();
    for b in title.bytes() {
        if out.len() == MAX_SLUG_BYTES {
            break;
        }
        if b.is_ascii_alphanumeric() {
            out.push(b.to_ascii_lowercase() as char);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() { format!("task-{}", now_ms / 1000) } else { out.into() }
}

pub fn unique_slug(base: &str, taken: &HashSet<String>) -> Result<String, AgentsError> {
    for n in 1..=99 {
        let candidate = if n == 1 {
            base.into()
        } else {
            let suffix = format!("-{n}");
            let keep = MAX_SLUG_BYTES - suffix.len();
            format!("{}{suffix}", &base[..base.len().min(keep)].trim_end_matches('-'))
        };
        if !taken.contains(&candidate) {
            return Ok(candidate);
        }
    }
    Err(AgentsError::SlugExhausted)
}

pub fn branch_for(slug: &str) -> String {
    format!("agent/{slug}")
}

pub fn worktree_dir(project: &Path, slug: &str) -> PathBuf {
    let name = project.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into());
    project.parent().unwrap_or(Path::new("/")).join(format!("{name}-agents")).join(slug)
}

pub use devbox_core::sh_quote; // Task 3에서 core에 둔 함수
```

- [ ] **Step 5: 구현 — model.rs·profile.rs·store.rs·설정**

```rust
// crates/agents/src/model.rs
use crate::state::{AgentState, CloseHow};
use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Attention { pub kind: String, pub message: String, pub at_ms: i64 }

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Tokens { pub input: u64, pub output: u64, pub cache_read: u64, pub cache_write: u64 }

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FollowUp { pub at_ms: i64, pub text: String }

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentTask {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub profile: String,
    pub base_branch: String,
    pub branch: Option<String>,
    pub worktree_path: Option<String>,
    pub workdir: String,
    pub session_id: Option<String>,
    pub terminal_id: Option<String>,
    pub state: AgentState,
    pub reason: Option<String>,
    pub attention: Option<Attention>,
    pub port: u16,
    pub prompt: String,
    pub follow_ups: Vec<FollowUp>,
    pub tokens: Option<Tokens>,
    pub pr_url: Option<String>,
    pub closed_how: Option<CloseHow>,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub closed_ms: Option<i64>,
}

domain_error! {
    pub enum AgentsError {
        TitleInvalid = "agents.title_invalid",
        SlugExhausted = "agents.slug_exhausted",
        NotFound { id: String } = "agents.not_found",
        ProjectNotFound { id: String } = "agents.project_not_found",
        ProfileNotFound { id: String } = "agents.profile_not_found",
        BadState { state: String } = "agents.bad_state",
        WorktreeDirty { files: Vec<String> } = "agents.worktree_dirty",
        BaseDirty { files: Vec<String> } = "agents.base_dirty",
        BaseNotCheckedOut { branch: String } = "agents.base_not_checked_out",
        Conflict { files: Vec<String> } = "agents.conflict",
        UnmergedCommits { count: u32 } = "agents.unmerged_commits",
        GhMissing = "agents.gh_missing",
        Git { stderr: String } = "agents.git_failed",
        SessionMissing = "agents.session_missing",
    }
}
```

`devbox_git::GitError`를 `AgentsError`로 바꾸는 `impl From<GitError> for Fail<AgentsError>`를 둔다.
- `BaseDirty`·`WorktreeDirty`·`BaseNotCheckedOut`·`Conflict`·`GhMissing`은 같은 이름으로 옮긴다.
- `Failed`·`PushFailed`는 `Git{stderr}`로 바꾼다.
- `NotRepo`는 `Git{stderr:"not a git repository"}`로 바꾼다.

```rust
// crates/agents/src/profile.rs
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Hooks { Claude, Codex, None }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ToolProfile {
    pub id: String,
    pub label: String,
    pub command: String,
    pub hooks: Hooks,
    #[serde(default)]
    pub waiting_patterns: Vec<String>,
}

/// 기본 프로필. Codex 대기 패턴은 S0a 측정(09-s0a-results "Codex 승인 대기 화면 문구")으로 채운다.
pub fn builtin() -> Vec<ToolProfile> {
    vec![
        ToolProfile { id: "claude".into(), label: "Claude Code".into(), command: "claude".into(), hooks: Hooks::Claude, waiting_patterns: vec![] },
        ToolProfile {
            id: "codex".into(),
            label: "Codex".into(),
            command: "codex".into(),
            hooks: Hooks::Codex,
            waiting_patterns: vec![r"(?i)allow .* (command|edit)\?".into(), r"(?i)\(y\)es.*\(n\)o".into(), r"(?i)approve".into()],
        },
    ]
}

pub fn resolve(custom: &[ToolProfile], id: &str) -> Option<ToolProfile> {
    custom.iter().chain(builtin().iter()).find(|p| p.id == id).cloned()
}
```

core `Config`에 다음을 추가한다.

```rust
#[serde(default)]
pub agents: AgentsConfig,

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct AgentsConfig {
    pub profiles: Vec<serde_json::Value>, // agents crate가 ToolProfile로 해석(core는 도메인 타입을 모름)
    pub cleanup_after_close: Option<bool>,
}
```

`AgentsConfig.profiles`는 toml 값을 `toml::Value`로 받아 agents에서 `ToolProfile`로 바꾼다. 위의 `serde_json::Value`는 `toml::Value`로 쓴다.

```rust
// crates/agents/src/store.rs — 작업은 JSON 한 덩어리로 저장하고 조회 칼럼만 따로 둔다.
pub const MIGRATION_1: &str = "
CREATE TABLE agents_tasks (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  state TEXT NOT NULL,
  updated_ms INTEGER NOT NULL,
  body TEXT NOT NULL
);
CREATE INDEX agents_tasks_project ON agents_tasks(project_id);
CREATE INDEX agents_tasks_state ON agents_tasks(state);";
```

저장 함수 `put(tx, &AgentTask)`·`get(tx, id)`·`list(tx, include_closed_since_ms)`를 둔다. `state` 칼럼은 serde 이름(`"waiting"` 등)을 쓴다.

- [ ] **Step 6: 통과·커밋**

```bash
cargo test -p devbox-agents
git add crates && git commit -m "feat(agents): add the agent task model, state machine, naming rules and tool profiles"
```

---

### Task 8: 에이전트 작업 생성·시작 스크립트·hook 이벤트

**Files:**
- Create: `crates/agents/src/service.rs`, `crates/agents/src/start.rs`, `crates/agents/src/hooks.rs`
- Create: `crates/projects/src/config.rs`(Task 11에서 넓힐 `[agent]` 절을 이 과제에서 먼저 최소로 만든다: `copy`·`setup`·`teardown`·`test`)
- Modify: `crates/cli/src/main.rs`(`devbox agent event` 하위 명령), `crates/cli/src/daemon/*`
- Test: `crates/agents/src/start.rs`(스크립트 생성 단위), `crates/agents/src/hooks.rs`(이벤트 해석 단위), `crates/cli/tests/agents.rs`(통합, 가짜 도구)

**Interfaces:**
- Consumes: Task 3 `TerminalService::create_internal`, Task 6 `worktree_add`, Task 7 전부, `ProjectsService::get`·`config`
- Produces:
  - 메서드:
    - `agents.create{ projectId, title, profile, baseBranch?, worktree: bool, prompt }`(mutation) → `AgentTask`(state=`preparing`)
    - `agents.list{ includeClosedHours? }`(query) → `{ rev, items }`
    - `agents.get{ id }`(query)
    - `agents.profiles{}`(query) → `{ items: ToolProfile[] }`(기본 + 사용자 정의, 새 작업 대화상자용)
    - `agents.event{ agentId, source, payload, code? }`(mutation, 내부용)
  - 주제: `agents.changed { rev, id }`, `agents.attention { id, state, message }`
  - CLI `devbox agent event --from <claude|codex|setup|exit> [--ok|--failed] [--code N] [payload]`. 환경 변수 `DEVBOX_AGENT_ID`가 없으면 아무것도 하지 않고 0으로 끝난다.
  - `start.rs::render_start_script(StartPlan) -> String`
  - `hooks.rs::claude_settings(devbox_bin) -> String`(JSON)
  - `hooks.rs::parse(source, payload, code) -> Vec<AgentEvent>`
  - 데몬이 시작할 때 `reconcile()`: `running·waiting·idle·preparing`인데 tmux 세션이 없으면 `SessionLost`

- [ ] **Step 1: 실패하는 테스트 — hook 해석**

```rust
// crates/agents/src/hooks.rs 끝
#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AgentEvent as E;

    #[test]
    fn parses_claude_hook_payloads() {
        assert_eq!(parse("claude", r#"{"hook_event_name":"SessionStart","session_id":"abc"}"#, None), vec![E::SessionStarted { session_id: "abc".into() }]);
        assert_eq!(parse("claude", r#"{"hook_event_name":"UserPromptSubmit","session_id":"abc"}"#, None), vec![E::PromptSubmitted]);
        assert_eq!(
            parse("claude", r#"{"hook_event_name":"Notification","notification_type":"permission_prompt","message":"Claude needs your permission to use Bash"}"#, None),
            vec![E::NeedsInput { message: "Claude needs your permission to use Bash".into() }]
        );
        assert_eq!(parse("claude", r#"{"hook_event_name":"Notification","notification_type":"idle_prompt","message":"waiting"}"#, None), vec![E::TurnComplete { message: None }]);
        assert_eq!(parse("claude", r#"{"hook_event_name":"Stop","last_assistant_message":"done"}"#, None), vec![E::TurnComplete { message: Some("done".into()) }]);
        assert!(parse("claude", "not json", None).is_empty());
    }

    #[test]
    fn parses_codex_notify_and_lifecycle_sources() {
        assert_eq!(parse("codex", r#"{"type":"agent-turn-complete","last-assistant-message":"ok"}"#, None), vec![E::TurnComplete { message: Some("ok".into()) }]);
        assert_eq!(parse("setup", "", Some(0)), vec![E::SetupOk]);
        assert_eq!(parse("setup", "", Some(3)), vec![E::SetupFailed { code: 3 }]);
        assert_eq!(parse("exit", "", Some(0)), vec![E::Exited { code: 0 }]);
    }

    #[test]
    fn claude_settings_route_every_hook_through_devbox() {
        let s: serde_json::Value = serde_json::from_str(&claude_settings("/home/u/.local/share/devbox/bin/devbox")).unwrap();
        for ev in ["SessionStart", "UserPromptSubmit", "Notification", "Stop"] {
            let cmd = s["hooks"][ev][0]["hooks"][0]["command"].as_str().unwrap();
            assert_eq!(cmd, "'/home/u/.local/share/devbox/bin/devbox' agent event --from claude");
        }
    }
}
```

- [ ] **Step 2: 실패하는 테스트 — 시작 스크립트**

```rust
// crates/agents/src/start.rs 끝
#[cfg(test)]
mod tests {
    use super::*;

    fn plan(hooks: crate::profile::Hooks) -> StartPlan {
        StartPlan {
            devbox: "/b/devbox".into(),
            instance: "prod".into(),
            agent_id: "a1".into(),
            workdir: "/w/x y".into(),
            setup: vec!["pnpm install".into()],
            tool_command: "claude".into(),
            hooks,
            claude_settings: Some("/d/agents/a1/claude.json".into()),
            prompt_file: Some("/d/agents/a1/prompt.md".into()),
            resume_session: None,
            use_scope: true,
        }
    }

    #[test]
    fn claude_script_runs_setup_then_tool_in_a_scope_with_injected_settings() {
        let s = render_start_script(&plan(crate::profile::Hooks::Claude));
        assert!(s.contains("cd '/w/x y'"));
        assert!(s.contains("pnpm install"));
        assert!(s.contains("'/b/devbox' agent event --from setup --code"));
        assert!(s.contains("systemd-run --user --scope --quiet --unit 'devbox-prod-agent-a1' --"));
        assert!(s.contains("claude --settings '/d/agents/a1/claude.json' \"$(cat '/d/agents/a1/prompt.md')\""));
        assert!(s.contains("'/b/devbox' agent event --from exit --code \"$code\""));
        assert!(s.trim_end().ends_with("exec bash -l"));
    }

    #[test]
    fn codex_injects_notify_and_resume_uses_the_session() {
        let mut p = plan(crate::profile::Hooks::Codex);
        p.tool_command = "codex".into();
        p.claude_settings = None;
        p.resume_session = Some("s-9".into());
        let s = render_start_script(&p);
        assert!(s.contains(r#"codex -c 'notify=["/b/devbox","agent","event","--from","codex"]' resume 's-9'"#));
    }

    #[test]
    fn tests_can_run_without_systemd_scope() {
        let mut p = plan(crate::profile::Hooks::None);
        p.use_scope = false;
        assert!(!render_start_script(&p).contains("systemd-run"));
    }
}
```

- [ ] **Step 3: 실패 확인** — `cargo test -p devbox-agents hooks start`

- [ ] **Step 4: 구현 — hooks.rs·start.rs**

```rust
// crates/agents/src/hooks.rs (테스트 위)
use crate::plan::sh_quote;
use crate::state::AgentEvent;

pub fn claude_settings(devbox: &str) -> String {
    let cmd = format!("{} agent event --from claude", sh_quote(devbox));
    let hook = serde_json::json!([{ "matcher": "", "hooks": [{ "type": "command", "command": cmd }] }]);
    serde_json::json!({ "hooks": { "SessionStart": hook, "UserPromptSubmit": hook, "Notification": hook, "Stop": hook } }).to_string()
}

pub fn codex_notify_arg(devbox: &str) -> String {
    format!("notify=[\"{devbox}\",\"agent\",\"event\",\"--from\",\"codex\"]")
}

pub fn parse(source: &str, payload: &str, code: Option<i32>) -> Vec<AgentEvent> {
    match source {
        "setup" => vec![if code == Some(0) { AgentEvent::SetupOk } else { AgentEvent::SetupFailed { code: code.unwrap_or(-1) } }],
        "exit" => vec![AgentEvent::Exited { code: code.unwrap_or(-1) }],
        "claude" => {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else { return vec![] };
            let text = |k: &str| v.get(k).and_then(|x| x.as_str()).map(String::from);
            match v.get("hook_event_name").and_then(|x| x.as_str()) {
                Some("SessionStart") => text("session_id").map(|session_id| AgentEvent::SessionStarted { session_id }).into_iter().collect(),
                Some("UserPromptSubmit") => vec![AgentEvent::PromptSubmitted],
                Some("Notification") => match text("notification_type").as_deref() {
                    Some("idle_prompt") => vec![AgentEvent::TurnComplete { message: None }],
                    _ => vec![AgentEvent::NeedsInput { message: text("message").unwrap_or_default() }],
                },
                Some("Stop") => vec![AgentEvent::TurnComplete { message: text("last_assistant_message") }],
                _ => vec![],
            }
        }
        "codex" => {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else { return vec![] };
            if v.get("type").and_then(|x| x.as_str()) == Some("agent-turn-complete") {
                vec![AgentEvent::TurnComplete { message: v.get("last-assistant-message").and_then(|x| x.as_str()).map(String::from) }]
            } else {
                vec![]
            }
        }
        _ => vec![],
    }
}
```

hook 입력은 외부 JSON이라 `serde_json::Value`로 읽는다. 데몬 안에서만 쓰고 RPC 경계 타입은 아니다.

```rust
// crates/agents/src/start.rs (테스트 위)
use crate::hooks::codex_notify_arg;
use crate::plan::sh_quote;
use crate::profile::Hooks;

pub struct StartPlan {
    pub devbox: String,
    pub instance: String,
    pub agent_id: String,
    pub workdir: String,
    pub setup: Vec<String>,
    pub tool_command: String,
    pub hooks: Hooks,
    pub claude_settings: Option<String>,
    pub prompt_file: Option<String>,
    pub resume_session: Option<String>,
    pub use_scope: bool,
}

pub fn render_start_script(p: &StartPlan) -> String {
    let dv = sh_quote(&p.devbox);
    let mut s = String::from("#!/usr/bin/env bash\n# devbox 에이전트 시작 스크립트(자동 생성)\n");
    s.push_str(&format!("cd {} || exit 1\n", sh_quote(&p.workdir)));
    if p.setup.is_empty() {
        s.push_str(&format!("{dv} agent event --from setup --code 0\n"));
    } else {
        s.push_str("( set -e\n");
        for c in &p.setup {
            s.push_str(&format!("  {c}\n")); // 프로젝트 파일(devbox.toml)의 명령은 사용자가 쓴 셸 명령 그대로 실행한다
        }
        s.push_str(")\n");
        s.push_str(&format!("setup=$?\n{dv} agent event --from setup --code \"$setup\"\n"));
        s.push_str("if [ \"$setup\" -ne 0 ]; then echo \"[devbox] 준비 명령이 실패했습니다($setup).\"; exec bash -l; fi\n");
    }
    let mut tool = p.tool_command.clone();
    match p.hooks {
        Hooks::Claude => {
            if let Some(f) = &p.claude_settings {
                tool.push_str(&format!(" --settings {}", sh_quote(f)));
            }
        }
        Hooks::Codex => tool.push_str(&format!(" -c {}", sh_quote(&codex_notify_arg(&p.devbox)))),
        Hooks::None => {}
    }
    match (&p.resume_session, p.hooks) {
        (Some(id), Hooks::Claude) => tool.push_str(&format!(" --resume {}", sh_quote(id))),
        (Some(id), Hooks::Codex) => tool.push_str(&format!(" resume {}", sh_quote(id))),
        // 사용자 정의 도구(Hooks::None)는 지시문을 DEVBOX_PROMPT_FILE 환경 변수로만 받는다.
        (None, Hooks::Claude | Hooks::Codex) => {
            if let Some(f) = &p.prompt_file {
                tool.push_str(&format!(" \"$(cat {})\"", sh_quote(f)));
            }
        }
        _ => {}
    }
    if p.use_scope {
        let unit = format!("devbox-{}-agent-{}", p.instance, p.agent_id);
        s.push_str(&format!("systemd-run --user --scope --quiet --unit {} -- {tool}\n", sh_quote(&unit)));
    } else {
        s.push_str(&format!("{tool}\n"));
    }
    s.push_str(&format!("code=$?\n{dv} agent event --from exit --code \"$code\"\necho \"[devbox] 에이전트가 끝났습니다(종료 코드 $code). 이 셸은 그대로 쓸 수 있습니다.\"\nexec bash -l\n"));
    s
}
```

사용자 정의 도구(`Hooks::None`)는 지시문을 `DEVBOX_PROMPT_FILE` 환경 변수로만 받는다(위 `match`의 마지막 분기).

- [ ] **Step 5: 구현 — 서비스(생성·이벤트·조정)**

`crates/agents/src/service.rs`의 `AgentsService { db, hub, paths, projects: Arc<ProjectsService>, terminal: Arc<TerminalService>, tmux: Tmux, profiles: Vec<ToolProfile>, devbox_bin: String, use_scope: bool }`.

`use_scope`는 `!instance.is_test() && std::env::var("DEVBOX_NO_SCOPE").is_err()`다.

```rust
// crates/agents/src/service.rs (핵심 흐름)
pub async fn create(&self, p: CreateAgentTask) -> R<AgentTask> {
    let title = crate::plan::checked_title(&p.title)?.to_owned();
    let project = self.projects.get(&p.project_id).await.map_err(|_| AgentsError::ProjectNotFound { id: p.project_id.clone() })?
        .ok_or_else(|| AgentsError::ProjectNotFound { id: p.project_id.clone() })?;
    let profile = crate::profile::resolve(&self.profiles, &p.profile).ok_or_else(|| AgentsError::ProfileNotFound { id: p.profile.clone() })?;
    let repo = std::path::PathBuf::from(&project.path);
    let base = match p.base_branch.clone() {
        Some(b) => b,
        None => devbox_git::current_branch(&repo).await?.unwrap_or_else(|| "main".into()),
    };
    let now = now_ms();
    let id = format!("a{}", &uuid::Uuid::new_v4().simple().to_string()[..10]);
    let (branch, worktree) = if p.worktree {
        let taken: std::collections::HashSet<String> = devbox_git::branches(&repo).await?.into_iter().filter_map(|b| b.strip_prefix("agent/").map(String::from)).collect();
        let slug = crate::plan::unique_slug(&crate::plan::slug(&title, now as u64), &taken)?;
        (Some(crate::plan::branch_for(&slug)), Some(crate::plan::worktree_dir(&repo, &slug)))
    } else {
        (None, None)
    };
    let task = AgentTask {
        id: id.clone(), project_id: project.id.clone(), title, profile: profile.id.clone(), base_branch: base,
        branch, worktree_path: worktree.as_ref().map(|w| w.display().to_string()),
        workdir: worktree.as_ref().map(|w| w.display().to_string()).unwrap_or_else(|| project.path.clone()),
        session_id: None, terminal_id: None, state: AgentState::Preparing, reason: None, attention: None,
        port: self.allocate_port().await?, prompt: p.prompt, follow_ups: vec![], tokens: None, pr_url: None,
        closed_how: None, created_ms: now, updated_ms: now, closed_ms: None,
    };
    self.save(&task).await?;
    let svc = self.clone();
    let t2 = task.clone();
    tokio::spawn(async move {
        if let Err(reason) = svc.prepare_and_start(t2.clone(), profile, project, worktree).await {
            let _ = svc.fail(&t2.id, reason).await;
        }
    });
    Ok(task)
}
```

`prepare_and_start` 순서:
1. worktree가 있으면 `worktree_add(repo, dir, branch, base)`.
2. 프로젝트 설정 `[agent] copy`의 각 경로를 프로젝트에서 worktree로 복사한다(있을 때만, 디렉터리 밖으로 나가는 경로 `..`는 거부).
3. `<data>/agents/<id>/`를 만들고 `prompt.md`·`start.sh`(`render_start_script`, 0700)를 쓴다. Claude hook 설정은 모든 작업이 같으므로(작업 구분은 `DEVBOX_AGENT_ID` 환경 변수) 서비스가 시작할 때 `<data>/agent-hooks/claude.json`에 한 번 쓰고, `StartPlan.claude_settings`에 그 경로를 넣는다(01-design §9.2).
4. `terminal.create_internal(kind=agent, project_id, workdir, title, command="bash <start.sh>", env=[DEVBOX_AGENT_ID, DEVBOX_INSTANCE, DEVBOX_PORT, DEVBOX_PROMPT_FILE])`.
5. `terminal_id`를 기록하고 저장·발행한다.

상태는 시작 스크립트가 보내는 `setup` 이벤트로 `running`이 된다.

`event(p: AgentEventParams)`:
1. 작업을 읽는다.
2. `hooks::parse(source, payload, code)`의 각 이벤트를 `state::next`에 적용한다. `SessionStarted`는 `session_id`를 기록한다.
3. 바뀌었으면 `attention`을 채운다(`waiting`이면 hook 메시지, `idle`이면 "다음 지시를 기다립니다", `failed`면 사유).
4. 저장 → `agents.changed`를 발행한다. 주의 상태로 들어갔으면 `agents.attention`도 발행한다.

`reconcile()`:
- 데몬 시작 시 호출한다.
- tmux 목록과 대조해 세션이 없는 살아 있는 작업에 `SessionLost`를 적용한다. `reason`은 `"session_lost"`다.

포트: 열린 작업이 쓰는 `port`를 모은 뒤, 20000부터 10씩 올리며 처음 빈 값을 쓴다. 이때 그 10개 포트 중 하나라도 이미 다른 프로그램이 쓰고 있으면(`std::net::TcpListener::bind(("127.0.0.1", p))`가 실패) 그 묶음도 건너뛴다. 시험 `allocate_port_skips_blocks_with_a_listening_port`: 20003에서 리스너를 연 뒤 새 작업의 포트가 20010.

- [ ] **Step 6: `devbox agent event` CLI**

```rust
// crates/cli/src/main.rs 의 Cmd에 추가
/// 에이전트 hook·시작 스크립트가 상태를 알린다.
Agent {
    #[command(subcommand)]
    cmd: AgentCmd,
},

#[derive(Subcommand)]
enum AgentCmd {
    Event {
        #[arg(long)]
        from: String,
        #[arg(long)]
        code: Option<i32>,
        /// Codex notify는 JSON을 마지막 인자로 넘긴다.
        payload: Option<String>,
    },
}
```

```rust
// crates/cli/src/agent_event.rs
use std::io::Read;

pub async fn run(from: String, code: Option<i32>, payload: Option<String>) -> anyhow::Result<()> {
    let Ok(agent_id) = std::env::var("DEVBOX_AGENT_ID") else { return Ok(()) };
    let mut body = payload.unwrap_or_default();
    if from == "claude" {
        std::io::stdin().read_to_string(&mut body)?;
    }
    let paths = crate::paths()?;
    let c = crate::client::Client::connect(&paths.socket, devbox_protocol::message::ClientKind::Cli, paths.instance.as_str()).await?;
    let params = serde_json::json!({ "agentId": agent_id, "source": from, "payload": body, "code": code });
    // hook은 실패해도 도구를 막지 않는다.
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), c.raw_call("agents.event", params, None)).await;
    Ok(())
}
```

- [ ] **Step 7: 실패하는 통합 테스트(가짜 도구)**

```rust
// crates/cli/tests/agents.rs
mod support;
use support::TestDaemon;
use std::time::Duration;

/// 가짜 도구: SessionStart → UserPromptSubmit → Notification(권한) → (입력 대기) → Stop 을 devbox로 보낸 뒤 대기.
const FAKE: &str = r#"#!/usr/bin/env bash
send() { printf '%s' "$1" | "$DEVBOX_BIN" agent event --from claude; }
send '{"hook_event_name":"SessionStart","session_id":"sess-1"}'
send '{"hook_event_name":"UserPromptSubmit"}'
send '{"hook_event_name":"Notification","notification_type":"permission_prompt","message":"needs Bash"}'
read -r answer
send '{"hook_event_name":"Stop","last_assistant_message":"done"}'
sleep 600
"#;

#[tokio::test]
async fn agent_task_goes_from_preparing_to_waiting_to_idle_via_hooks() {
    let d = TestDaemon::start_with_env(&[("DEVBOX_NO_SCOPE", "1")]);
    let tool = d.home.path().join("fake-tool.sh"); // 테스트마다 자기 HOME 안에 둔다(병렬 실행 충돌 방지)
    std::fs::write(&tool, FAKE).unwrap();
    d.write_config(&format!("[[agents.profiles]]\nid = \"fake\"\nlabel = \"Fake\"\ncommand = \"bash {}\"\nhooks = \"none\"\n", tool.display()));
    let repo = d.git_repo("projects/app");
    let c = d.client().await;
    let p: serde_json::Value = serde_json::from_str(&c.raw_call("projects.add", serde_json::json!({ "path": repo }), None).await.unwrap()).unwrap();
    let task: serde_json::Value = serde_json::from_str(&c.raw_call("agents.create", serde_json::json!({
        "projectId": p["id"], "title": "Fix login", "profile": "fake", "worktree": true, "prompt": "fix it"
    }), None).await.unwrap()).unwrap();
    assert_eq!(task["state"], "preparing");
    let id = task["id"].as_str().unwrap();

    let waiting = d.wait_for(&c, id, "waiting", Duration::from_secs(10)).await;
    assert_eq!(waiting["attention"]["message"], "needs Bash");
    assert_eq!(waiting["sessionId"], "sess-1");
    assert!(std::path::Path::new(waiting["worktreePath"].as_str().unwrap()).ends_with("app-agents/fix-login"));

    c.raw_call("agents.send", serde_json::json!({ "id": id, "text": "yes" }), None).await.unwrap();
    let idle = d.wait_for(&c, id, "idle", Duration::from_secs(10)).await;
    assert_eq!(idle["followUps"][0]["text"], "yes");
}
```

- 이 시험은 Task 9의 `agents.send`도 쓴다. Task 9를 마친 뒤 통과시킨다. Task 8에서는 `waiting`까지만 확인하는 형태로 먼저 둔다.
- `TestDaemon`에 다음을 추가한다.
  - `start_with_env`. `start`·`start_with_env` 모두 데몬을 띄우기 전에 임시 HOME에 `.gitconfig`를 쓴다: `[user] name = devbox-test`, `email = test@devbox.invalid`, `[init] defaultBranch = main`. 임시 HOME이라 사용자의 `~/.gitconfig`가 보이지 않으므로, 이것이 없으면 데몬의 squash 병합·rebase 커밋이 "Please tell me who you are"로 실패한다(로컬·CI 모두)
  - `write_config(text)`(`~/.config/devbox/config.toml`)
  - `git_repo(rel)`(`git init -q -b main` + 첫 커밋. 기준 브랜치 이름을 시험이 `main`으로 가정한다)
  - `wait_for(client, id, state, timeout)`(`agents.get` 폴링)
- 가짜 도구는 `hooks = "none"` 프로필로 실행하지만, 스스로 `--from claude` 이벤트를 보내 Claude hook을 흉내 낸다.
- 시작 스크립트는 `DEVBOX_BIN` 환경 변수도 넣는다(`create_internal`의 env에 추가).

- [ ] **Step 8: 통과·커밋**

```bash
cargo test -p devbox-agents && cargo test -p devbox-cli --test agents
git add crates xtask app/src/rpc/gen && git commit -m "feat(agents): create agent tasks with worktrees, start scripts and hook-driven state"
```

---

### Task 9: 에이전트 작업 행동 (지시·재개·기준 반영·병합·PR·폐기·정리·테스트 실행)

**Files:**
- Create: `crates/agents/src/actions.rs`
- Test: `crates/cli/tests/agents_actions.rs`

**Interfaces:**
- Consumes: Task 6 `mutate::*`, Task 3 `Tmux::paste`, Task 8 서비스
- Produces(메서드):
  - `agents.send{ id, text }`(mutation): 세션이 있으면 bracketed paste + Enter. 기록을 남기고 `UserInstructed`를 적용한다. 세션이 없으면 `agents.session_missing`(화면이 [재개]를 권한다).
  - `agents.resume{ id }`(mutation): 새 시작 스크립트(`resume_session`)로 tmux 세션을 다시 만든다. Codex 세션 ID가 없으면 `~/.codex/sessions`에서 `session_meta.cwd == workdir`인 가장 최근 파일의 ID를 쓴다.
  - `agents.review{ id }`(mutation) → `review`
  - `agents.rebase{ id }`(mutation) → `{ result: "done" | "conflict", files }`. 충돌 시 rebase를 진행 중으로 둔다.
  - `agents.rebase_abort{ id }`(mutation)
  - `agents.merge{ id, strategy: "merge"|"squash"|"rebase", message?, expect: ReviewExpect }`(mutation, always):
    - 에이전트 worktree가 dirty면 `agents.worktree_dirty`
    - `expect`의 `headOid`·`baseOid`가 지금 값과 다르면 `agents.state_changed`(Step 3b, IR-11)
    - 성공하면 `closed(merged)`이고, 설정에 따라 정리한다.
  - `agents.pr{ id, title?, body?, expect: ReviewExpect }`(mutation, always) → `closed(pullRequest)`, `prUrl`
  - `agents.discard_check{ id }`(query) → `{ unmergedCommits: u32, dirtyFiles: u32, headOid: String }`
  - `agents.discard{ id, expect: ReviewExpect }`(mutation, always): `headOid`·`unmergedCommits`가 확인 때와 다르면 `agents.state_changed`. 같으면 세션 종료, scope 정지, teardown, worktree 강제 제거, 브랜치 삭제 → `closed(discarded)`
  - `ReviewExpect { headOid: String, baseOid?: String, unmergedCommits?: u32 }`: 사용자가 확인 대화상자에서 본 상태(01-design §5.2). 병합은 `baseOid`, 폐기는 `unmergedCommits`가 필수다.
  - `agents.test{ id }`(mutation): `[agent] test` 명령을 같은 작업 폴더의 새 터미널 세션에서 실행한다. 반환은 터미널 ID다(S2가 runtime으로 바꾼다).
  - `agents.cleanup_candidates{ projectId }`(query) → `{ worktrees: [path], branches: [name] }`
  - `agents.cleanup{ projectId, worktrees, branches }`(mutation, always)
  - 검토 화면용 조회(모두 query, 작업 폴더가 없으면 프로젝트 체크아웃 기준):
    - `agents.changes{ id }` → `{ base, ahead, behind, headOid, baseOid, files: FileChange[] }`(`changed_files(workdir, base)` + `ahead_behind` + `rev_parse`). 검토 화면은 이 `headOid`·`baseOid`를 병합·PR 확인의 `expect`로 쓴다
    - `agents.patch{ id, path }` → `{ patch }`(`file_patch`, 1 MiB 넘으면 앞부분만 + `truncated: true`)
    - `agents.commits{ id }` → `{ items: Commit[] }`(`log_range(workdir, base)`)
  - 닫힐 때 기록 고정: 토큰(Task 10의 `usage`), 커밋 목록 수, `closedMs`

- [ ] **Step 1: 실패하는 통합 테스트**

```rust
// crates/cli/tests/agents_actions.rs
mod support;
use support::TestDaemon;
use std::time::Duration;

const COMMITTING_TOOL: &str = r#"#!/usr/bin/env bash
echo "change" > agent.txt && git add agent.txt && git -c user.name=t -c user.email=t@t commit -qm "agent change"
printf '%s' '{"hook_event_name":"Stop","last_assistant_message":"done"}' | "$DEVBOX_BIN" agent event --from claude
sleep 600
"#;

#[tokio::test]
async fn squash_merge_closes_the_task_and_cleans_up() {
    let d = TestDaemon::start_with_env(&[("DEVBOX_NO_SCOPE", "1")]);
    let tool = d.home.path().join("commit-tool.sh");
    std::fs::write(&tool, COMMITTING_TOOL).unwrap();
    d.write_config(&format!("[agents]\ncleanup_after_close = true\n[[agents.profiles]]\nid = \"commit\"\nlabel = \"C\"\ncommand = \"bash {}\"\nhooks = \"none\"\n", tool.display()));
    let repo = d.git_repo("projects/app");
    let c = d.client().await;
    let p = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &p, "commit", true).await;
    let idle = d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    let wt = idle["worktreePath"].as_str().unwrap().to_owned();

    let check: serde_json::Value = serde_json::from_str(&c.raw_call("agents.discard_check", serde_json::json!({ "id": id }), None).await.unwrap()).unwrap();
    assert_eq!(check["unmergedCommits"], 1);

    c.raw_call("agents.merge", serde_json::json!({ "id": id, "strategy": "squash", "message": "agent: change" }), None).await.unwrap();
    let closed = d.wait_for(&c, &id, "closed", Duration::from_secs(10)).await;
    assert_eq!(closed["closedHow"], "merged");
    assert!(std::path::Path::new(&repo).join("agent.txt").exists());
    assert!(!std::path::Path::new(&wt).exists(), "worktree removed after close");
}

#[tokio::test]
async fn merge_refuses_a_dirty_base_and_discard_removes_everything() {
    let d = TestDaemon::start_with_env(&[("DEVBOX_NO_SCOPE", "1")]);
    let tool = d.home.path().join("commit-tool.sh");
    std::fs::write(&tool, COMMITTING_TOOL).unwrap();
    d.write_config(&format!("[[agents.profiles]]\nid = \"commit\"\nlabel = \"C\"\ncommand = \"bash {}\"\nhooks = \"none\"\n", tool.display()));
    let repo = d.git_repo("projects/app2");
    let c = d.client().await;
    let p = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &p, "commit", true).await;
    let idle = d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    std::fs::write(std::path::Path::new(&repo).join("dirty.txt"), "x").unwrap();
    let e = c.raw_call("agents.merge", serde_json::json!({ "id": id, "strategy": "merge" }), None).await.unwrap_err();
    assert!(format!("{e:?}").contains("agents.base_dirty"));

    c.raw_call("agents.discard", serde_json::json!({ "id": id }), None).await.unwrap();
    let closed = d.wait_for(&c, &id, "closed", Duration::from_secs(10)).await;
    assert_eq!(closed["closedHow"], "discarded");
    assert!(!std::path::Path::new(idle["worktreePath"].as_str().unwrap()).exists());
    let branches = std::process::Command::new("git").args(["-C", &repo, "branch", "--list", "agent/*"]).output().unwrap();
    assert!(String::from_utf8_lossy(&branches.stdout).trim().is_empty());
}

#[tokio::test]
async fn lost_sessions_become_failed_and_can_be_resumed() {
    let mut d = TestDaemon::start_with_env(&[("DEVBOX_NO_SCOPE", "1")]);
    let tool = d.home.path().join("commit-tool.sh");
    std::fs::write(&tool, COMMITTING_TOOL).unwrap();
    d.write_config(&format!("[[agents.profiles]]\nid = \"commit\"\nlabel = \"C\"\ncommand = \"bash {}\"\nhooks = \"none\"\n", tool.display()));
    let repo = d.git_repo("projects/app3");
    let c = d.client().await;
    let p = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &p, "commit", true).await;
    d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    d.kill_tmux_server(); // PC 재부팅 흉내
    d.restart();
    let c = d.client().await;
    let failed = d.wait_for(&c, &id, "failed", Duration::from_secs(10)).await;
    assert_eq!(failed["reason"], "session_lost");
    c.raw_call("agents.resume", serde_json::json!({ "id": id }), None).await.unwrap();
    d.wait_for(&c, &id, "running", Duration::from_secs(10)).await;
}
```

`TestDaemon` 보조 함수 `add_project`·`create_agent`·`kill_tmux_server`(`tmux -L devbox-<instance> kill-server`)를 추가한다.

- [ ] **Step 2: 실패 확인** — `cargo test -p devbox-cli --test agents_actions`

- [ ] **Step 3: 구현 요점(actions.rs)**

```rust
// crates/agents/src/actions.rs (요점)
impl AgentsService {
    pub async fn send(&self, p: SendToAgent) -> R<AgentTask> {
        let mut t = self.get_task(&p.id).await?;
        let session = t.terminal_id.clone().ok_or(AgentsError::SessionMissing)?;
        if !self.tmux.list().await.map_err(internal)?.iter().any(|s| s.name == session) {
            return Err(AgentsError::SessionMissing.into());
        }
        self.tmux.paste(&session, &p.text, true).await.map_err(internal)?;
        t.follow_ups.push(FollowUp { at_ms: now_ms(), text: p.text });
        self.apply(t, &AgentEvent::UserInstructed).await
    }

    pub async fn merge(&self, p: MergeAgent) -> R<AgentTask> {
        let t = self.get_task(&p.id).await?;
        let (Some(branch), Some(wt)) = (t.branch.clone(), t.worktree_path.clone()) else {
            return Err(AgentsError::BadState { state: "no-worktree".into() }.into());
        };
        let wt = std::path::PathBuf::from(wt);
        let st = devbox_git::status(&wt).await?;
        if !st.clean {
            return Err(AgentsError::WorktreeDirty { files: dirty_files(&wt).await? }.into());
        }
        let project = self.project_of(&t).await?;
        let strategy = match p.strategy {
            MergeKind::Merge => devbox_git::mutate::MergeStrategy::Merge,
            MergeKind::Squash => devbox_git::mutate::MergeStrategy::Squash { message: p.message.unwrap_or_else(|| format!("{} (agent)", t.title)) },
            MergeKind::Rebase => devbox_git::mutate::MergeStrategy::Rebase { worktree: wt.clone() },
        };
        devbox_git::mutate::merge_into(std::path::Path::new(&project.path), &t.base_branch, &branch, strategy).await?;
        self.close(t, CloseHow::Merged).await
    }

    /// 닫기: 기록 고정 → 상태 → (설정 시) 정리.
    async fn close(&self, mut t: AgentTask, how: CloseHow) -> R<AgentTask> {
        t.tokens = None; // Task 10이 crate::usage::tokens_for(...)로 바꾼다
        t.closed_how = Some(how);
        t.closed_ms = Some(now_ms());
        let t = self.apply(t, &AgentEvent::Closed { how }).await?;
        if self.cleanup_after_close || how == CloseHow::Discarded {
            self.teardown(&t, how == CloseHow::Discarded).await?;
        }
        Ok(t)
    }

    /// 세션 종료 → scope 정지 → teardown 명령(60초 상한) → worktree 제거 → (폐기면) 브랜치 삭제.
    async fn teardown(&self, t: &AgentTask, delete_branch: bool) -> R<()> {
        if let Some(s) = &t.terminal_id {
            let _ = self.terminal.close_internal(s).await;
        }
        let unit = format!("devbox-{}-agent-{}.scope", self.paths.instance.as_str(), t.id);
        let _ = tokio::process::Command::new("systemctl").args(["--user", "stop", &unit]).status().await;
        let project = self.project_of(t).await?;
        let repo = std::path::PathBuf::from(&project.path);
        if let Some(wt) = &t.worktree_path {
            for cmd in self.project_config(&project).await.agent.teardown {
                let _ = tokio::time::timeout(std::time::Duration::from_secs(60), tokio::process::Command::new("bash").args(["-lc", &cmd]).current_dir(wt).status()).await;
            }
            devbox_git::mutate::worktree_remove(&repo, std::path::Path::new(wt), true).await?;
        }
        if delete_branch {
            if let Some(b) = &t.branch {
                let _ = devbox_git::mutate::delete_branch(&repo, b, true).await;
            }
        }
        Ok(())
    }
}
```

나머지 행동도 같은 모양으로 만든다. 각 메서드는 `method!`로 선언하고 라우터에 등록한다(확인 정책: `merge`·`pr`·`discard`·`cleanup`은 `always`).

- [ ] **Step 3b: 확인한 상태로만 병합·PR·폐기 (IR-11, AR-F01)**

확인 대화상자가 보여 준 상태와 실행 순간의 상태가 다르면 실행하지 않는다. v0.9.0은 승인 뒤 외부 checkout이 일어나면 승인하지 않은 브랜치를 push할 수 있었다(00-roadmap §7).

1. 위 Step 1 시험 파일에 보조 함수와 시험을 더하고, 기존 두 시험의 호출에 `expect`를 넣는다.

```rust
// crates/cli/tests/agents_actions.rs 에 추가
async fn review_expect(c: &devbox_cli::client::Client, id: &str) -> serde_json::Value {
    let ch: serde_json::Value =
        serde_json::from_str(&c.raw_call("agents.changes", serde_json::json!({ "id": id }), None).await.unwrap()).unwrap();
    serde_json::json!({ "headOid": ch["headOid"], "baseOid": ch["baseOid"] })
}

async fn discard_expect(c: &devbox_cli::client::Client, id: &str) -> serde_json::Value {
    let ch: serde_json::Value =
        serde_json::from_str(&c.raw_call("agents.discard_check", serde_json::json!({ "id": id }), None).await.unwrap()).unwrap();
    serde_json::json!({ "headOid": ch["headOid"], "unmergedCommits": ch["unmergedCommits"] })
}

fn rev(dir: &str, r: &str) -> String {
    let out = std::process::Command::new("git").args(["-C", dir, "rev-parse", r]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[tokio::test]
async fn merge_after_the_reviewed_head_moved_is_refused_and_changes_nothing() {
    let d = TestDaemon::start_with_env(&[("DEVBOX_NO_SCOPE", "1")]);
    let tool = d.home.path().join("commit-tool.sh");
    std::fs::write(&tool, COMMITTING_TOOL).unwrap();
    d.write_config(&format!("[[agents.profiles]]\nid = \"commit\"\nlabel = \"C\"\ncommand = \"bash {}\"\nhooks = \"none\"\n", tool.display()));
    let repo = d.git_repo("projects/app4");
    let c = d.client().await;
    let p = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &p, "commit", true).await;
    let idle = d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    let reviewed = review_expect(&c, &id).await;

    // 확인 뒤 작업 폴더에 커밋이 하나 더 생긴다.
    let wt = idle["worktreePath"].as_str().unwrap();
    let ok = std::process::Command::new("git")
        .args(["-C", wt, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "--allow-empty", "-qm", "late"])
        .status().unwrap();
    assert!(ok.success());
    let base_before = rev(&repo, "HEAD");

    let e = c.raw_call("agents.merge", serde_json::json!({ "id": id, "strategy": "squash", "message": "m", "expect": reviewed }), None)
        .await.unwrap_err();
    assert!(format!("{e:?}").contains("agents.state_changed"));
    assert_eq!(rev(&repo, "HEAD"), base_before, "기준 브랜치가 움직이면 안 된다");

    // 다시 본 상태로는 병합된다.
    let fresh = review_expect(&c, &id).await;
    c.raw_call("agents.merge", serde_json::json!({ "id": id, "strategy": "squash", "message": "m", "expect": fresh }), None)
        .await.unwrap();
}
```

기존 시험의 호출을 다음처럼 바꾼다.

```rust
// squash_merge_closes_the_task_and_cleans_up
let reviewed = review_expect(&c, &id).await;
c.raw_call("agents.merge", serde_json::json!({ "id": id, "strategy": "squash", "message": "agent: change", "expect": reviewed }), None).await.unwrap();

// merge_refuses_a_dirty_base_and_discard_removes_everything
let reviewed = review_expect(&c, &id).await;
let e = c.raw_call("agents.merge", serde_json::json!({ "id": id, "strategy": "merge", "expect": reviewed }), None).await.unwrap_err();
// …
let gone = discard_expect(&c, &id).await;
c.raw_call("agents.discard", serde_json::json!({ "id": id, "expect": gone }), None).await.unwrap();
```

2. 실패 확인: `cargo test -p devbox-cli --test agents_actions merge_after_the_reviewed_head_moved` → `expect` 필드를 모르는 역직렬화 오류 또는 병합 성공으로 FAIL.

3. 구현.

```rust
// crates/git/src/lib.rs 에 추가
pub async fn rev_parse(dir: &std::path::Path, rev: &str) -> Result<String, GitError> {
    Ok(git(dir, &["rev-parse", "--verify", rev]).await?.trim().to_owned())
}
```

```rust
// crates/agents/src/model.rs 에 추가
#[derive(Debug, Clone, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ReviewExpect {
    pub head_oid: String,
    pub base_oid: Option<String>,
    pub unmerged_commits: Option<u32>,
}
// AgentsError(domain_error!)에 변형 추가:
//     StateChanged { head: String, base: String, unmerged: u32 } = "agents.state_changed",
// (domain_error! 규칙: 세부 필드 이름은 한 단어 소문자. 지금 상태의 HEAD·기준 OID·병합 안 된 커밋 수)
```

```rust
// crates/agents/src/actions.rs
impl AgentsService {
    /// 확인 대화상자가 보여 준 상태와 지금 상태를 비교한다. 다르면 아무것도 바꾸지 않고 지금 상태를 돌려준다.
    async fn check_expect(&self, t: &AgentTask, repo: &std::path::Path, branch: &str, e: &ReviewExpect) -> R<()> {
        let head = devbox_git::rev_parse(repo, branch).await?;
        let base = devbox_git::rev_parse(repo, &t.base_branch).await?;
        let unmerged = devbox_git::mutate::unmerged_count(repo, branch, &t.base_branch).await?;
        let same = e.head_oid == head
            && e.base_oid.as_deref().is_none_or(|b| b == base)
            && e.unmerged_commits.is_none_or(|n| n == unmerged);
        if same {
            Ok(())
        } else {
            Err(AgentsError::StateChanged { head, base, unmerged }.into())
        }
    }
}
```

`merge`는 worktree dirty 검사 뒤 `check_expect`를 부르고(`base_oid`가 없으면 `invalid_params`), `merge`·`squash` 전략에서는 브랜치 이름 대신 **확인한 `headOid`**를 `merge_into`의 원본으로 넘긴다. 확인 뒤 생긴 커밋이 섞이지 않는다. `pr`은 push 전에, `discard`는 teardown 전에 `check_expect`를 부른다(`discard`는 `unmerged_commits`가 없으면 `invalid_params`).

4. 통과 확인: `cargo test -p devbox-cli --test agents_actions` → 기존 3개 + 새 1개 통과.
- `rebase`: `rebase_onto`. 충돌이면 결과로 돌려주고 상태는 유지한다.
- `pr`: `push_and_pr`. 제목 기본값은 작업 제목, 본문 기본값은 초기 지시문 + 커밋 목록이다.
- `discard`: `close(Discarded)`.
- `cleanup_candidates`:
  - `<parent>/<name>-agents/*` 중 열린 작업의 `worktree_path`가 아닌 것
  - `merged_agent_branches(repo, base)` 중 열린 작업의 브랜치가 아닌 것

- [ ] **Step 4: 통과·커밋**

```bash
cargo test -p devbox-cli --test agents_actions && cargo test -p devbox-cli --test agents && cargo run -q -p xtask -- gen-ts
git add crates xtask app/src/rpc/gen && git commit -m "feat(agents): instruct, resume, rebase, merge, open PRs, discard and clean up agent tasks"
```

---

### Task 10: 출력 기반 대기 감지·자원·토큰

**Files:**
- Create: `crates/agents/src/watch.rs`, `crates/agents/src/usage.rs`, `crates/agents/src/resources.rs`
- Test: `crates/agents/src/usage.rs`(v0.9.0 `agent_usage.rs` 시험 이식), `crates/agents/src/watch.rs`(패턴 단위)

**Interfaces:**
- Produces:
  - `watch::spawn(service)`: 2초마다 동작한다.
    - 대상: `running`·`waiting` 중 프로필에 `waiting_patterns`가 있는 작업(Codex·사용자 정의)
    - `capture_tail(15)`이 패턴과 맞으면 `OutputWaiting`, `waiting(출처=출력)`인데 더는 맞지 않으면 `OutputResumed`를 적용한다.
    - hook으로 들어온 `waiting`은 건드리지 않는다(`attention.kind == "output"`일 때만 되돌림).
  - `usage::tokens_for(home, task, hooks: Hooks) -> Option<Tokens>`. Task 9 `close()`의 `t.tokens = None;` 줄을 이 함수 호출로 바꾼다:
    - Claude: `~/.claude/projects/<workdir 변환>/<sessionId>.jsonl`의 `assistant` 기록을 `message.id` 기준으로 중복 없이 합산
    - Codex: `~/.codex/sessions/**`에서 `session_meta.payload.cwd == workdir`인 파일의 마지막 `token_count.total_token_usage`
    - 파일 128MiB·줄 4MiB 상한(이식)
  - 메서드 `agents.usage{ id }`(query) → `{ tokens?, truncated }`
  - 메서드 `agents.git_stats{}`(query) → `{ items: [{ id, changedFiles, ahead, behind }] }`: 열린 작업마다 `changed_files`의 개수와 `ahead_behind`를 4개씩 동시에 계산한다. 목록 행의 "변경 n · 뒤처짐 m"에 쓴다. 화면은 `agents.changed` 이벤트와 10초 간격 폴링으로 갱신한다.
  - 메서드 `agents.resources{}`(query) → `{ items: [{ id, cpuPercent, memoryBytes }] }`
    - 살아 있는 작업의 scope를 `systemctl --user show -p CPUUsageNSec -p MemoryCurrent --value`로 읽는다.
    - CPU%는 이전 표본과의 차로 계산한다(서비스 메모리에 마지막 표본 보관).
    - 화면이 보일 때 3초 간격으로 부른다(이벤트 원천 없음, 01-design §5.5).

- [ ] **Step 1: 실패하는 테스트(토큰)**

```rust
// crates/agents/src/usage.rs 끝 (v0.9.0 crates/wsl-helper/src/agent_usage.rs 시험을 함께 옮긴다)
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn claude_sums_unique_assistant_messages() {
        let lines = r#"{"type":"assistant","message":{"id":"m1","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100,"cache_creation_input_tokens":7}}}
{"type":"assistant","message":{"id":"m1","usage":{"input_tokens":10,"output_tokens":5}}}
{"type":"user","message":{"id":"u"}}
{"type":"assistant","message":{"id":"m2","usage":{"input_tokens":1,"output_tokens":2}}}
"#;
        let t = claude_totals(Cursor::new(lines)).unwrap();
        assert_eq!((t.input, t.output, t.cache_read, t.cache_write), (11, 7, 100, 7));
    }

    #[test]
    fn codex_takes_the_last_total_for_the_matching_cwd() {
        let lines = r#"{"type":"session_meta","payload":{"cwd":"/w/app-agents/x","id":"s"}}
{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":5,"output_tokens":1,"cached_input_tokens":2}}}}
{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":9,"output_tokens":3,"cached_input_tokens":4}}}}
"#;
        let t = codex_session(Cursor::new(lines), std::path::Path::new("/w/app-agents/x")).unwrap();
        assert_eq!((t.input, t.output, t.cache_read), (9, 3, 4));
        assert!(codex_session(Cursor::new(lines), std::path::Path::new("/w/other")).is_none());
    }

    #[test]
    fn claude_project_dir_matches_claude_code_naming() {
        assert_eq!(claude_project_dir(std::path::Path::new("/h"), "/home/u/projects/app-agents/x"), std::path::PathBuf::from("/h/.claude/projects/-home-u-projects-app-agents-x"));
    }
}
```

- [ ] **Step 2: 구현**

`usage.rs`는 v0.9.0 `crates/wsl-helper/src/agent_usage.rs`의 다음 부분을 옮긴다.
- `records`(상한 있는 줄 읽기)
- `claude_project_dir`
- `claude_read`
- `codex_read`

반환 타입은 `crate::model::Tokens`로 바꾸고, 세션 수 집계는 뺀다. `tokens_for`는 프로필 hooks 종류로 Claude/Codex를 고르고, 파일을 찾지 못하면 `None`이다.

`watch.rs`:

```rust
pub fn matches_waiting(patterns: &[regex::Regex], tail: &str) -> bool {
    let last = tail.lines().rev().filter(|l| !l.trim().is_empty()).take(6).collect::<Vec<_>>().join("\n");
    patterns.iter().any(|p| p.is_match(&last))
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_the_last_non_empty_lines_count() {
        let p = vec![regex::Regex::new(r"(?i)approve").unwrap()];
        assert!(super::matches_waiting(&p, "work...\nApprove this edit? (y/n)\n\n"));
        assert!(!super::matches_waiting(&p, "approve\n1\n2\n3\n4\n5\n6\n7\n"));
    }
}
```

`Cargo.toml`에 `regex = "1"`을 추가한다.

- [ ] **Step 3: 통과·커밋·묶음 I 끝**

```bash
cargo test -p devbox-agents && cargo test -p devbox-cli && cargo run -q -p xtask -- gen-ts
git add crates xtask app/src/rpc/gen && git commit -m "feat(agents): detect waiting from output, measure resources and pin token usage"
# 묶음 I 끝: PROGRESS.md의 묶음 I 행과 현재 위치를 고쳐 커밋한 뒤 push한다. PR·CI는 없다(00-roadmap §3)
pnpm check && git push origin v1/s1-agents-terminal
```

---

### Task 11: 프로젝트 설정 파일과 시작 구성

**Files:**
- Modify: `crates/projects/src/config.rs`(Task 8의 최소 `[agent]`에 `[launch]`·`raw_*`·`parse`·`load`를 더한다)
- Modify: `crates/projects/src/lib.rs`(메서드 `projects.config`·`projects.summary`·`projects.launch`·`projects.clone`), `crates/projects/Cargo.toml`(`toml`)
- Test: `crates/projects/src/config.rs` 단위, `crates/cli/tests/projects_launch.rs`

**Interfaces:**
- Produces:
  - `ProjectConfig { agent: AgentConfig { copy, setup, teardown, test? }, launch: LaunchConfig { services, terminals: [{ name, command? }], agents: [{ tool, worktree, prompt? }] }, raw_tasks, raw_services, raw_schedules }`. 작업 관련 절은 S2가 해석한다(`toml::Value` 보관).
  - `projects.config{ id }`(query) → `{ config, error? }`. 파일이 없으면 기본값이다. 문법 오류면 `error: { message }`이고 기본값으로 동작한다.
  - `projects.summary{}`(query) → `{ items: [{ id, branch?, dirty, ahead, behind, terminals, agents: { open, attention } }] }`. 전환기 카드용이며, 각 프로젝트 `git status`를 동시에 4개씩 돈다.
  - `projects.launch{ id }`(mutation) → `LaunchResult { terminals: u32, agents: u32 }`(시작한 개수)
    - `[launch].terminals` → 터미널 세션들
    - `[launch].agents` → worktree 없는 에이전트 작업들(`agents.create` 내부 호출)
    - services는 S2에서 연결하고, 지금은 건너뛰며 `warnings`에 "실행은 S2"를 넣지 않고 조용히 무시한다.
  - `projects.clone{ url, dir? }`(mutation): `git clone`(기본 대상 `~/projects/<repo 이름>`) 후 등록한다.
  - `projects.set_favorite{ id, favorite }`(mutation): S0b의 `favorite` 칸을 바꾼다. 목록은 즐겨찾기 먼저 정렬한다.
  - `projects.prompts{ id }`(query) → `{ items: [{ name, body }] }`: `.devbox/prompts/*.md`(파일 50개·각 64 KiB 상한, 이름순). 새 에이전트 작업의 지시문 템플릿이다. 변수(`{{title}}`·`{{branch}}`·`{{project}}`) 치환은 화면에서 한다.
  - 의존 방향:
    - projects → (terminal, agents)를 직접 부르지 않는다. `launch`는 데몬 라우트가 projects 설정을 읽고 terminal·agents 서비스를 부르는 조립 함수다(`cli/src/daemon/launch.rs`).
    - `summary`도 데몬 조립 함수다. 순환 의존을 피하려고 이렇게 나눈다.

- [ ] **Step 1: 실패하는 테스트(설정 해석)**

```rust
// crates/projects/src/config.rs 끝
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_agent_and_launch_sections_and_keeps_tasks_raw() {
        let c = parse(r#"
[[task]]
id = "test"
command = "pnpm test"

[agent]
copy = [".env"]
setup = ["pnpm install --frozen-lockfile"]
test = "pnpm test"

[launch]
services = ["web"]
terminals = [{ name = "shell" }, { name = "logs", command = "tail -f x.log" }]
agents = [{ tool = "claude", worktree = false }]
"#).unwrap();
        assert_eq!(c.agent.copy, vec![".env"]);
        assert_eq!(c.agent.test.as_deref(), Some("pnpm test"));
        assert_eq!(c.launch.terminals[1].command.as_deref(), Some("tail -f x.log"));
        assert!(!c.launch.agents[0].worktree);
        assert_eq!(c.raw_tasks.len(), 1);
    }

    #[test]
    fn unknown_top_level_keys_are_errors_with_a_message() {
        let e = parse("[agnet]\nsetup = []\n").unwrap_err();
        assert!(e.contains("agnet"), "{e}");
    }

    #[test]
    fn copy_paths_must_stay_inside_the_project() {
        assert!(parse("[agent]\ncopy = [\"../secrets\"]\n").is_err());
        assert!(parse("[agent]\ncopy = [\"/etc/passwd\"]\n").is_err());
    }
}
```

- [ ] **Step 2: 구현**

```rust
// crates/projects/src/config.rs (테스트 위)
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Default, Deserialize, Serialize, TS)]
#[serde(default, deny_unknown_fields)]
pub struct AgentConfig { pub copy: Vec<String>, pub setup: Vec<String>, pub teardown: Vec<String>, pub test: Option<String> }

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct LaunchTerminal { pub name: String, #[serde(default)] pub command: Option<String> }

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct LaunchAgent { pub tool: String, #[serde(default)] pub worktree: bool, #[serde(default)] pub prompt: Option<String> }

#[derive(Debug, Clone, Default, Deserialize, Serialize, TS)]
#[serde(default, deny_unknown_fields)]
pub struct LaunchConfig { pub services: Vec<String>, pub terminals: Vec<LaunchTerminal>, pub agents: Vec<LaunchAgent> }

#[derive(Debug, Clone, Default, Deserialize, Serialize, TS)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectConfig {
    pub agent: AgentConfig,
    pub launch: LaunchConfig,
    #[serde(rename = "task", skip_serializing)]
    #[ts(skip)]
    pub raw_tasks: Vec<toml::Value>,
    #[serde(rename = "service", skip_serializing)]
    #[ts(skip)]
    pub raw_services: Vec<toml::Value>,
    #[serde(rename = "schedule", skip_serializing)]
    #[ts(skip)]
    pub raw_schedules: Vec<toml::Value>,
}

pub fn parse(text: &str) -> Result<ProjectConfig, String> {
    let c: ProjectConfig = toml::from_str(text).map_err(|e| e.to_string())?;
    for p in &c.agent.copy {
        let path = std::path::Path::new(p);
        if path.is_absolute() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(format!("[agent].copy 경로는 프로젝트 안의 상대 경로여야 합니다: {p}"));
        }
    }
    Ok(c)
}

pub fn load(project_path: &std::path::Path) -> (ProjectConfig, Option<String>) {
    match std::fs::read_to_string(project_path.join(".devbox/devbox.toml")) {
        Ok(text) => match parse(&text) {
            Ok(c) => (c, None),
            Err(e) => (ProjectConfig::default(), Some(e)),
        },
        Err(_) => (ProjectConfig::default(), None),
    }
}
```

Task 8에서 임시로 둔 `[agent]` 읽기를 이 `load`로 바꾼다. 데몬 조립 함수 `launch`·`summary`를 `cli/src/daemon/launch.rs`에 두고 라우터에 등록한다.

- [ ] **Step 3: 통합 테스트·커밋**

```rust
// crates/cli/tests/projects_launch.rs — launch가 터미널 2개와 worktree 없는 에이전트 1개를 만든다
mod support;
#[tokio::test]
async fn launch_creates_terminals_and_inline_agents() {
    let d = support::TestDaemon::start_with_env(&[("DEVBOX_NO_SCOPE", "1")]);
    d.write_config("[[agents.profiles]]\nid = \"claude\"\nlabel = \"fake\"\ncommand = \"sleep 600\"\nhooks = \"none\"\n");
    let repo = d.git_repo("projects/web");
    std::fs::create_dir_all(std::path::Path::new(&repo).join(".devbox")).unwrap();
    std::fs::write(std::path::Path::new(&repo).join(".devbox/devbox.toml"), "[launch]\nterminals = [{ name = \"a\" }, { name = \"b\" }]\nagents = [{ tool = \"claude\" }]\n").unwrap();
    let c = d.client().await;
    let p = d.add_project(&c, &repo).await;
    c.raw_call("projects.launch", serde_json::json!({ "id": p }), None).await.unwrap();
    let terms: serde_json::Value = serde_json::from_str(&c.raw_call("terminal.list", serde_json::json!({ "projectId": p }), None).await.unwrap()).unwrap();
    assert_eq!(terms["items"].as_array().unwrap().len(), 3, "2 shells + 1 agent session");
    let agents: serde_json::Value = serde_json::from_str(&c.raw_call("agents.list", serde_json::json!({}), None).await.unwrap()).unwrap();
    assert!(agents["items"][0]["worktreePath"].is_null());
}
```

```bash
cargo test -p devbox-projects && cargo test -p devbox-cli --test projects_launch && cargo run -q -p xtask -- gen-ts
git add crates xtask app/src/rpc/gen && git commit -m "feat(projects): read .devbox/devbox.toml and launch project terminals and agents"
```

---

### Task 12: MCP 서버 (`devbox mcp`)

**Files:**
- Create: `crates/cli/src/mcp.rs`
- Modify: `crates/cli/src/main.rs`(`Mcp` 하위 명령), `crates/core/src/config.rs`(`[mcp] write = false`)
- Test: `crates/cli/tests/mcp.rs`

**Interfaces:**
- Produces:
  - `devbox mcp`: stdio JSON-RPC 2.0(한 줄 = 메시지 하나)
  - 처리하는 메시지: `initialize`(protocolVersion은 클라이언트 값을 그대로 돌려줌, `capabilities.tools`, `serverInfo { name: "devbox", version }`), `notifications/initialized`, `tools/list`, `tools/call`
  - 도구:
    - `devbox_projects`(읽기): 프로젝트 목록
    - `devbox_agents`(읽기): 열린 에이전트 작업과 상태·주의 메시지
    - `devbox_agent_send`(쓰기): `[mcp] write = true`일 때만 목록에 나온다. 다른 에이전트 작업에 지시를 보낸다.
  - 결과는 `content: [{ type: "text", text: <JSON 문자열> }]`이다. 오류는 `isError: true`와 한국어 문구다.
  - S2·S3이 실행·로그·노트 도구를 덧붙인다(같은 표에 행 추가).

- [ ] **Step 1: 실패하는 테스트**

```rust
// crates/cli/tests/mcp.rs
mod support;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn mcp_lists_read_tools_and_calls_projects() {
    let d = support::TestDaemon::start();
    let mut child = Command::new(env!("CARGO_BIN_EXE_devbox")).arg("mcp")
        .env("HOME", d.home.path()).env("XDG_RUNTIME_DIR", d.socket.parent().unwrap()).env("DEVBOX_INSTANCE", &d.instance)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut call = |msg: serde_json::Value| -> serde_json::Value {
        writeln!(stdin, "{msg}").unwrap();
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    let init = call(serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}));
    assert_eq!(init["result"]["serverInfo"]["name"], "devbox");
    writeln!(stdin, "{}", serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})).unwrap();
    let tools = call(serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    let names: Vec<_> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_owned()).collect();
    assert_eq!(names, vec!["devbox_projects", "devbox_agents"]);
    let r = call(serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"devbox_projects","arguments":{}}}));
    assert_eq!(r["result"]["content"][0]["type"], "text");
    let _ = child.kill();
}
```

- [ ] **Step 2: 구현 요점**

```rust
// crates/cli/src/mcp.rs (요점)
pub async fn run() -> anyhow::Result<()> {
    let paths = crate::paths()?;
    let config = devbox_core::Config::load(&paths)?;
    let client = crate::client::Client::connect(&paths.socket, devbox_protocol::message::ClientKind::Mcp, paths.instance.as_str()).await?;
    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut lines = tokio::io::AsyncBufReadExt::lines(stdin);
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
        let Some(id) = msg.get("id").cloned() else { continue }; // 알림은 응답하지 않는다
        let result = match msg["method"].as_str().unwrap_or("") {
            "initialize" => Ok(serde_json::json!({
                "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "devbox", "version": crate::VERSION }
            })),
            "tools/list" => Ok(serde_json::json!({ "tools": tools(config.mcp.write) })),
            "tools/call" => call_tool(&client, &config, &msg["params"]).await,
            other => Err(format!("unknown method {other}")),
        };
        let reply = match result {
            Ok(r) => serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err(e) => serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": e } }),
        };
        use tokio::io::AsyncWriteExt;
        stdout.write_all(format!("{reply}\n").as_bytes()).await?;
        stdout.flush().await?;
    }
    Ok(())
}
```

- `tools(write)`는 이름·설명(한국어)·`inputSchema`(JSON Schema)를 가진 목록이다.
- `call_tool`은 도구 이름으로 `projects.list`·`agents.list`·`agents.send`를 부른다. 결과 JSON을 `content[0].text`에 담고, RPC 오류는 `messageFor`와 같은 뜻의 한국어로 `isError: true`를 붙인다.
- MCP는 외부 JSON-RPC 프로토콜이라 `serde_json::Value`를 쓴다(RPC 경계 규칙의 예외. 데몬 RPC 경계가 아님).
- Claude Code 등록 예시를 `README.md`의 "개발" 절에 한 줄로 적는다: `claude mcp add devbox -- ~/.local/share/devbox/bin/devbox mcp`.

- [ ] **Step 3: 통과·커밋·묶음 J 끝**

```bash
cargo test -p devbox-cli --test mcp && pnpm check
git add crates README.md && git commit -m "feat(cli): expose projects and agents to coding agents over MCP"
# 묶음 J 끝: PROGRESS.md의 묶음 J 행과 현재 위치를 고쳐 커밋한 뒤 push한다. PR·CI는 없다(00-roadmap §3)
git push origin v1/s1-agents-terminal
```

화면과 Windows 통합은 [04b-s1-app](04b-s1-app.md)으로 이어진다.
