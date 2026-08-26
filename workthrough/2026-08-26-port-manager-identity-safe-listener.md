# Port Manager identity-safe listener command

## Overview

Issue #285의 P2-02 초안으로 Port Manager가 Windows native, WSL, Docker
published-port listener를 한 화면에서 구분하고, 선택 당시의 PID만 믿지 않고
endpoint와 process identity를 다시 확인한 뒤 종료하도록 확장했다. Windows는
process creation FILETIME과 process handle, WSL은 distro/PID와 proc start tick을
사용하며, container는 프로세스 종료 대신 WSL Desktop stop handoff descriptor를
반환한다.

## Context

기존 구현은 netstat rows에서 PID와 process name만 얻고 sysinfo의 PID lookup 후
bare PID kill을 호출했다. 따라서 사용자가 메뉴를 연 뒤 PID가 재사용되거나
endpoint가 바뀌어도 의도하지 않은 프로세스에 종료 요청이 갈 수 있었다. 또한
Windows command line/executable path, WSL start tick, container provenance를
표시하지 않았다.

이번 초안은 P2-02 범위만 다룬다. auto-refresh/diff/favorite/provenance UI와
arbitrary process kill은 추가하지 않았고, protocol v2 one-time store는 선행
applink #284와 충돌하지 않도록 Port Manager 소유의 검증된 handoff descriptor
경계로 남겼다.

## Changes Made

### 1. App-local listener core

File: apps/port-manager/src-tauri/src/core/listeners.rs

- ListenerEndpoint, ListenerIdentity, KillListenerRequest, ListenerSnapshot과
  ContainerStopHandoff wire model을 추가했다.
- Windows identity는 PID와 100 ns process creation FILETIME ticks를 가진다. FILETIME은
  JavaScript safe integer 범위를 넘으므로 decimal string으로 wire한다.
- WSL identity는 distro, PID, /proc pid stat field 22 start tick을 가진다.
- Container identity는 docker/podman engine, distro, hexadecimal container ID를
  가지며 process action으로 변환되지 않는다.
- endpoint/protocol/state/listener와 identity의 bounds를 확인하고 established
  connection, PID 0, identity 없는 row를 종료 대상에서 제거한다.
- endpoint와 identity가 한 글자라도 달라지면 ListenerError::StaleTarget으로
  고정 실패한다.
- netstat, ss, proc stat/cmdline, Docker published-port fixture parser와 row/
  output/name/path/command bounds를 추가했다.
- process command/path display에는 common password, token, secret, API key,
  authorization, cookie 계열 key의 값을 redacted한다.

### 2. Windows/WSL command adapter

File: apps/port-manager/src-tauri/src/commands/ports.rs

- list_ports와 kill_listener를 async Tauri command로 감싸고, blocking child
  process 작업은 spawn_blocking에서 수행한다.
- native netstat은 고정 netstat -ano argv만 사용한다.
- Windows process detail은 sysinfo 이름/command vector와 Windows
  GetProcessTimes/QueryFullProcessImageNameW를 결합한다.
- kill 직전 netstat snapshot, endpoint, identity를 다시 비교하고 같은
  process handle에서만 TerminateProcess를 호출한다. handle creation time이
  기대값과 다르면 종료하지 않는다.
- running WSL distro를 고정 wsl.exe argv로 검색하고, 각 distro의 ss output과
  numeric PID로 제한된 /proc stat/cmdline만 조회한다.
- WSL 종료는 start tick을 다시 확인한 뒤 wsl.exe -d DISTRO -- kill -TERM -- PID
  고정 argv로만 실행한다. shell interpolation, arbitrary command/path는 없다.
- child stderr와 OS error 원문을 버리고 stdout은 2 MiB bounded reader thread로 읽는다.
  snapshot의 모든 child는 하나의 15초 deadline을 공유하며 초과 시 child를 종료한다.
- 동일 distro/PID가 여러 endpoint를 열면 bounded detail cache의 start tick과 command를
  모든 row에 재사용한다. 최대 16개 running distro와 256개 unique process만 상세 조회한다.
- Docker ps는 ID/name/Ports의 tab-separated format으로 조회하며 published
  mapping만 container row로 변환한다.
- handoff_container_stop은 container snapshot을 재조회한 뒤 endpoint/identity를
  검증하고 WSL Desktop stop-container descriptor만 반환한다. Docker engine이나
  OS process kill을 직접 호출하지 않는다.
- 기존 PID-only kill_process command와 invoke registration을 제거했다.

### 3. Frontend identity and detail surface

Files: apps/port-manager/src/App.tsx,
apps/port-manager/src/types.ts, apps/port-manager/src/api.ts,
apps/port-manager/src/App.css, apps/port-manager/src/App.test.tsx

- row key에 Windows start time, WSL start tick, container ID를 포함해 stale
  selection이 다른 실행으로 재사용되지 않게 했다.
- kill invoke payload는 endpoint와 identity만 보낸다. executable path나 command
  line은 process control 입력으로 사용하지 않는다.
- Windows/WSL detail과 container engine/distro/ID를 details panel에서 보여준다.
- container action label을 Stop in WSL Desktop으로 분리하고 handoff 결과를
  aria-live status로 알려준다.
- refresh와 lazy process detail lookup에 request generation 및 mounted guard를
  추가해 stale promise와 unmount 이후 setState를 무시한다.
- listener/container action은 state commit 전에도 동기 busy ref로 중복 호출을 막는다.
- IME 조합 중 Enter/Space/F10 shortcut 소비를 막고 table, details, alert,
  handoff, filter에 semantic aria 정보를 추가했다.
- frontend mock row와 tests에 Windows identity, detail, missing identity,
  container handoff, IME, stale request, unmount fixture를 추가했다.

### 4. Dependency and documentation

Files: apps/port-manager/src-tauri/Cargo.toml, Cargo.lock,
apps/port-manager/README.md, docs/architecture.md, docs/roadmap.md,
docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md

- 기존 workspace의 wsl crate와 windows 0.61 API를 target-specific dependency로
  연결했다. 신규 외부 service, sidecar, download, telemetry는 추가하지 않았다.
- README에 source별 identity, command boundary, bounds, privacy, handoff를
  기록했다.
- architecture data flow에 native netstat, WSL proc identity, container handoff
  경계를 반영했다.
- roadmap과 native-first 설계의 P2-02 구현 상태, 포함/제외 범위, W2 및 #284
  통합 검증 잔여 작업을 갱신했다.

## Code Examples

### Kill request contains no display path

~~~rust
pub struct KillListenerRequest {
    pub endpoint: ListenerEndpoint,
    pub identity: ListenerIdentity,
}
~~~

### Pure stale-target check

~~~rust
if request.endpoint != observed.endpoint
    || request.identity != observed.identity
{
    return Err(ListenerError::StaleTarget);
}
~~~

### Windows handle revalidation

~~~rust
let observed_start_time =
    (u64::from(creation.dwHighDateTime) << 32)
        | u64::from(creation.dwLowDateTime);
if observed_start_time.to_string() != expected_start_time {
    return Err(ListenerError::StaleTarget);
}
TerminateProcess(handle, 1)?;
~~~

## Verification Results

### Focused Rust test

~~~text
cargo fmt --manifest-path apps/port-manager/src-tauri/Cargo.toml
cargo test --manifest-path apps/port-manager/src-tauri/Cargo.toml --lib

22 tests passed
exit code: 0
~~~

Covered fixtures include Windows netstat, WSL TCP/UDP ss states, Docker IPv4/IPv6 mappings,
parenthesized proc stat, NUL-separated cmdline, reused PID/start-time mismatch,
changed endpoint, established-target rejection, oversized output, credential
redaction including multi-token Authorization/API-key values, strict request wire shape,
zero start tick/oversized PID rejection, and container handoff.

### Focused frontend test and build

~~~text
pnpm --filter port-manager test
21 tests passed

pnpm --filter port-manager build
TypeScript and Vite production build passed
~~~

The frontend fixtures cover lazy path actions, confirmation and refresh, stale/unmounted
requests, IME shortcuts, accessible details, container handoff, and state-commit-before-click
duplicate action suppression.

### Static checks

~~~text
git diff --check
passed
~~~

`cargo clippy --manifest-path apps/port-manager/src-tauri/Cargo.toml --lib -- -D warnings`
and focused `cargo fmt --check` also passed after the platform-gated core helpers,
bounded command deadline, and redaction branches were reviewed.

### Repository-wide pre-PR gates

After rebasing onto `main` commit `847e28f`, the complete local gates passed:

~~~text
cargo test --workspace -j4
cargo check --workspace -j4
cargo clippy --workspace --all-targets -j4 -- -D warnings
cargo fmt --all -- --check
pnpm test
pnpm build
~~~

All 17 frontend projects completed their tests and production builds. Dependency-policy
and notice checks, their regression fixtures, build-manifest fixtures, catalog consistency,
CI scope detection, and diff whitespace checks also passed. CI scope correctly resolves
the frontend to Port Manager and Rust fail-safe to the full workspace because `Cargo.lock`
changes.

### Not run in this draft

- Windows GNU target check (`cargo check --manifest-path
  apps/port-manager/src-tauri/Cargo.toml --target x86_64-pc-windows-gnu --lib
  -j1`) stopped before app-library compilation because the Linux host has no
  `x86_64-w64-mingw32-windres`. MSVC check, packaged W2 smoke, real WSL process
  signal, and Docker handoff consumer require Windows/runtime state and should
  run after the P2-01 applink contract is available.
- GitHub Windows compile/test/clippy gate and the packaged W2 runtime checks below.

## Next Steps

- On Windows, verify packaged listener enumeration and permission-denied behavior.
- Execute reused-PID, WSL start-tick mismatch, and container-ID changed smoke fixtures.
- Connect ContainerStopHandoff to protocol v2 one-time handoff and WSL Desktop's
  explicit stop consumer after #284 lands.
- Keep auto-refresh, diff, favorite/provenance, and arbitrary PID kill in their
  separately tracked scopes.
