# P1-08 자식 프로세스 트리 소유를 한 crate로 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** 7개 crate에 따로 있는 "자식 프로세스와 그 자손을 한 단위로 소유하고 확실히 끝내기" 구현(Windows Job Object + 일시정지 시작, Unix 프로세스 그룹)을 `crates/process-tree` 하나로 모아 `unsafe` 코드와 검토 대상을 한곳에 둔다(A4). 앞 PR(P1-07)의 기계적 변경 commit을 blame에서 제외한다.

**Architecture:** 가장 완전한 구현인 `crates/http-client-engine/src/commands/process_tree.rs`(일시정지 시작 → kill-on-close Job 배정 → Job에 root 하나만 있는지 확인 → 주 스레드 재개, 기한 안에서 SIGTERM→SIGKILL 단계 종료)를 새 crate의 바탕으로 옮기고, 동기(`std::process`) 소비자를 위한 API와 `terminate_descendants`를 더한다. 소비자는 `prepare_*`로 명령을 준비하고 `assign*`으로 소유한 뒤 `terminate*`로 끝낸다. 자체 `CreateProcess` 실행기를 가진 `runtime-engine/src/platform/windows.rs`는 프로세스 식별·stdio 소유까지 다루므로 옮기지 않는다.

**Tech Stack:** Rust, `windows` 0.61(workspace), `libc`(unix), `tokio`(선택 feature)

**Spec:** `review.md` §6 A4

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 동작 불변: 각 소비자의 종료 기한(`CLEANUP_TIMEOUT` 등)과 실패 시 반환값(오류 문자열)을 그대로 유지한다.
- 새 외부 의존성 없음(`libc`, `tokio`, `windows`는 이미 workspace에 있다).
- 옮기는 소비자: `devbox-http-client-engine`, `devbox-projects-engine`, `devbox-editor-engine`(LSP), `git`, `devbox-logs-engine`, `devbox-ports-engine`, `devbox-installation-tools`(doctor·related_tools). 옮기지 않는 것: `devbox-runtime-engine`의 Windows 실행기.

## Review Focus

1. Windows에서 자식이 일시정지 상태로 시작하지 않았는데 `assign`을 부름 → 주 스레드 재개 확인(`previous_suspend_count == 1`)이 실패해 Job을 종료하고 오류를 돌려준다(자손이 Job 밖으로 새지 않음). (Task 1, Windows 테스트)
2. 자식이 자손을 만든 뒤 root만 먼저 끝남 → `terminate_descendants`가 남은 자손을 기한 안에 끝낸다. (Task 1)
3. 소비자가 `prepare_*` 없이 spawn → Windows에서 `assign`이 실패한다. 모든 소비자의 spawn 경로가 `prepare_*`를 거치는지 확인한다(Task 2·3 체크리스트).
4. `ProcessTree`를 async 작업 사이로 옮김 → `Send` 유지(기존 테스트 `process_tree_can_move_with_its_async_worker`). (Task 1)
5. Drop만으로 정리됐다고 보고함 → Drop은 신호만 보내고 "정리 확인"은 `terminate*`의 반환값으로만 한다(기존 주석 계약 유지). (Task 1)

## Branch · PR

- 묶음: **B5** — 브랜치 `refactor/crates/shared-platform-crates`, PR 제목 `refactor(crates): share process-tree, DPAPI, WSL helper and Markdown preview`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `refactor(crates): share one owned process-tree implementation`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

## File Structure

| 파일 | 변경 |
|---|---|
| `.git-blame-ignore-revs` | 생성: P1-07 squash commit |
| `crates/process-tree/{Cargo.toml,src/lib.rs}` | 생성(http-client-engine 구현을 옮겨 일반화) |
| 루트 `Cargo.toml` | member·workspace dependency 추가 |
| 소비자 7개 crate | 자체 구현 삭제, 새 crate 사용 |
| `crates/suite-runtime/src/platform/peer_identity.rs` | `struct Handle(HANDLE)` → `OwnedHandle` |

---

### Task 0: blame 제외 목록

- [ ] 묶음 B4(P1-07만 담은 기계적 변경 PR)의 squash commit SHA(ledger에 기록됨)로 루트 `.git-blame-ignore-revs`를 만든다. B4에는 다른 변경을 섞지 않으므로 이 commit만 blame에서 빼면 된다.

```text
# Mechanical changes only: crate renames and Biome formatting (P1-07).
<B4 squash commit SHA>
```

  `CONVENTIONS.md`의 Git 절에 "로컬 blame은 `git config blame.ignoreRevsFile .git-blame-ignore-revs`를 한 번 설정한다. GitHub blame은 이 파일을 자동으로 따른다."를 추가한다.
- [ ] 커밋: `git add .git-blame-ignore-revs CONVENTIONS.md && git commit -m "chore(workspace): ignore the mechanical formatting commit in blame"`

---

### Task 1: `crates/process-tree`

**Files:** Create `crates/process-tree/Cargo.toml`, `crates/process-tree/src/lib.rs`(`git mv crates/http-client-engine/src/commands/process_tree.rs`에서 시작)

**Interfaces (Produces):**
- `process_tree::CLEANUP_TIMEOUT: Duration`(750ms, 기존 값)
- `ProcessTree::prepare_std(&mut std::process::Command)`, `ProcessTree::prepare_tokio(&mut tokio::process::Command)`(feature `tokio`)
- `ProcessTree::assign_std(&std::process::Child) -> Result<ProcessTree, ()>`, `ProcessTree::assign(&tokio::process::Child) -> Result<ProcessTree, ()>`(feature `tokio`)
- `ProcessTree::is_empty(&self) -> Option<bool>`
- `ProcessTree::terminate_blocking(&mut self, &mut std::process::Child, deadline: std::time::Instant) -> bool`
- `async ProcessTree::terminate(&mut self, &mut tokio::process::Child) -> bool`, `terminate_until(.., tokio::time::Instant) -> bool`, `async ProcessTree::terminate_unassigned(&mut tokio::process::Child) -> bool`
- `ProcessTree::terminate_descendants(&mut self) -> bool`(root 종료 후 남은 자손을 `CLEANUP_TIMEOUT` 안에 끝냄, blocking)

- [ ] **Step 1: crate 만들기와 이동**

```bash
mkdir -p crates/process-tree/src
git mv crates/http-client-engine/src/commands/process_tree.rs crates/process-tree/src/lib.rs
```

`crates/process-tree/Cargo.toml`:

```toml
[package]
name = "process-tree"
version = "0.1.0"
edition.workspace = true

[features]
default = []
tokio = ["dep:tokio"]

[dependencies]
tokio = { workspace = true, optional = true }

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[target.'cfg(windows)'.dependencies]
windows = { workspace = true }

[dev-dependencies]
tokio = { workspace = true }
```

  (`libc` 버전은 `Cargo.lock`에 있는 버전으로 맞춘다. 두 곳 이상에서 쓰면 P1-06 규칙대로 workspace 의존성으로 옮긴다.) 루트 `Cargo.toml` `members`에 `"crates/process-tree"`, `[workspace.dependencies]`에 `process-tree = { path = "crates/process-tree" }`를 추가한다.

- [ ] **Step 2: 실패하는 테스트** — `lib.rs` 테스트 모듈에 추가한다(기존 테스트는 그대로 둔다).

```rust
    #[cfg(unix)]
    #[test]
    fn blocking_termination_ends_the_whole_group() {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "sleep 30 & sleep 30"]);
        ProcessTree::prepare_std(&mut command);
        let mut child = command.spawn().unwrap();
        let mut tree = ProcessTree::assign_std(&child).unwrap();
        assert_eq!(tree.is_empty(), Some(false));
        assert!(tree.terminate_blocking(&mut child, std::time::Instant::now() + CLEANUP_TIMEOUT));
        assert_eq!(tree.is_empty(), Some(true));
    }

    #[cfg(unix)]
    #[test]
    fn descendants_are_ended_after_the_root_exits() {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "sleep 30 & exit 0"]);
        ProcessTree::prepare_std(&mut command);
        let mut child = command.spawn().unwrap();
        let mut tree = ProcessTree::assign_std(&child).unwrap();
        child.wait().unwrap();
        assert!(tree.terminate_descendants());
        assert_eq!(tree.is_empty(), Some(true));
    }

    #[cfg(windows)]
    #[test]
    fn windows_children_start_owned_and_end_with_their_descendants() {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start /B ping -n 30 127.0.0.1 >NUL & ping -n 30 127.0.0.1 >NUL"]);
        ProcessTree::prepare_std(&mut command);
        let mut child = command.spawn().unwrap();
        let mut tree = ProcessTree::assign_std(&child).unwrap();
        assert_eq!(tree.is_empty(), Some(false));
        assert!(tree.terminate_blocking(&mut child, std::time::Instant::now() + CLEANUP_TIMEOUT));
        assert_eq!(tree.is_empty(), Some(true));
    }

    #[cfg(windows)]
    #[test]
    fn a_child_that_was_not_started_suspended_is_refused() {
        let mut child = std::process::Command::new("cmd").args(["/C", "ping -n 5 127.0.0.1 >NUL"]).spawn().unwrap();
        assert!(ProcessTree::assign_std(&child).is_err());
        let _ = child.kill();
        let _ = child.wait();
    }
```

- [ ] **Step 3: 실패 확인** — Run: `source ~/.cargo/env && cargo test -p process-tree --features tokio` → 컴파일 실패(`prepare_std` 등 없음).

- [ ] **Step 4: 일반화 구현**
  - `pub(crate)` → `pub`. 파일 머리 주석은 유지하고 "Shared by every product crate that spawns helper processes." 한 줄을 더한다.
  - `tokio` 타입을 쓰는 함수(`assign`, `terminate`, `terminate_until`, `terminate_unassigned`, `wait_for_empty`)에 `#[cfg(feature = "tokio")]`를 붙인다.
  - 공통 배정 로직을 뽑는다: 기존 `assign(child: &Child)`의 Windows 분기 본문을 `fn assign_parts(pid: u32, process: HANDLE) -> Result<Self, ()>`로, Unix 분기를 `fn assign_group(pid: u32) -> Result<Self, ()>`로 옮기고, `assign`(tokio)과 새 `assign_std`는 각각 `child.id()`·`raw_handle()`(tokio) / `child.id()`·`as_raw_handle()`(std)로 값을 얻어 이 둘을 부른다.
  - 명령 준비:

```rust
impl ProcessTree {
    /// Start the child suspended (Windows) or as its own process group (Unix)
    /// so `assign*` can take ownership before it runs any code.
    pub fn prepare_std(command: &mut std::process::Command) {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            use windows::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
            command.creation_flags(CREATE_NO_WINDOW.0 | CREATE_SUSPENDED.0);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
    }

    #[cfg(feature = "tokio")]
    pub fn prepare_tokio(command: &mut tokio::process::Command) {
        #[cfg(windows)]
        {
            use windows::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
            command.creation_flags(CREATE_NO_WINDOW.0 | CREATE_SUSPENDED.0);
        }
        #[cfg(unix)]
        {
            command.process_group(0);
        }
    }
}
```

  (소비자가 이미 다른 creation flag를 쓰면 `prepare_*` 뒤에 `|`로 더하지 말고, 소비자 쪽에서 `prepare_*`를 부른 뒤 필요한 flag를 한 번에 다시 설정하도록 Task 2·3에서 맞춘다.)
  - 동기 종료:

```rust
    pub fn is_empty(&self) -> Option<bool> {
        if self.terminal_empty {
            return Some(true);
        }
        self.authority_is_empty()
    }

    pub fn terminate_blocking(
        &mut self,
        child: &mut std::process::Child,
        deadline: std::time::Instant,
    ) -> bool {
        let wait_root = |child: &mut std::process::Child, until: std::time::Instant| loop {
            match child.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) if std::time::Instant::now() < until => std::thread::sleep(POLL_INTERVAL),
                _ => return false,
            }
        };
        if !self.terminal_empty && !self.signal_termination(false) {
            let _ = child.kill();
        }
        let now = std::time::Instant::now();
        let grace = now + deadline.saturating_duration_since(now) / 2;
        let mut root_gone = wait_root(child, grace);
        if !root_gone || self.authority_is_empty() != Some(true) {
            let _ = self.signal_termination(true);
            if !root_gone {
                let _ = child.kill();
                root_gone = wait_root(child, deadline);
            }
        }
        let tree_gone = self.wait_empty_blocking(deadline);
        if tree_gone {
            self.terminal_empty = true;
        }
        tree_gone && root_gone
    }

    pub fn terminate_descendants(&mut self) -> bool {
        if self.terminal_empty {
            return true;
        }
        let _ = self.signal_termination(true);
        let gone = self.wait_empty_blocking(std::time::Instant::now() + CLEANUP_TIMEOUT);
        if gone {
            self.terminal_empty = true;
        }
        gone
    }

    fn wait_empty_blocking(&self, deadline: std::time::Instant) -> bool {
        loop {
            if self.authority_is_empty() == Some(true) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
```

  - `CLEANUP_TIMEOUT`를 `pub const`로, `POLL_INTERVAL`은 private 유지.

- [ ] **Step 5: 통과 확인** — Run: `cargo test -p process-tree --features tokio && cargo clippy -p process-tree --all-features --all-targets -- -D warnings` → PASS(Windows 테스트는 CI `Rust (Windows)`에서 돈다).

- [ ] **Step 6: 커밋** — `git add -A && git commit -m "refactor(crates): extract the owned process tree into its own crate"`

---

### Task 2: async 소비자

**Files:** `crates/http-client-engine`, `crates/projects-engine`, `crates/editor-engine`

- [ ] **Step 1: 옮기기** — 각 crate `Cargo.toml`에 `process-tree = { workspace = true, features = ["tokio"] }`를 추가한다.
  - http-client-engine: `commands/mod.rs`의 `mod process_tree;`를 지우고 `use crate::commands::process_tree::…`를 `use process_tree::…`로 바꾼다. spawn하는 곳(`rg -n "ProcessTree::assign" crates/http-client-engine`)의 바로 앞 `Command` 설정에 `ProcessTree::prepare_tokio(&mut command)`가 있는지 확인하고, creation flag를 직접 넣던 코드는 지운다.
  - projects-engine: `commands/process_tree.rs`를 지우고 같은 방식으로 바꾼다. 이 파일에만 있던 `terminate_descendants`는 새 crate의 같은 이름 함수로 대체된다(동작: 강제 종료 신호 후 `CLEANUP_TIMEOUT` 대기). 기존 테스트 중 새 crate와 겹치지 않는 것(예: 소비자 고유 동작)은 소비자에 남긴다.
  - editor-engine: `lsp/windows_job.rs`의 `assign_to`·`is_empty`·`terminate`를 새 crate로 바꾼다. 반환 오류가 `String`이었으므로 호출부에서 `.map_err(|()| "lsp_process_job_unavailable".to_string())`처럼 기존 오류 문자열을 그대로 만든다(`rg -n "windows_job::" crates/editor-engine`로 호출부 확인). `process.rs`의 `ProcessTreeCleanup`이 이 타입을 감싸면 그대로 둔다.
- [ ] **Step 2: 확인** — Run: `cargo test -p devbox-http-client-engine -p devbox-projects-engine -p devbox-editor-engine --lib` → PASS.
- [ ] **Step 3: 커밋** — `git add -A && git commit -m "refactor(crates): use the shared process tree in async helpers"`

---

### Task 3: 동기 소비자

**Files:** `crates/git/src/lib.rs`, `crates/logs-engine/src/core/sources.rs`, `crates/ports-engine/src/commands/ports.rs`, `crates/installation-tools/src/commands/{doctor,related_tools}.rs`

- [ ] **Step 1: 옮기기** — 각 crate에 `process-tree = { workspace = true }`를 추가하고 cfg별로 세 벌씩 있던 `struct ProcessTree`와 Job 생성 코드를 지운다. 기존 호출을 아래처럼 바꾼다.

| 기존 | 새 코드 |
|---|---|
| `ProcessTree::spawn(&mut command)` (logs) | `ProcessTree::prepare_std(&mut command); let child = command.spawn()?; let tree = ProcessTree::assign_std(&child)?;` |
| `ProcessTree::assign_to(&child)` (git) | `ProcessTree::assign_std(&child)` (spawn 전에 `prepare_std`) |
| `tree.terminate(&mut child)` (git, 동기) | `tree.terminate_blocking(&mut child, Instant::now() + CLEANUP_TIMEOUT)` |
| `tree.terminate_descendants()` | 같은 이름 |
| ports의 인라인 Job 생성(`CreateJobObjectW` 블록) | `prepare_std` + `assign_std`, 끝낼 때 `terminate_blocking` |
| installation-tools `ProcessTree`(related_tools·doctor) | 같은 규칙 |

  비Windows·비Unix용 빈 구현(`struct ProcessTree;`)은 새 crate가 대신하므로 지운다.
- [ ] **Step 2: 확인** — Run: `cargo test -p git -p devbox-logs-engine -p devbox-ports-engine -p devbox-installation-tools --lib && ! rg -n "CreateJobObjectW" crates --glob '!crates/process-tree/**' --glob '!crates/runtime-engine/**'` → PASS(마지막 명령은 0건).
- [ ] **Step 3: 커밋** — `git add -A && git commit -m "refactor(crates): use the shared process tree in blocking helpers"`

---

### Task 4: suite-runtime 핸들

- [ ] `crates/suite-runtime/src/platform/peer_identity.rs`의 `struct Handle(HANDLE)`와 그 `Drop`을 `std::os::windows::io::OwnedHandle`로 바꾼다: 핸들을 얻는 곳에서 `unsafe { OwnedHandle::from_raw_handle(handle.0 as _) }`, Win32 호출에는 `HANDLE(owned.as_raw_handle() as _)`. Run: `cargo check -p suite-runtime && cargo check --target x86_64-pc-windows-msvc -p suite-runtime`(가능하면) → PASS. 커밋: `git commit -am "refactor(suite): own peer handles with OwnedHandle"`

---

### Task 5: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9. PR 본문에 "옮긴 소비자 7곳, 옮기지 않은 runtime 실행기와 이유"를 적는다. Windows 전용 테스트는 CI `Rust (Windows)` 결과를 근거로 적는다.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): Workspace에서 Git fetch 중 취소, LSP 서버 재시작, Ports 화면 새로고침, API Studio에서 MCP stdio 서버 연결·해제 후 작업 관리자에 남은 자식 프로세스(`git.exe`, `node.exe` 등)가 없다.
