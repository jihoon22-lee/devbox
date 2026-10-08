# S0a 구조 확인 실험 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

- 상태: 계획 · 미착수

**Goal:** 재구축 설계가 기대는 플랫폼 가정(브리지 성능, systemd user unit, 실행 결과 기록, tmux 서버 유지, 에이전트 hook 주입, systemd 아래 interop, WSL 유휴 동작)을 실제 환경에서 측정해 합격·불합격을 판정하고, 결과를 설계에 반영한다.

**Architecture:** 저장소 밖 임시 폴더(`/home/jihoon/projects/devbox-spikes/s0a/`)에 버릴 실험 도구를 만든다.
- `devbox-probe`: Rust 에코 서버와 브리지
- 측정 스크립트: PowerShell·셸

결과는 이 계획 폴더의 `09-s0a-results.md` 하나로 남긴다. 실험 코드는 저장소에 넣지 않는다.

**Tech Stack:** Rust 1.98(tokio), systemd 259 user manager, tmux, PowerShell 5.1(.NET Process), Claude Code 2.1.x, codex-cli 0.160.x.

**Spec:** [01-design.md](01-design.md) §4·§5.4·§6.3·§9.2·§14 (R1·R2·R3·R5·R10)

## Global Constraints

- 저장소(`/home/jihoon/projects/devbox`)의 제품 파일을 바꾸지 않는다. 바꾸는 것은 이 계획 폴더뿐이다: 결과 문서 `09-s0a-results.md`, `PROGRESS.md`, 판정에 따라 `01-design.md`와 그 가정을 코드로 적은 계획 과제(Task 8 Step 2의 표). 이 변경은 계획 원본 worktree `../devbox-wt/docs-v1-plan`(브랜치 `docs/v1-plan`)에서 하고 바로 커밋·push한다. PR·CI는 없다(00-roadmap §2 P0).
- 이 단계는 S0b 전이라 main의 `AGENTS.md`가 v0.9.0용이다. 00-roadmap §3의 규칙이 우선한다.
- 실험용 systemd unit·tmux 서버·파일 이름에는 반드시 `s0a`를 넣는다(`devbox-s0a-*`, `tmux -L devbox-s0a`). 사용자의 기존 tmux·unit과 섞이지 않게 한다.
- `/etc`를 바꾸지 않는다. 패키지를 설치하지 않는다. `wsl --shutdown`은 에이전트가 실행하지 않는다(자신도 종료됨). 그런 확인은 Task 7의 사용자 수동 항목이다.
- 실험이 끝나면 Task 8에서 만든 unit·소켓·tmux 서버·임시 파일을 모두 지운다.
- 에이전트 hook 실험(Task 6)은 실제 Claude·Codex 호출을 하므로, 지시는 짧게(한 문장) 한다.

## Review Focus

| 상황 | 기대 동작 |
|---|---|
| 처음 측정한 브리지 값이 VM이 이미 떠 있는 상태뿐 | VM이 막 켜진 직후의 첫 응답 시간을 사용자 수동 항목(Task 7)으로 따로 잰다 |
| systemd 아래에서 interop이 되는 것처럼 보이지만 사용자 로그인 세션에서만 됨 | 반드시 `systemd-run --user`로 띄운 프로세스에서 확인한다(Task 5) |
| hook이 `-p`(비대화) 모드에서만 동작 | tmux 안 대화형 실행에서 확인한다(Task 6) |
| `tmux -D`가 없는 버전 | 버전과 대체(`Type=forking`) 동작을 함께 기록한다(Task 4) |
| 실험 unit이 남아 사용자 환경을 더럽힘 | Task 8의 정리 확인 명령 결과가 비어 있어야 끝난다 |

---

### Task 1: 실험 도구 만들기와 브리지 성능 측정 (R3)

**Files:**
- Create: `/home/jihoon/projects/devbox-spikes/s0a/Cargo.toml`
- Create: `/home/jihoon/projects/devbox-spikes/s0a/src/main.rs`
- Create: `/home/jihoon/projects/devbox-spikes/s0a/bench.ps1`

**Interfaces:**
- Produces: `devbox-probe serve --sock <path>`(에코 서버, systemd socket activation 지원), `devbox-probe bridge --sock <path>`(표지 바이트열 `DEVBOX-BRIDGE/1\n` 16바이트를 먼저 쓰고 stdin↔socket 중계). Task 2·7이 사용한다.

- [ ] **Step 1: 크레이트 작성**

```toml
# /home/jihoon/projects/devbox-spikes/s0a/Cargo.toml
[package]
name = "devbox-probe"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "io-util", "io-std"] }

[profile.release]
debug = "line-tables-only"
```

```rust
// /home/jihoon/projects/devbox-spikes/s0a/src/main.rs
use std::io;
use std::os::fd::FromRawFd;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};

const MAGIC: &[u8; 16] = b"DEVBOX-BRIDGE/1\n";

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let sock = PathBuf::from(arg(&args, "--sock").unwrap_or_else(|| {
        eprintln!("usage: devbox-probe serve|bridge --sock PATH");
        std::process::exit(2)
    }));
    match args.get(1).map(String::as_str) {
        Some("serve") => serve(sock).await,
        Some("bridge") => bridge(sock).await,
        _ => {
            eprintln!("usage: devbox-probe serve|bridge --sock PATH");
            std::process::exit(2)
        }
    }
}

fn listener(sock: &PathBuf) -> io::Result<UnixListener> {
    // systemd socket activation: LISTEN_FDS=1 means fd 3 is our listening socket.
    if std::env::var("LISTEN_FDS").ok().as_deref() == Some("1") {
        let std_listener = unsafe { std::os::unix::net::UnixListener::from_raw_fd(3) };
        std_listener.set_nonblocking(true)?;
        return UnixListener::from_std(std_listener);
    }
    let _ = std::fs::remove_file(sock);
    UnixListener::bind(sock)
}

async fn serve(sock: PathBuf) -> io::Result<()> {
    let l = listener(&sock)?;
    eprintln!("probe serve pid={} activated={}", std::process::id(), std::env::var("LISTEN_FDS").is_ok());
    loop {
        let (mut s, _) = l.accept().await?;
        tokio::spawn(async move {
            let (mut r, mut w) = s.split();
            let _ = tokio::io::copy(&mut r, &mut w).await;
        });
    }
}

async fn bridge(sock: PathBuf) -> io::Result<()> {
    let mut stdout = tokio::io::stdout();
    stdout.write_all(MAGIC).await?;
    stdout.flush().await?;
    let s = UnixStream::connect(&sock).await?;
    let (mut sr, mut sw) = s.into_split();
    let mut stdin = tokio::io::stdin();
    let up = async { tokio::io::copy(&mut stdin, &mut sw).await };
    let down = async { tokio::io::copy(&mut sr, &mut stdout).await };
    tokio::select! { r = up => { r?; } r = down => { r?; } }
    Ok(())
}
```

- [ ] **Step 2: 빌드**

Run: `cd /home/jihoon/projects/devbox-spikes/s0a && source ~/.cargo/env && cargo build --release`
Expected: `target/release/devbox-probe` 생성, 경고 0

- [ ] **Step 3: 측정 스크립트 작성**

```powershell
# /home/jihoon/projects/devbox-spikes/s0a/bench.ps1
param(
  [string]$Distro = "Ubuntu",
  [string]$Probe = "/home/jihoon/projects/devbox-spikes/s0a/target/release/devbox-probe",
  [string]$Sock = "/run/user/1000/devbox-s0a.sock"
)
$ErrorActionPreference = "Stop"
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = "wsl.exe"
$psi.Arguments = "-d $Distro --exec $Probe bridge --sock $Sock"
$psi.UseShellExecute = $false
$psi.CreateNoWindow = $true
$psi.RedirectStandardInput = $true
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$sw = [Diagnostics.Stopwatch]::StartNew()
$p = [Diagnostics.Process]::Start($psi)
$in = $p.StandardInput.BaseStream
$out = $p.StandardOutput.BaseStream

function ReadExact([int]$n) {
  $buf = New-Object byte[] $n; $got = 0
  while ($got -lt $n) { $r = $out.Read($buf, $got, $n - $got); if ($r -le 0) { throw "eof after $got/$n" }; $got += $r }
  return ,$buf
}
$magic = [Text.Encoding]::ASCII.GetString((ReadExact 16))
if ($magic -ne "DEVBOX-BRIDGE/1`n") { throw "bad magic: $magic" }
$firstMs = $sw.Elapsed.TotalMilliseconds

function RoundTrip([byte[]]$payload) {
  $len = [BitConverter]::GetBytes([uint32]$payload.Length)
  $in.Write($len, 0, 4); $in.Write($payload, 0, $payload.Length); $in.Flush()
  $hdr = ReadExact 4
  return ,(ReadExact ([BitConverter]::ToUInt32($hdr, 0)))
}
$rng = New-Object Random 7
function Lat([int]$size, [int]$count) {
  $payload = New-Object byte[] $size; $rng.NextBytes($payload)
  $t = New-Object 'System.Collections.Generic.List[double]'
  for ($i = 0; $i -lt $count; $i++) {
    $s = [Diagnostics.Stopwatch]::StartNew(); [void](RoundTrip $payload); $t.Add($s.Elapsed.TotalMilliseconds)
  }
  $sorted = $t | Sort-Object
  return @{ p50 = $sorted[[int]($count * 0.5)]; p99 = $sorted[[int]($count * 0.99)]; max = $sorted[-1] }
}
$l64 = Lat 64 1000
$l4k = Lat 4096 1000
$mb = New-Object byte[] (1MB); $rng.NextBytes($mb)
$s = [Diagnostics.Stopwatch]::StartNew(); for ($i = 0; $i -lt 64; $i++) { [void](RoundTrip $mb) }
$echoMiBs = 64 / $s.Elapsed.TotalSeconds
$big = New-Object byte[] (8MB); $rng.NextBytes($big)
$back = RoundTrip $big
$same = [Linq.Enumerable]::SequenceEqual([byte[]]$big, [byte[]]$back)
$in.Close(); [void]$p.WaitForExit(5000)
[pscustomobject]@{
  first_response_ms = [math]::Round($firstMs, 1)
  rt64_p50_ms = [math]::Round($l64.p50, 3); rt64_p99_ms = [math]::Round($l64.p99, 3); rt64_max_ms = [math]::Round($l64.max, 3)
  rt4k_p50_ms = [math]::Round($l4k.p50, 3)
  echo_MiB_per_s = [math]::Round($echoMiBs, 1)
  binary_8MiB_identical = $same
  bridge_exited = $p.HasExited
} | ConvertTo-Json
```

- [ ] **Step 4: 에코 서버를 띄우고 측정**

Run (WSL):

```bash
cd /home/jihoon/projects/devbox-spikes/s0a
./target/release/devbox-probe serve --sock /run/user/$(id -u)/devbox-s0a.sock & echo $! > serve.pid
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$(wslpath -w bench.ps1)" | tee bench-warm.json
kill "$(cat serve.pid)"
```

Expected: JSON 출력. 합격 기준은 다음과 같다.
- `rt64_p50_ms ≤ 5`
- `echo_MiB_per_s ≥ 20`
- `binary_8MiB_identical = true`
- `bridge_exited = true`(stdin을 닫으면 브리지가 끝남)

- [ ] **Step 5: 결과 기록**

`09-s0a-results.md`의 "R3 브리지" 절에 JSON과 판정을 붙인다(Task 8의 양식).

---

### Task 2: systemd user 템플릿 unit과 socket activation (R2 일부)

**Files:**
- Create: `~/.config/systemd/user/devbox-s0a@.socket`
- Create: `~/.config/systemd/user/devbox-s0a@.service`

**Interfaces:**
- Consumes: Task 1의 `devbox-probe serve`
- Produces: 판정 "템플릿 unit + socket activation + EnvironmentFile 동작 여부" → 01-design §4.1

- [ ] **Step 1: 사용자 환경 잡기와 unit 작성**

```bash
mkdir -p ~/.config/devbox-s0a && bash -lc 'env -0' | tr '\0' '\n' | grep -E '^(PATH|HOME|LANG|NVM_DIR|CARGO_HOME|RUSTUP_HOME)=' > ~/.config/devbox-s0a/env
cat ~/.config/devbox-s0a/env | grep '^PATH='
systemctl --user show-environment | grep '^PATH='
```

Expected: 첫 PATH에는 `~/.local/bin`·`~/.cargo/bin`이 있고, 두 번째(user manager)에는 없다(PL-위험1 재현).

```ini
# ~/.config/systemd/user/devbox-s0a@.socket
[Socket]
ListenStream=%t/devbox-s0a-%i.sock
SocketMode=0600

[Install]
WantedBy=sockets.target
```

```ini
# ~/.config/systemd/user/devbox-s0a@.service
[Service]
Type=simple
EnvironmentFile=%h/.config/devbox-s0a/env
ExecStart=/home/jihoon/projects/devbox-spikes/s0a/target/release/devbox-probe serve --sock %t/devbox-s0a-%i.sock
Restart=on-failure
KillMode=mixed
```

- [ ] **Step 2: socket activation 확인**

Run:

```bash
systemctl --user daemon-reload
systemctl --user start devbox-s0a@probe.socket
systemctl --user is-active devbox-s0a@probe.service || true      # 아직 inactive 기대
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$(wslpath -w bench.ps1)" -Sock /run/user/$(id -u)/devbox-s0a-probe.sock | tee bench-activated.json
systemctl --user is-active devbox-s0a@probe.service
journalctl --user -u devbox-s0a@probe.service -n 5 --no-pager
```

Expected: 측정 전에는 `inactive`, 측정 뒤에는 `active`이다. 로그에 `activated=true`가 찍힌다. 측정값은 Task 1과 비슷하다.

- [ ] **Step 3: 서비스 안에서 사용자 도구가 보이는지 확인**

```bash
systemd-run --user --wait --pipe --collect -p EnvironmentFile=$HOME/.config/devbox-s0a/env -- sh -c 'command -v claude; command -v codex; command -v pnpm; command -v cargo'
systemd-run --user --wait --pipe --collect -- sh -c 'command -v claude || echo MISSING-claude'
```

Expected: 첫 줄은 네 경로를 모두 출력하고, 두 번째는 `MISSING-claude`를 출력한다(EnvironmentFile이 필요함을 확인).

---

### Task 3: 실행 unit의 결과 기록 (ExecStopPost)과 데몬 재시작 생존

**Files:**
- Create: `/home/jihoon/projects/devbox-spikes/s0a/run-exited.sh`

**Interfaces:**
- Produces: 판정 "transient 실행 unit + ExecStopPost로 결과 파일 기록, 데몬(unit) 재시작과 무관하게 실행 유지" → 01-design §4.3

- [ ] **Step 1: 결과 기록 스크립트**

```bash
cat > /home/jihoon/projects/devbox-spikes/s0a/run-exited.sh <<'EOF'
#!/bin/sh
# usage: run-exited.sh <runId>
printf '%s %s %s\n' "$SERVICE_RESULT" "$EXIT_CODE" "$EXIT_STATUS" > "/tmp/devbox-s0a/$1.exit"
EOF
chmod +x /home/jihoon/projects/devbox-spikes/s0a/run-exited.sh && mkdir -p /tmp/devbox-s0a
```

- [ ] **Step 2: 정상·실패·강제 종료 세 경우 실행**

```bash
S=/home/jihoon/projects/devbox-spikes/s0a/run-exited.sh
systemd-run --user --unit devbox-s0a-run-ok --collect -p ExecStopPost="$S ok" -- sh -c 'echo out; echo err >&2; exit 0'
systemd-run --user --unit devbox-s0a-run-fail --collect -p ExecStopPost="$S fail" -- sh -c 'exit 3'
systemd-run --user --unit devbox-s0a-run-kill --collect -p ExecStopPost="$S kill" -p KillMode=control-group -- sh -c 'sleep 600 & sleep 600'
sleep 2 && systemctl --user stop devbox-s0a-run-kill.service && sleep 1
cat /tmp/devbox-s0a/ok.exit /tmp/devbox-s0a/fail.exit /tmp/devbox-s0a/kill.exit
pgrep -f 'sleep 600' || echo "no leftover sleep"
```

Expected:
- `success exited 0`
- `exit-code exited 3`
- `success killed TERM`(또는 `signal killed TERM`)
- `no leftover sleep`(control-group 종료로 자식까지 정리)

- [ ] **Step 3: "데몬" 재시작 중 실행 생존**

```bash
systemd-run --user --unit devbox-s0a-run-long --collect -p ExecStopPost="$S long" -- sleep 30
systemctl --user restart devbox-s0a@probe.service
systemctl --user is-active devbox-s0a-run-long.service
```

Expected: `active`(데몬 역할 unit 재시작과 무관). 30초 뒤 `/tmp/devbox-s0a/long.exit`에 `success exited 0`.

- [ ] **Step 4: CPU·메모리 읽기**

```bash
systemd-run --user --unit devbox-s0a-run-cpu --collect -p MemoryAccounting=yes -- sh -c 'i=0; while [ $i -lt 3000000 ]; do i=$((i+1)); done; sleep 5'
sleep 2 && systemctl --user show devbox-s0a-run-cpu.service -p CPUUsageNSec -p MemoryCurrent
```

Expected: `CPUUsageNSec`가 0보다 크고, `MemoryCurrent`가 숫자로 나온다.

---

### Task 4: tmux 서버 unit과 에이전트 scope 측정

**Files:**
- Create: `/home/jihoon/projects/devbox-spikes/s0a/tmux.conf`

**Interfaces:**
- Produces: 판정 "tmux `-D` 지원 여부, 서버 unit 유지, pane 안 scope의 자원 측정, smcup 무효화로 스크롤백 확보, capture-pane 선채움" → 01-design §4.3

- [ ] **Step 1: 버전과 `-D` 확인**

```bash
tmux -V
tmux -D -L devbox-s0a-check -f /dev/null new-session -d 2>&1 | head -1; tmux -L devbox-s0a-check kill-server 2>/dev/null; true
```

Expected: 버전을 기록한다. `-D`를 모르면 `unknown option` 류의 오류가 나온다. 이 경우 Step 2는 `Type=forking`으로 한다.

- [ ] **Step 2: devbox 전용 설정으로 서버 unit 실행**

```bash
cat > /home/jihoon/projects/devbox-spikes/s0a/tmux.conf <<'EOF'
set -g prefix None
unbind -a
set -g mouse off
set -g status off
set -ga terminal-overrides ',*:smcup@:rmcup@'
set -g window-size latest
set -g history-limit 50000
set -g allow-passthrough on
EOF
# -D 지원 시
systemd-run --user --unit devbox-s0a-tmux -p Type=exec -p EnvironmentFile=$HOME/.config/devbox-s0a/env -- tmux -D -L devbox-s0a -f /home/jihoon/projects/devbox-spikes/s0a/tmux.conf
# -D 미지원 시 대신:
# systemd-run --user --unit devbox-s0a-tmux -p Type=forking -p EnvironmentFile=$HOME/.config/devbox-s0a/env -- tmux -L devbox-s0a -f /home/jihoon/projects/devbox-spikes/s0a/tmux.conf start-server \; set -g exit-empty off
sleep 1 && tmux -L devbox-s0a new-session -d -s a1 -c "$HOME" && tmux -L devbox-s0a ls
```

Expected: 세션 `a1`이 보인다.

- [ ] **Step 3: pane 안 scope로 에이전트 대역 실행과 측정**

```bash
tmux -L devbox-s0a send-keys -t a1 "systemd-run --user --scope --unit devbox-s0a-agent-1 -- sh -c 'yes > /dev/null & sleep 300'" Enter
sleep 3 && systemctl --user show devbox-s0a-agent-1.scope -p CPUUsageNSec -p MemoryCurrent -p ActiveState
systemctl --user stop devbox-s0a-agent-1.scope && pgrep -x yes || echo "agent scope stopped"
```

Expected: `ActiveState=active`이고 CPU가 증가한다. scope를 멈추면 `agent scope stopped`가 나온다.

- [ ] **Step 4: 다시 붙기 — 스크롤백 선채움과 smcup 무효화 확인**

```bash
tmux -L devbox-s0a send-keys -t a1 'for i in $(seq 1 300); do echo line-$i; done' Enter
sleep 1 && tmux -L devbox-s0a capture-pane -p -e -S -2000 -t a1 | grep -c '^line-'
script -qc "tmux -L devbox-s0a attach -t a1" /tmp/devbox-s0a/attach.typescript </dev/null & sleep 1; kill %1 2>/dev/null
grep -c $'\e\[?1049h' /tmp/devbox-s0a/attach.typescript || echo "no alt-screen enter"
```

Expected:
- capture 결과에 `line-`이 300개 있다.
- attach 출력에 대체 화면 진입(`ESC[?1049h`)이 없다(`no alt-screen enter`). 따라서 xterm 스크롤백이 쌓인다.

- [ ] **Step 5: 읽기 전용·크기 무시 attach 지원**

```bash
script -qc "tmux -L devbox-s0a attach -f read-only,ignore-size -t a1" /dev/null </dev/null & sleep 1; kill %1 2>/dev/null; echo "exit=$?"
```

Expected: 오류 없이 붙는다(tmux 3.2 이상). 실패하면 버전 하한을 기록한다.

- [ ] **Step 6: PTY + tmux attach 경로의 입력 왕복 지연 (SC10, IR-6)**

브리지 왕복(Task 1)과 합쳐 SC10 터미널 반응 목표를 판정한다. 데몬 구간은 S1 Task 24에서 잰다.

```python
# /home/jihoon/projects/devbox-spikes/s0a/pty_latency.py
# tmux 세션 안에서 `cat`이 받은 바이트를 그대로 내보내고, attach 클라이언트 PTY에서 그 바이트가 보일 때까지를 잰다.
import os, pty, select, subprocess, time

SOCK = "devbox-s0a"
CONF = "/home/jihoon/projects/devbox-spikes/s0a/tmux.conf"
subprocess.run(["tmux", "-L", SOCK, "-f", CONF, "new-session", "-d", "-s", "lat", "-x", "120", "-y", "40",
                "stty raw -echo; cat"], check=True)
pid, fd = pty.fork()
if pid == 0:
    os.execvp("tmux", ["tmux", "-u", "-L", SOCK, "attach", "-t", "lat"])
time.sleep(1.0)
while select.select([fd], [], [], 0.3)[0]:
    os.read(fd, 65536)  # 첫 화면 그리기를 비운다

marks = ["가", "나", "다", "라"]  # 이스케이프 시퀀스와 겹치지 않는 UTF-8 바이트열
samples = []
for i in range(200):
    mark = marks[i % len(marks)].encode()
    t0 = time.perf_counter()
    os.write(fd, mark)
    buf = b""
    while mark not in buf:
        ready, _, _ = select.select([fd], [], [], 1.0)
        if not ready:
            break
        buf += os.read(fd, 65536)
    samples.append((time.perf_counter() - t0) * 1000)
    time.sleep(0.02)
samples.sort()
print({"n": len(samples), "p50_ms": round(samples[len(samples) // 2], 2),
       "p95_ms": round(samples[int(len(samples) * 0.95)], 2)})
os.kill(pid, 9)
subprocess.run(["tmux", "-L", SOCK, "kill-session", "-t", "lat"])
```

```bash
python3 /home/jihoon/projects/devbox-spikes/s0a/pty_latency.py
```

Expected: `{"n": 200, "p50_ms": …, "p95_ms": …}`. 합격 기준은 이 값 + Task 1 브리지 64B p95가 p50 10ms·p95 25ms 이하다(01-design SC10). 넘으면 원인(tmux 다시 그리기, PTY 읽기 크기)을 기록하고 01-design §4.3의 attach 방식을 다시 검토한다.

---

### Task 5: systemd 아래에서 interop과 DPAPI 왕복 (R10)

**Interfaces:**
- Produces: 판정 "데몬(systemd 서비스)이 Windows 실행 파일·DPAPI를 쓸 수 있는가" → 01-design §4.3 Windows 대상 실행, §6.3 키 풀기 경로

- [ ] **Step 1: 사용자 세션과 systemd 서비스에서 각각 실행**

```bash
cmd.exe /c ver
systemd-run --user --wait --pipe --collect -p EnvironmentFile=$HOME/.config/devbox-s0a/env -- /mnt/c/Windows/System32/cmd.exe /c ver
systemd-run --user --wait --pipe --collect -p EnvironmentFile=$HOME/.config/devbox-s0a/env -- sh -c 'env | grep -E "^WSL_INTEROP|^WSL_DISTRO_NAME" || echo "no WSL_INTEROP env"'
```

Expected: 첫 줄은 Windows 버전을 출력한다. 둘째·셋째 결과를 그대로 기록한다. 실패하면 오류 문장을 기록한다(예: `UtilBindVsockAnyPort`).

- [ ] **Step 2: 실패하면 interop 소켓을 명시해 재시도**

```bash
ls -1 /run/WSL/ 2>/dev/null | head
SOCK=$(ls -1t /run/WSL/*_interop 2>/dev/null | head -1)
systemd-run --user --wait --pipe --collect -p Environment=WSL_INTEROP=$SOCK -- /mnt/c/Windows/System32/cmd.exe /c ver
```

Expected: 결과를 기록한다. 이 방식이 되면, 데몬이 시작할 때 가장 최근 `/run/WSL/*_interop`를 찾아 쓰는 방법을 설계에 적는다. 단, 해당 세션이 끝나면 소켓이 사라질 수 있다는 한계도 함께 적는다.

- [ ] **Step 3: DPAPI 왕복**

```bash
cat > /tmp/devbox-s0a/dpapi.ps1 <<'EOF'
Add-Type -AssemblyName System.Security
$in = [Console]::In.ReadToEnd().Trim()
$parts = $in.Split(':')
$bytes = [Convert]::FromBase64String($parts[1])
if ($parts[0] -eq 'protect') { $o = [Security.Cryptography.ProtectedData]::Protect($bytes, $null, 'CurrentUser') }
else { $o = [Security.Cryptography.ProtectedData]::Unprotect($bytes, $null, 'CurrentUser') }
[Console]::Out.Write([Convert]::ToBase64String($o))
EOF
KEY=$(head -c 32 /dev/urandom | base64 -w0)
PS1W=$(wslpath -w /tmp/devbox-s0a/dpapi.ps1)
SEALED=$(echo "protect:$KEY" | systemd-run --user --wait --pipe --collect -p EnvironmentFile=$HOME/.config/devbox-s0a/env -- powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PS1W")
OPENED=$(echo "unprotect:$SEALED" | systemd-run --user --wait --pipe --collect -p EnvironmentFile=$HOME/.config/devbox-s0a/env -- powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PS1W")
[ "$KEY" = "$OPENED" ] && echo "DPAPI roundtrip OK" || echo "DPAPI roundtrip FAILED"
time (echo "unprotect:$SEALED" | powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PS1W" >/dev/null)
```

Expected: `DPAPI roundtrip OK`와 소요 시간(1초 안팎 예상). 실패하면 §6.3의 대체 경로(Windows 앱이 풀어 `secrets.unlock`)를 기본으로 바꾼다.

---

### Task 6: 에이전트 hook 주입 (R5)

**Files:**
- Create: `/tmp/devbox-s0a/hooks/claude-settings.json`
- Create: `/tmp/devbox-s0a/hooks/record.sh`

**Interfaces:**
- Produces: Claude `--settings` hook 이벤트 이름·stdin 필드, Codex `-c notify` 인자 형식, Codex 승인 대기 화면의 출력 패턴 → 01-design §9.2, S1 Task "hook 주입"

- [ ] **Step 1: 기록 스크립트와 Claude 설정**

```bash
mkdir -p /tmp/devbox-s0a/hooks
cat > /tmp/devbox-s0a/hooks/record.sh <<'EOF'
#!/bin/sh
# usage: record.sh <source> [args...]; stdin is the hook JSON (claude) or empty (codex)
ts=$(date +%s%3N)
{ printf '{"ts":%s,"source":"%s","agent":"%s","args":' "$ts" "$1" "${DEVBOX_AGENT_ID:-none}"
  shift; printf '%s' "$*" | python3 -c 'import json,sys;print(json.dumps(sys.stdin.read()),end="")'
  printf ',"stdin":'; python3 -c 'import json,sys;print(json.dumps(sys.stdin.read()),end="")'
  printf '}\n'; } >> /tmp/devbox-s0a/hooks/events.jsonl
EOF
chmod +x /tmp/devbox-s0a/hooks/record.sh
cat > /tmp/devbox-s0a/hooks/claude-settings.json <<'EOF'
{
  "hooks": {
    "SessionStart":     [{ "matcher": "", "hooks": [{ "type": "command", "command": "/tmp/devbox-s0a/hooks/record.sh claude-SessionStart" }] }],
    "UserPromptSubmit": [{ "matcher": "", "hooks": [{ "type": "command", "command": "/tmp/devbox-s0a/hooks/record.sh claude-UserPromptSubmit" }] }],
    "Notification":     [{ "matcher": "", "hooks": [{ "type": "command", "command": "/tmp/devbox-s0a/hooks/record.sh claude-Notification" }] }],
    "Stop":             [{ "matcher": "", "hooks": [{ "type": "command", "command": "/tmp/devbox-s0a/hooks/record.sh claude-Stop" }] }]
  }
}
EOF
```

- [ ] **Step 2: Claude를 tmux 안 대화형으로 실행**

```bash
mkdir -p /tmp/devbox-s0a/work && cd /tmp/devbox-s0a/work && git init -q .
tmux -L devbox-s0a new-session -d -s claude1 -c /tmp/devbox-s0a/work "DEVBOX_AGENT_ID=a-claude claude --settings /tmp/devbox-s0a/hooks/claude-settings.json"
sleep 8 && tmux -L devbox-s0a send-keys -t claude1 "Run the shell command: ls -la" Enter
sleep 20 && tmux -L devbox-s0a capture-pane -p -t claude1 | tail -20 > /tmp/devbox-s0a/claude-wait.txt
tmux -L devbox-s0a send-keys -t claude1 Escape && sleep 2 && tmux -L devbox-s0a send-keys -t claude1 "/exit" Enter
cat /tmp/devbox-s0a/hooks/events.jsonl
```

Expected: 다음을 확인하고 기록한다.
- `claude-SessionStart`(stdin에 `session_id`)
- `claude-UserPromptSubmit`
- `claude-Notification`(stdin의 `notification_type`이 권한 요청 계열)
- `agent` 필드가 `a-claude`(환경 변수 상속)

권한을 거부했으므로 `Stop`은 생길 수도 있고 없을 수도 있다. 두 경우 모두 기록한다. 사용자의 `~/.claude/settings.json`이 바뀌지 않았는지 `git diff --no-index` 대신 수정 시각으로 확인한다.

- [ ] **Step 3: Codex notify를 tmux 안 대화형으로 실행**

```bash
tmux -L devbox-s0a new-session -d -s codex1 -c /tmp/devbox-s0a/work "DEVBOX_AGENT_ID=a-codex codex -c 'notify=[\"/tmp/devbox-s0a/hooks/record.sh\",\"codex-notify\"]'"
sleep 8 && tmux -L devbox-s0a send-keys -t codex1 "Reply with the single word: pong" Enter
sleep 30 && cat /tmp/devbox-s0a/hooks/events.jsonl | grep codex-notify
tmux -L devbox-s0a send-keys -t codex1 "Create a file named hello.txt containing hi" Enter
sleep 20 && tmux -L devbox-s0a capture-pane -p -t codex1 | tail -25 > /tmp/devbox-s0a/codex-wait.txt
tmux -L devbox-s0a send-keys -t codex1 Escape C-c && sleep 1 && tmux -L devbox-s0a kill-session -t codex1
```

Expected: 다음을 기록한다.
- 첫 지시 뒤 `codex-notify` 이벤트가 생기는지. `args` 마지막 값이 `agent-turn-complete` JSON인지.
- `codex-wait.txt`: 파일 쓰기 승인을 기다리는 화면의 문구. S1 대체 감지기의 패턴이 된다.
- `~/.codex/config.toml`이 바뀌지 않았는지(수정 시각).

---

### Task 7: 사람이 재야 하는 측정 → S0 확인으로 옮김

`wsl --shutdown`처럼 에이전트가 할 수 없는 측정은 여기서 사용자를 기다리지 않는다(00-roadmap §2.2). S0b 끝의 S0 확인(03b Task 24 Step 4)에서 실제 앱으로 함께 잰다.

| 원래 측정 | S0 확인에서 |
|---|---|
| R1 앱 연결 중 30분 유휴 뒤에도 유지 | 설치한 앱을 트레이에 둔 채 30분 → `systemctl --user is-active devbox@prod`가 `active`, `wsl.exe -l -v`에서 Running |
| R1 앱 종료 뒤 멈춤 | 하지 않는다. 앱을 끄면 WSL이 멈출 수 있다는 것은 설계가 이미 전제하고 안내한다(01-design §4.1) |
| R2 `wsl --shutdown` 뒤 복귀와 첫 응답 시간 | S0 확인 4번(다시 연결 배너가 사라질 때까지 걸린 시간을 적음) |
| v0.9.0 에이전트 시작 시간 비교 | 하지 않는다. S1의 SC3(2초 안 반영) 절대 기준으로 대신한다 |

`09-s0a-results.md`의 해당 행은 "S0 확인에서 채움"으로 두고, S0 확인 뒤 S0b 브랜치에서 채운다.

---

### Task 8: 결과 문서·판정·설계 반영·정리

**Files:**
- Create: `docs/superpowers/plans/2026-10-08-devbox-v1/09-s0a-results.md`
- Modify: `docs/superpowers/plans/2026-10-08-devbox-v1/01-design.md` (불합격 항목이 있을 때만)
- Modify: 판정이 바뀐 가정을 코드로 적은 계획 과제(Step 2의 두 번째 표, 불합격 항목이 있을 때만)

- [ ] **Step 0: 계획 원본 worktree로 가기**

```bash
cd /home/jihoon/projects/devbox && git fetch origin docs/v1-plan
test -d ../devbox-wt/docs-v1-plan || git worktree add ../devbox-wt/docs-v1-plan docs/v1-plan
cd ../devbox-wt/docs-v1-plan && git pull --ff-only   # 이하 Step 1–5의 파일 경로는 이 worktree 기준
```

- [ ] **Step 1: 결과 문서 작성(양식 그대로)**

```markdown
# S0a 구조 확인 결과

- 측정일: YYYY-MM-DD · 환경: Windows 11 <빌드>, WSL <wsl --version 출력 첫 줄>, systemd <버전>, tmux <버전>, claude <버전>, codex <버전>

| 항목 | 합격 기준 | 측정값 | 판정 |
|---|---|---|---|
| R3 브리지 왕복 64B p50 | ≤ 5ms | | |
| R3 처리량 | ≥ 20 MiB/s | | |
| R3 바이너리 8MiB 일치 | true | | |
| R2 socket activation | 첫 연결에 서비스 기동 | | |
| R13 EnvironmentFile로 사용자 도구 찾기 | claude·codex·pnpm·cargo 경로 출력 | | |
| 실행 결과 기록(ExecStopPost) | ok/fail/kill 세 줄 | | |
| 데몬 재시작 중 실행 생존 | active | | |
| tmux `-D` 지원 | 지원/미지원(대체 기록) | | |
| tmux 서버 unit 유지 + scope 측정 | CPU 증가·정지 시 정리 | | |
| 대체 화면 무효화 | ESC[?1049h 없음 | | |
| read-only,ignore-size attach | 오류 없음 | | |
| PTY + tmux 입력 왕복(SC10, + 브리지 p95) | p50 ≤ 10ms·p95 ≤ 25ms | | |
| `systemctl --user is-system-running` | `running` 또는 `degraded`(둘 다 정상으로 봄, IR-9) | | |
| R10 systemd 아래 interop | cmd.exe ver 출력 | | |
| R10 DPAPI 왕복(systemd 아래) | OK | | |
| R5 Claude hook(`--settings`) | SessionStart·UserPromptSubmit·Notification 기록, 사용자 설정 불변 | | |
| R5 Codex notify(`-c`) | agent-turn-complete JSON, 사용자 설정 불변 | | |
| R1 유휴 30분 유지 | WSL·데몬 유지 | S0 확인에서 채움 | |
| R2 shutdown 뒤 복귀 | 다시 연결 + 걸린 시간 | S0 확인에서 채움 | |

## 설계 반영
- (불합격·대체 경로가 생긴 항목과 01-design에서 고친 절)

## hook 세부(S1 입력)
- Claude Notification stdin 예시: ...
- Codex notify 인자 예시: ...
- Codex 승인 대기 화면 문구(대체 감지 패턴): ...
```

- [ ] **Step 2: 판정에 따른 설계 반영**

| 결과 | 01-design에 할 일 |
|---|---|
| R3 불합격 | §5.4에 WSL localhost TCP + 토큰 경로 추가, §5.1 TCP 예외에 추가 |
| `tmux -D` 미지원 | §4.3을 `Type=forking` + `exit-empty off`로 확정 |
| R10 interop 불가(Step 2로도 불가) | §4.3 Windows 대상 실행·§6.3 키 풀기를 "Windows 앱이 대신 실행(브리지 요청)"으로 확정 |
| R10은 되지만 DPAPI 1초 초과 | 키 풀기를 데몬 시작 때 한 번으로 고정(이미 설계와 같음), 문서에 시간 기록 |
| Claude `--settings` hook 미동작 | §9.2를 "사용자 설정에 한 번 설치 + 기존 hook 연쇄"로 변경 |
| Codex notify 미동작 | §9.2 Codex를 tmux 출력 감지만으로 변경 |
| R1 유휴 유지 실패 | §4.1에 `instanceIdleTimeout=-1` 안내를 기본 권장으로 격상, 첫 실행에서 사용자 확인 |
| SC10 터미널 왕복 미달 | 원인을 기록하고 §4.3 attach 방식(PTY 읽기 크기, tmux 다시 그리기)을 고친다. 고친 방식을 S1 Task 4에 반영 |

**상세 계획도 함께 고친다(IR-10).** 위 표로 01-design을 고쳤으면, 같은 가정을 코드 수준으로 적은 계획의 해당 과제도 같은 PR에서 고친다. 01-design만 바뀌고 계획 코드가 옛 가정으로 남으면 S0b·S1 하위 에이전트가 옛 가정대로 구현한다.

| 바뀐 판정 | 고칠 계획 과제 |
|---|---|
| R3(브리지 → TCP) | 03 Task 14(브리지), 03b Task 23(Tauri 연결) |
| tmux `-D` | 03 Task 15(setup의 unit 파일), 04 Task 3(tmux 서버) |
| R10(interop) | 03 Task 15(doctor), 05 Task 11·12(이 둘은 S2 상세화 때 반영) |
| R5(hook) | 04 Task 8·10(hook 이벤트, 출력 기반 감지) |
| SC10 | 04 Task 4(attach 스트림) |

- [ ] **Step 3: ADR 0017 초안을 결과 문서 끝에 붙임** (S0b Task 1이 저장소로 옮긴다)

```markdown
## ADR 0017 초안 — 재구축: 단일 앱 + WSL 데몬
상태: 채택 · 기록일: <날짜>
맥락: 사용 빈도 1위인 에이전트 기능(앱을 닫아도 유지되는 세션, hook 상태 감지, 알림, 재개)이 v0.9.0 구조에 없었고, 리뷰가 확정한 결함 약 20건 중 에이전트·터미널 결함은 0건이었다. v0.9.0의 네 제품·설치 세대·제품 간 연결·Windows 측 프로세스 처리·WSL helper 중계 전용 코드와 엔진 안 Windows·WSL 이중 경로 파일이 전체 약 49.7만 줄의 약 25%(파일 단위 하한)였고, `.github`·`scripts`가 5.6만 줄이었다. 사용자는 데이터 이전 없이 WSL 중심 재구축을 결정했다.
결정: docs/superpowers/plans/2026-10-08-devbox-v1/01-design.md 를 원장으로 하는 단일 Windows 앱 + WSL Linux 데몬 구조로 다시 짓는다. S0a 측정 결과는 09-s0a-results.md에 있다.
결과: ADR 0001·0002·0003·0004·0007·0009·0010·0011·0012·0014·0015를 대체한다. 0005·0006·0008·0013·0016은 01-design §13의 형태로 유지한다.
```

- [ ] **Step 4: 정리**

```bash
systemctl --user stop 'devbox-s0a*' 2>/dev/null; systemctl --user disable devbox-s0a@probe.socket 2>/dev/null
rm -f ~/.config/systemd/user/devbox-s0a@.socket ~/.config/systemd/user/devbox-s0a@.service
systemctl --user daemon-reload && systemctl --user reset-failed
tmux -L devbox-s0a kill-server 2>/dev/null; tmux -L devbox-s0a-check kill-server 2>/dev/null
rm -rf /tmp/devbox-s0a ~/.config/devbox-s0a /run/user/$(id -u)/devbox-s0a*.sock
systemctl --user list-units --all 'devbox-s0a*' --no-legend; ls ~/.config/systemd/user | grep s0a; true
```

Expected: 마지막 두 명령의 출력이 비어 있다. `/home/jihoon/projects/devbox-spikes/s0a`는 S0b 이후 참고용으로 남겨 두고, S1 완료 후 지운다.

- [ ] **Step 5: 커밋·push(PR 없음)**

```bash
cd /home/jihoon/projects/devbox-wt/docs-v1-plan
# PROGRESS.md: S0a 행 완료, PR "S0a" 행, 현재 위치(다음: S0b)를 고친다
git add docs && git commit -m "docs(plan): record S0a results and design updates"
git push origin docs/v1-plan
```

계획 문서만 바뀌므로 PR·CI를 거치지 않는다. 이 내용은 S0b PR과 함께 main에 들어간다.

- [ ] **Step 6: 사용자 보고**

판정 표를 사용자에게 보고한다. **모두 합격이면 기다리지 않고 S0b로 넘어간다**(00-roadmap §2.2). 불합격 항목 때문에 01-design의 구조를 바꿔야 하면(Step 2 표) 멈추고 사용자에게 묻는다(§3.2).
