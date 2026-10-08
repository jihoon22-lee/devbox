# S2 실행·관찰 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

- 상태: 계획(과제 수준) · 미착수 · 시작 조건: S1 완료([04b](04b-s1-app.md) 상태 줄)
- 상세화 수준: 과제·파일·인터페이스·핵심 테스트까지 적었다. **Task 1에서 실제 S0b·S1 코드에 맞춰 단계별 코드까지 상세화한 뒤 실행한다.**

**Goal:** 프로젝트의 작업·서비스·예약을 devbox가 띄우고 지켜보게 한다.
- 대상: 실시간 로그(여러 소스 병합), 포트·프로세스·컨테이너, 문제(file:line), WSL 자원
- 함께 하는 일: 비밀 값 주입, Windows 대상 실행, 시작 구성의 서비스, MCP 실행·로그 도구

S2가 끝나면 **전환점**이다. 일상의 에이전트·실행·터미널(A·B·C)을 새 빌드로 옮긴다.

**Architecture:**
- `crates/runtime`:
  - 정의를 만든다: `.devbox/devbox.toml` + DB의 "이 PC에만" 정의를 병합한다.
  - 실행은 `systemd-run --user`로 transient unit을 띄운다. 그 안의 `devbox exec` 래퍼가 줄별 로그와 종료 결과를 남긴다.
  - 재시작은 systemd(`Restart=`)만 한다.
- `crates/observe`: `/proc`·`/proc/net/tcp*`·PSI·`df`와 Docker CLI를 읽는다.
- `crates/secrets`:
  - 데이터 키는 DPAPI로 봉인한다(Q1 확정). 데몬이 interop으로 풀고, 안 되면 Windows 앱이 대신 푼다.
  - XChaCha20-Poly1305로 값을 저장한다.
  - 실행 래퍼에는 환경 변수로만 넘긴다.
- 화면: 실행 섹션(목록 + 가상 스크롤 로그)과 하단 패널의 로그·문제 탭.

**Tech Stack:** S1 스택 + `cron`(이식한 v0.9.0 `cron.rs`), `chrono-tz`, `chacha20poly1305`, `zeroize`, `notify`(inotify, toml 감시), `@tanstack/react-virtual`.

**Spec:** [01-design.md](01-design.md) §2.1 실행·§4.3·§4.4·§6.3·§7·§9 `runtime`·`observe`·`secrets`·§9.1-10, [09-s0a-results.md](09-s0a-results.md)(R10 interop 판정)

## Global Constraints

- S0b·S1의 Global Constraints를 따른다.
- 작업 위치: S2 전체가 브랜치 `v1/s2-runs`(origin/main에서) 하나와 worktree `../devbox-wt/v1-s2-runs` 하나다. Task 1(상세화)이 이 브랜치의 첫 커밋이다. PR은 Task 16에서 한 번 연다(00-roadmap §3).
- 실행 unit 이름은 `devbox-<인스턴스>-run-<runId>`다. tmux·데몬 unit과 섞지 않는다. 테스트는 `devbox-test-<난수>.slice` 아래에서만 띄우고 끝나면 slice를 멈춘다(01-design §4.3).
- 재시작 주체는 systemd 하나다. 데몬은 재시작을 직접 하지 않는다. 표시와 알림만 한다.
- 실행 결과의 원장은 `runs/<runId>.exit` 파일이다. transient unit은 끝나면 정리되므로 unit 상태를 결과로 쓰지 않는다.
- 비밀 평문은 DB·로그·파일·프로세스 인자·MCP 응답·RPC 결과에 나가지 않는다. 실행에는 `systemd-run --setenv`가 아니라 래퍼의 표준 입력으로 넘긴다. 이유: 인자는 `ps`·`systemctl show`에 보인다.
- 로그 줄 형식은 `<RFC3339 밀리초>\t<o|e|s>\t<줄>`이다(`s` = devbox 표시 줄). 64 MiB를 넘으면 앞을 자르고 표시 줄을 남긴다.
- 사용자 서비스·Docker·방화벽·네트워크를 테스트 때문에 바꾸지 않는다. Docker 관련 테스트는 `docker` CLI가 없거나 데몬이 꺼져 있으면 건너뛰고, 컨테이너를 만들거나 지우지 않는다(목록·로그 읽기만, 시작·중지는 단위 테스트에서 명령 조립만 검사).

## Review Focus

| 상황 | 기대 동작 | 시험 위치 |
|---|---|---|
| 서비스가 시작 직후 계속 죽음 | systemd 한도에 걸려 `failed`가 되고, 알림 한 번. 수동 [재시작]은 한도를 초기화(`reset-failed`) | Task 4 |
| 데몬이 꺼진 사이 실행이 끝남 | 데몬 시작 시 `.exit` 파일로 결과를 맞추고, 끝난 실행이 "실행 중"으로 남지 않음 | Task 4 |
| 래퍼가 `kill -9`로 죽음 | `ExecStopPost`가 쓴 결과로 "강제 종료"가 보이고 로그는 그때까지 남음 | Task 3 |
| 비밀을 쓰는 작업을 실행 | 자식 환경에는 값이 있고, `ps`·`systemctl show`·로그·`runtime.get` 결과에는 없음 | Task 11 |
| 일광 절약 시간 전환 날의 예약 | 없는 시각은 건너뛰고 겹치는 시각은 한 번만(이식한 `cron.rs` 테스트) | Task 5 |
| "재시작 안 함" 서비스가 죽음, `::1`에서만 수신하는 개발 서버(AR-F02) | 다시 띄우지 않음. `::1` 수신도 "준비됨" | Task 4·8 |
| 작업을 만든 직후 목록 조회만 실패(AR-F04) | 대화상자는 수정 모드, 다시 저장해도 새로 만들지 않음 | Task 13 |

---

## 작업 묶음

S2 전체가 브랜치 `v1/s2-runs` 하나, PR 하나다(00-roadmap §3). 작업 위치는 worktree `../devbox-wt/v1-s2-runs`다.
묶음은 그 안의 중간 지점이다. 묶음이 끝나면 로컬 검사(`pnpm check`, 그 시점에 없으면 있는 검사만) → `PROGRESS.md` 묶음 행 갱신 커밋 → push 한다. PR·CI는 없다. 세션 인계 지점이 된다.
PR은 마지막 묶음이 끝난 뒤 한 번 열고, 그 CI가 S2 전체를 한 번 검사한다.

| 묶음 | 과제 | 끝나면 |
|---|---|---|
| 상세화 | Task 1 | 커밋(이 브랜치의 첫 커밋) |
| O | Task 2–6 | push |
| P | Task 7–10 | push |
| Q | Task 11–12 | push |
| R | Task 13–15b | push |
| 마무리 | Task 16 | **S2 PR 열기** → CI·설치 파일 → 사용자 확인 → rebase 머지 |

---

### Task 1: 실제 코드에 맞춘 상세화

**Files:**
- Modify: 이 문서(`05-s2-runs.md`)

- [ ] **Step 1:** S0b·S1에서 실제로 만들어진 이름을 확인해 이 계획의 Interfaces를 고친다. `PROGRESS.md` §5(계획과 달라진 점)를 먼저 읽는다.
  - `method!`·`Router`·`Ctx::open_stream`·`Db`·`Hub`·`TestDaemon` 보조 함수
  - 화면 쪽 `useRpcQuery`·`SectionLayout`·`useBottomTabs`·`TerminalView`
  - `09-s0a-results.md`의 R10(interop) 판정과 S1 실사용에서 나온 수정 사항
- [ ] **Step 2:** Task 2–16 각각을 `writing-plans` 형식(실패 테스트 코드 → 실패 확인 → 구현 코드 → 통과 → 커밋)으로 펼친다. 아래 "핵심 테스트"와 [00-roadmap §7](00-roadmap.md) 추적표에서 S2에 걸린 시험(AR-F02·F04·F14·F15)과 입력값을 그대로 포함한다.
- [ ] **Step 3:** `09-s0a-results.md`의 R10·DPAPI 실험 결과로 Task 11의 키 풀기 경로를 정한다. 데몬 interop이 되면 데몬이 직접 풀고, 안 되면 `secrets.unlock` 경로만 쓴다(키 방식은 Q1에서 DPAPI로 확정).
- [ ] **Step 4:** 커밋·PR(문서만).

```bash
git add docs && git commit -m "docs(plan): detail S2 against the S1 code"
```

---

### Task 2: 실행 정의 — toml + 이 PC 정의 병합

**Files:**
- Create: `crates/runtime/{Cargo.toml,src/lib.rs,src/model.rs,src/defs.rs,src/store.rs}`
- Modify: `crates/projects/src/config.rs`(S1의 `raw_tasks`·`raw_services`·`raw_schedules`를 runtime이 해석하도록 공개)

**Interfaces:**
- `TaskDef { id, command, cwd?, env: BTreeMap, target: Linux|Windows, depends_on: Vec<id>, worktree?: "main"|"<agent id>", secrets: Vec<name> }`
- `ServiceDef { 위 + restart: No|OnFailure|Always, restart_sec, start_limit: { burst, interval_sec }, ports_hint: Vec<u16>, ready_timeout_sec: u32(기본 60) }`
  - health 검사 필드는 없다. 재시작 원인은 프로세스 종료뿐이다(01-design §9.1-17, AR-F02).
  - `depends_on`에 서비스가 있으면 그 서비스가 "준비됨"(Task 8)이 될 때까지 기다린 뒤 다음을 시작한다. `ports_hint`가 비어 있는 서비스는 시작 즉시 준비됨이다.
- `ScheduleDef { id, task, cron, tz?, missed: Skip|RunOnce }`
- `Definition { source: ProjectFile|ThisPc, def }`
- `merge(file_defs, local_defs) -> (Vec<Definition>, Vec<Shadowed>)`: 같은 ID는 프로젝트 파일이 이긴다. 가려진 정의는 화면에 출처와 함께 보인다.
- 검증 오류 `RuntimeError::DefInvalid { id, reason }`: 순환 의존, 없는 의존, 빈 명령, 잘못된 cron
- 메서드:
  - `runtime.list{ projectId }` → `{ rev, defs, runs: 최근 실행 요약 }`
  - `runtime.define{ projectId, def, saveTo: project|thisPc }`(mutation): 프로젝트 파일에 쓰면 `toml_edit`로 해당 표만 바꾸고 주석·순서를 보존한다.
  - `runtime.undefine{ projectId, id }`(mutation, always)
- 프로젝트 `.devbox/devbox.toml` 변경 감시(`notify`, 300ms 디바운스) → `runtime.changed`

**핵심 테스트:**
- `merge_prefers_project_file_and_reports_shadowed`
- `rejects_dependency_cycles_with_the_cycle_path`(`a → b → a`의 경로를 오류에 담음)
- `define_into_project_file_preserves_comments`(주석과 다른 표가 그대로)
- `watch_reloads_on_external_edit`(파일을 고치면 `runtime.changed` 수신)

---

### Task 3: `devbox exec` 래퍼와 `devbox run-exited`

**Files:**
- Create: `crates/cli/src/exec.rs`, `crates/cli/src/run_exited.rs`, `crates/runtime/src/logfile.rs`
- Modify: `crates/cli/src/main.rs`

**Interfaces:**
- `devbox exec --run <runId> [--secrets-stdin] -- <명령...>`
  - 자식을 띄운다. 비밀이 있으면 표준 입력의 `NAME=값\0` 목록을 읽어 자식 환경에만 넣는다.
  - stdout·stderr를 줄 단위로 `runs/<runId>.log`에 쓴다(`logfile::Writer`, 64 MiB 회전).
  - 끝나면 `runs/<runId>.exit`(JSON `{ code, signal?, endedMs, by: "wrapper" }`)를 쓴다.
  - 데몬 소켓으로 `runtime.exited`를 보낸다(실패해도 무시).
  - SIGTERM을 받으면 자식 프로세스 그룹에 전달한다.
- `devbox run-exited --run <runId>`(ExecStopPost):
  - `.exit` 파일이 없을 때만 `$SERVICE_RESULT`·`$EXIT_CODE`·`$EXIT_STATUS`로 `{ by: "systemd" }` 결과를 쓴다.
- `logfile::Writer::write_line(stream, bytes)`
  - 시각은 받은 시각이다.
  - 줄이 1 MiB를 넘으면 자르고 `…(잘림)`을 붙인다.
  - UTF-8이 아니면 손실 변환한다.
- `logfile::read_range(path, from_offset, max_lines) -> (lines, next_offset)`

**핵심 테스트:**
- `wrapper_records_lines_with_stream_markers_and_exit_code`: `sh -c 'echo out; echo err >&2; exit 3'` → 두 줄(`o`·`e`), exit 3
- `secrets_from_stdin_reach_the_child_but_not_argv`: 자식이 `$TOKEN`을 출력해 값이 보이는지 보고, `/proc/<pid>/cmdline`에는 없는지 확인
- `log_rotation_keeps_the_tail_and_marks_truncation`: 상한을 작게 주입해 확인
- `run_exited_does_not_overwrite_wrapper_result`
- `run_exited_records_killed_wrapper`: 래퍼를 `kill -9` → `ExecStopPost` 흉내 → `by: systemd`, `signal: KILL`

---

### Task 4: 실행 시작·중지·재시작·맞춤

**Files:**
- Create: `crates/runtime/src/launch.rs`, `crates/runtime/src/reconcile.rs`, `crates/runtime/src/service.rs`
- Create: `crates/core/src/systemd.rs`(S0b `crates/cli/src/setup.rs`의 `systemctl` 호출 helper를 이리로 옮기고 `run_transient`·`stop`·`reset_failed`·`show`를 더한다. 오류는 `CoreError`)

**Interfaces:**
- `Run { id, projectId, defId, kind: Task|Service|Scheduled, state: Starting|Running|Restarting|Exited|Failed|Stopped, exit?, startedMs, endedMs?, restarts, unit }`
- `launch::unit_args(def, run_id, paths, slice?) -> Vec<String>`
  - 순수 함수다. 단위 테스트로 인자 배열을 고정한다.
  - 속성: `--unit`, `--collect`, `WorkingDirectory`, `EnvironmentFile`, `Environment=PYTHONUNBUFFERED=1 FORCE_COLOR=1 DEVBOX_PORT=…`, `KillMode=control-group`, `MemoryAccounting=yes`, `ExecStopPost=<devbox> run-exited --run <id>`
  - 서비스는 `Restart`·`RestartSec=1s`·`RestartSteps=5`·`RestartMaxDelaySec=30s`·`StartLimitBurst`·`StartLimitIntervalSec`를 더한다(빠른 반복 종료의 간격이 1초 → 30초로 늘어남, AR-F14, IR-5). `restart = "no"`면 `Restart=no`다.
- 메서드:
  - `runtime.start{ projectId, id, worktree? }`(mutation): 의존 작업을 순서대로 실행한다. 앞 단계가 실패하면 멈춘다.
  - `runtime.stop{ runId }`(mutation, reversible)
  - `runtime.restart{ runId }`(mutation): 실패 상태면 `reset-failed` 뒤 시작한다.
  - `runtime.exited{ runId, code, signal? }`(내부)
  - `runtime.runs{ projectId?, limit }`(query)
- 데몬이 시작할 때 맞춘다: `systemctl --user list-units 'devbox-<i>-run-*' --output=json` + `.exit` 파일 → 상태를 갱신한다.
- 서비스 재시작은 `systemctl show -p NRestarts,ActiveState,Result`를 실행 이벤트 때와 10초 간격(서비스가 있을 때만)으로 읽어 `restarts`를 갱신한다.
- 한도 초과(`Result=start-limit-hit`)는 `failed`이고 `notify.raise`(Task 14)를 한 번 부른다.
- 시작 구성의 `services`(S1에서 무시하던 것)를 이제 `projects.launch`에서 시작한다.

**핵심 테스트(L2, 실제 systemd user):**
- `task_runs_to_completion_and_reports_exit`
- `service_restarts_on_failure_until_start_limit_then_fails_once`: `exit 1` 서비스, `StartLimitBurst=3`
- `manual_restart_resets_the_start_limit`
- `restart_no_service_is_not_restarted_after_exit`: `restart = "no"` 서비스가 `exit 1` → 10초 뒤 `NRestarts=0`, 상태 `Failed`(AR-F02b 입력)
- `daemon_restart_reconciles_finished_and_running_runs`: 실행 중 데몬 재시작 + 그사이 끝난 작업
- `depends_on_runs_in_order_and_stops_on_failure`
- `dependent_service_waits_for_readiness_then_times_out`: 앞 서비스가 `ports_hint` 포트를 3초 뒤에 열면 뒤 서비스는 그 뒤에 시작. 포트를 열지 않으면 `ready_timeout_sec`(시험에서 2초) 뒤 뒤 서비스를 시작하지 않고 `runtime.not_ready{ id }`로 실패
- 단위: `unit_args_for_service_with_restart_policy`(인자 배열 전체 비교. `RestartSteps=5`·`RestartMaxDelaySec=30s` 포함)
- systemd user 세션을 CI에서 못 쓰면(S0a 판정) L2 중 systemd 시험은 `#[ignore = "needs systemd user"]` + WSL 로컬 필수로 둔다(00-roadmap 실행 규칙).

---

### Task 5: 예약(cron)·놓친 예약·실패 알림

**Files:**
- Create: `crates/runtime/src/cron.rs`(v0.9.0 `crates/runtime-engine/src/core/cron.rs`와 테스트를 그대로 이식), `crates/runtime/src/scheduler.rs`

**Interfaces:**
- `cron::next_after(expr, tz, after) -> Option<DateTime>`(이식)
- `Scheduler`: 다음 실행 시각 하나만 잡고 `tokio::time::sleep_until`로 기다린다(폴링 없음). 정의가 바뀌면 다시 계산한다.
- 데몬 시작 시 놓친 예약:
  - `last_fired`(DB)와 지금 사이의 예약 시각이 있으면 `missed` 정책을 따른다.
  - `RunOnce`면 한 번 실행하고, `Skip`이면 "건너뜀"을 기록한다.
- 예약 실행이 실패하면 알림 `{ kind: "schedule_failed", projectId, task, runId }`
- 메서드 `runtime.schedules{ projectId }` → `{ items: [{ id, cron, next, last: { at, result } }] }`

**핵심 테스트:**
- 이식한 `cron.rs` 테스트 전부(DST 포함)
- `missed_run_once_fires_exactly_once_after_downtime`
- `missed_skip_records_without_running`
- `scheduler_recomputes_when_definitions_change`(가짜 시계)

---

### Task 6: 작업 자동 감지

**Files:**
- Create: `crates/runtime/src/detect.rs`

**Interfaces:**
- `detect(project_dir) -> Vec<Suggestion { id, command, kind: Task|Service, from: "package.json"|"Makefile"|"justfile"|"Cargo.toml"|"compose" }>`
  - `package.json`의 `scripts`: `dev`·`start`·`serve`는 서비스, 나머지는 작업. 패키지 관리자는 lockfile로 고른다(`pnpm`/`yarn`/`npm`/`bun`).
  - `Makefile` 타깃(`.PHONY`와 `^name:`), `justfile` 레시피(`just --summary`가 있으면 그것)
  - `Cargo.toml`이 있으면 `cargo build`·`cargo test`
  - `compose.yaml`/`docker-compose.yml`이 있으면 `docker compose up`(서비스)
- 메서드 `runtime.detect{ projectId }`(query)

**핵심 테스트:** 각 파일 종류의 fixture 폴더로 제안 목록 비교. 이미 정의된 ID는 제외.

```bash
# 묶음 O 끝
# 묶음 O 끝: PROGRESS.md의 묶음 O 행과 현재 위치를 고쳐 커밋한 뒤 push한다. PR·CI는 없다(00-roadmap §3)
pnpm check && git push origin v1/s2-runs
```

---

### Task 7: 로그 — 실시간 tail·검색·소스 병합·파일 tail

**Files:**
- Create: `crates/observe/{Cargo.toml,src/lib.rs,src/logs.rs,src/sources.rs}`

**Interfaces:**
- `LogSource = Run{ runId } | File{ path } | Container{ id }`
- `logs.open{ sources: LogSource[], from: "tail"|{ offset }, filter?: { text?, regex?, streams? } }`(stream)
  - 각 소스를 읽어 시각 순으로 합친다. 소스별로 이미 시각 정렬이므로 k-way 병합을 쓴다.
  - 스트림 본문은 줄 단위 JSON Lines다: `{ src, ts, stream, text }`. 줄은 64KiB 조각으로 묶어 보낸다.
  - 실행 로그는 inotify로 추가분을 읽는다. 회전(앞 자르기)을 감지하면 처음부터 다시 연다.
  - 파일 소스는 등록 프로젝트·vault·`/var/log` 아래만 허용한다(§12 쓰기 범위와 달리 읽기 범위는 여기서 정함).
  - 컨테이너 소스는 `docker logs -f --timestamps`다.
- `logs.search{ sources, query, regex, before?, limit }`(query): 지난 줄을 거꾸로 찾는다.
- 줄 시각이 없는 파일 소스는 읽은 시각을 쓴다.

**핵심 테스트:**
- `merges_two_sources_in_timestamp_order`
- `follows_appended_lines_and_survives_truncation`
- `filter_by_stream_and_regex`
- `file_source_outside_allowed_roots_is_rejected`
- `slow_client_pauses_reading_not_the_daemon`(신용 소진 시 다른 RPC 응답 유지, S1 Task 1 규칙 재확인)

---

### Task 8: 포트·프로세스·WSL 자원

**Files:**
- Create: `crates/observe/src/ports.rs`, `processes.rs`, `system.rs`

**Interfaces:**
- `observe.ports{}`(query) → `[{ port, proto, addr, pid?, process?, runId?, containerId?, projectId? }]`
  - `/proc/net/tcp`·`tcp6`의 LISTEN과 inode → `/proc/*/fd` 소켓 링크로 pid를 찾는다.
  - `pid`의 cgroup(`/proc/<pid>/cgroup`)으로 실행 unit을 찾고, 그것으로 `runId`를 정한다.
  - 컨테이너 포트는 `docker ps --format json`의 `Ports`로 정한다.
- `observe.processes{ projectId? }`(query): 프로젝트 경로 아래 `cwd`를 가진 프로세스를 먼저, 나머지는 CPU 순 상위 50개
- `observe.kill{ pid, signal: term|kill, expect: { startTimeMs } }`(mutation, always): 본인 UID 프로세스만 허용한다. `expect.startTimeMs`는 확인 대화상자가 보여 준 그 프로세스의 시작 시각(`/proc/<pid>/stat` 22번째 값)이다. 다르면 PID가 다른 프로세스로 재사용된 것이므로 `observe.state_changed`로 거절한다(01-design §5.2, IR-11).
- `observe.system{}`(query): `MemTotal/MemAvailable`·스왑·PSI(`/proc/pressure/{cpu,memory,io}`)·`df` 홈 파티션
- 화면은 보일 때만 3초 간격으로 조회한다(이벤트 원천 없음).
- **서비스 준비 상태(IR-4):** `observe::is_listening(port) -> bool`은 `/proc/net/tcp`·`tcp6`에서 그 포트가 LISTEN이면 참이다. 주소는 `127.0.0.1`·`::1`·`0.0.0.0`·`::`를 모두 인정한다(v0.9.0은 `localhost`를 해석하지 못해 정상 서비스를 실패로 봤다, AR-F02a). runtime은 실행 중 서비스의 `ports_hint`를 1초 간격으로 확인해 `Run.ready: bool`을 바꾸고 `runtime.changed`를 낸다. 준비됨이 된 뒤에는 확인을 멈춘다. 준비 상태는 표시와 `depends_on` 순서에만 쓰고, 재시작에는 쓰지 않는다.

**핵심 테스트:**
- `/proc` fixture 디렉터리로 `parse_net_tcp`·`socket_inode_to_pid`·`unit_from_cgroup` 단위 테스트
- `is_listening_accepts_ipv4_ipv6_loopback_and_wildcard`: fixture에 `127.0.0.1`·`::1`·`0.0.0.0`·`::` LISTEN 행 → 모두 참, `ESTABLISHED`만 있는 포트 → 거짓
- L2: 테스트가 연 TCP 리스너(127.0.0.1:0)가 목록에 pid와 함께 보임
- `kill_refuses_other_users_processes`(pid 1)
- `kill_refuses_a_reused_pid`: 실제 시작 시각과 다른 `expect.startTimeMs` → `observe.state_changed`, 프로세스 살아 있음

---

### Task 9: 문제 추출

**Files:**
- Create: `crates/observe/src/problems.rs`

**Interfaces:**
- `extract(line, project_root) -> Option<Problem { file, line, col?, severity, message, source }>`
  - 지원 형식: rustc/cargo(`--> src/x.rs:10:5`), tsc(`src/x.ts(10,5): error TS…`), eslint/biome(`file:line:col`), vitest/jest 스택, python traceback(`File "x.py", line 10`), go(`x.go:10:5:`)
  - 경로는 프로젝트 기준 상대 경로로 바꾸고, 프로젝트 밖이면 버린다.
- 실행 로그를 읽을 때 문제를 모아 `observe.problems{ projectId }`(query)에 둔다. 실행이 다시 시작되면 그 실행 출처의 문제를 비운다.
- 주제 `observe.problems.changed`
- S4에서 LSP 진단이 같은 목록에 `source: "lsp:<server>"`로 들어온다.

**핵심 테스트:** 형식마다 실제 출력 줄 fixture → `Problem` 비교. 프로젝트 밖 경로 버림. 같은 문제 중복 제거.

---

### Task 10: Docker 컨테이너

**Files:**
- Create: `crates/runtime/src/containers.rs`

**Interfaces:**
- `runtime.containers{}`(query): `docker ps -a --format json` → `{ id, name, image, state, status, ports, composeProject? }`. CLI가 없거나 데몬이 꺼져 있으면 `{ available: false, reason }`.
- `runtime.container_action{ id, action: start|stop|restart }`(mutation, `stop`은 reversible)
- 로그는 Task 7의 `Container` 소스로 본다.

**핵심 테스트:**
- 명령 조립 단위 테스트
- `docker ps` JSON fixture 해석
- 실제 Docker 시험은 두지 않는다(Global Constraints).

```bash
# 묶음 P 끝
# 묶음 P 끝: PROGRESS.md의 묶음 P 행과 현재 위치를 고쳐 커밋한 뒤 push한다. PR·CI는 없다(00-roadmap §3)
pnpm check && git push origin v1/s2-runs
```

---

### Task 11: 비밀

**Files:**
- Create: `crates/secrets/{Cargo.toml,src/lib.rs,src/crypto.rs,src/keystore.rs,src/store.rs}`
- Modify: `crates/runtime/src/launch.rs`(비밀 주입), `crates/cli/src/main.rs`(`devbox secrets export-key|import-key`)

**Interfaces:**
- `crypto::seal(key, secret_id, key_id, plaintext) -> Vec<u8>`, `open(...)`
  - XChaCha20-Poly1305, 무작위 nonce 24바이트
  - AAD는 `secret_id | key_id`
- `DpapiKeyStore`(Q1 확정, 구현은 이것 하나):
  - `create() -> (Key, SealedBlob)`: 32바이트 무작위 키를 만들고, interop `powershell.exe -NoProfile -NonInteractive`의 표준 입출력으로 DPAPI(CurrentUser) 봉인해 `keys/data.key.dpapi`에 둔다. `key_id`(키 지문)를 DB에 기록한다.
  - `unseal() -> Result<Key, SecretsError>`: 같은 경로로 푼다. R10이 실패한 환경에서는 호출하지 않고, Windows 앱이 풀어 `secrets.unlock{ key }`로 전달한다.
  - `key_id`가 다르면 `secrets.key_mismatch`다.
  - 테스트는 키 저장소를 거치지 않고 `SecretsService::with_key(key)`로 열린 키를 주입한다(트레잇을 두지 않는다).
- 메서드:
  - `secrets.list{}`: 이름·이 PC에 값 있음 여부. 값은 주지 않는다.
  - `secrets.set{ name, value }`(mutation)
  - `secrets.delete{ name }`(mutation, always)
  - `secrets.export_key{}` → 복구 문자열(Base32, 4자 묶음)
  - `secrets.import_key{ recovery }`(mutation)
  - `secrets.unlock{ key }`(Windows 앱 전용, R10 대체 경로)
- 이름 목록은 `config.toml`의 `[secrets] names = [...]`로 두 PC가 공유한다. 값은 PC별 DB다.
- 실행 주입:
  1. 정의의 `secrets`와 `{{secret:NAME}}` 참조(env 값 안)를 모은다.
  2. 래퍼 표준 입력으로 넘긴다(Task 3 `--secrets-stdin`).
  3. 값이 없으면 실행하지 않고 `runtime.secret_missing{ names }`를 돌려준다.
  4. 키가 잠겨 있으면 `secrets_locked`다.
- 메모리의 키·값은 `zeroize`한다.

**핵심 테스트:**
- `seal_open_round_trip_and_wrong_aad_fails`
- `key_mismatch_is_reported_not_silently_reset`
- `run_with_secret_hides_value_from_argv_unit_properties_logs_and_rpc`: `systemctl --user show <unit> -p Environment`·`/proc/<pid>/cmdline`·로그 파일·`runtime.runs` 결과에서 값 문자열을 검색해 없음을 확인
- `missing_secret_blocks_the_run_with_names`
- `recovery_string_restores_the_same_key_id`
- DPAPI 구현은 Windows·interop이 필요하다. 플랫폼 독립인 blob 입출력 조립만 단위 테스트하고, 실제 왕복은 Task 16 실사용 확인에서 본다.

---

### Task 12: Windows 대상 실행

**Files:**
- Create: `crates/runtime/src/windows.rs`
- Modify: `crates/core/src/config.rs`(`config.local.toml`의 `[windows] root = 'C:\dev'`)

**Interfaces:**
- 대상 폴더 `windows_dir = <root>\<프로젝트 이름>`
- `sync(project_dir, windows_dir) -> SyncReport { copied, deleted, skipped }`
  - 대상 목록: `git ls-files -co --exclude-standard -z`
  - `/mnt/c/...`로 바꾼 경로에 크기·mtime이 다른 파일만 복사한다. 목록에 없는 대상 쪽 파일은 지우지 않는다(`node_modules` 같은 Windows 쪽 산출물 보호).
- 실행:
  - `powershell.exe -NoProfile -Command "Set-Location '<windows_dir>'; <command>"`를 래퍼 안에서 실행한다.
  - interop이 안 되는 환경(R10)이면 Windows 앱에 위임한다. 앱 명령 `run_windows`가 출력을 브리지로 돌려준다.
- 화면 표시: "Windows에서 실행 · 자식 프로세스 종료는 보장하지 않음"

**핵심 테스트:**
- `sync_copies_changed_tracked_files_and_keeps_extra_target_files`(임시 폴더 두 개로)
- `windows_command_is_quoted_for_powershell`(작은따옴표 이스케이프)
- 실제 실행은 Task 16 실사용 확인

```bash
# 묶음 Q 끝
# 묶음 Q 끝: PROGRESS.md의 묶음 Q 행과 현재 위치를 고쳐 커밋한 뒤 push한다. PR·CI는 없다(00-roadmap §3)
pnpm check && git push origin v1/s2-runs
```

---

### Task 13: 실행 섹션 화면

**Files:**
- Create: `app/src/features/runs/{RunsPage,RunList,LogView,RunChips,DefineDialog,model}.tsx|ts`
- Create: `app/src/ui/VirtualList.tsx`(`@tanstack/react-virtual`), `app/src/ui/DataTable.tsx`
- Create: `app/e2e/runs.spec.ts`

**Interfaces:**
- 목록: 서비스·작업·예약·컨테이너 그룹, 각 행에 상태 점·이름·최근 결과·재시작 수
  - 행 동작: [시작][중지][재시작], 출처 표시(프로젝트 파일 / 이 PC)
  - 위쪽: [작업 추가▾](직접 · 자동 감지 제안)
- 본문 `LogView`:
  - 가상 스크롤이다. 열은 시각·스트림·본문(ANSI 색)이다.
  - 맨 아래에 붙어 있으면 따라가고, 위로 올리면 멈추고 [맨 아래로] 버튼을 띄운다.
  - 검색(`logs.search`)과 필터(텍스트·정규식·스트림)가 있다.
  - [소스 추가]로 다른 실행·파일·컨테이너를 병합한다.
  - 줄의 `file:line`은 링크다. S2에서는 외부 편집기, S4부터는 편집기로 연다.
- 위쪽 칩: 포트(누르면 브라우저로 열기), CPU·메모리, 경과 시간
- `DefineDialog`: 명령·작업 폴더·환경 변수(`KeyValueEditor`)·비밀 참조·재시작 정책·[프로젝트 파일에 저장 | 이 PC에만]
- `model.ts`의 `ansiToSpans(text)`: SGR 16·256·truecolor·굵게·밑줄. 그 밖의 제어열은 지운다.

- 생성과 조회 실패의 분리(01-design §8.6 규칙 2, AR-F04): `DefineDialog`는 `runtime.define` 응답으로 바로 "수정" 모드(생성된 ID)로 바뀐다. 이어지는 `runtime.list` 조회가 실패하면 목록 위 오류 배너 + [다시 불러오기]만 보이고 대화상자는 수정 모드를 유지한다. 저장을 다시 눌러도 같은 `clientRequestId`를 쓴다.
- 열린 실행 상세(PID·실행 ID·재시작 수)는 `runtime.changed`의 rev로 갱신한다. 처음 읽은 값을 캐시로 붙들지 않는다(AR-F15).

**핵심 테스트:**
- `ansiToSpans` 단위
- `LogView`의 따라가기·멈춤 상태 기계 단위(`follow`·`userScrolled`·`newLines`)
- `define_success_then_list_failure_keeps_edit_mode_and_does_not_create_twice`: 가짜 클라이언트에서 `runtime.define` 성공 + `runtime.list` 실패 → 대화상자 제목 "작업 수정", [저장] 다시 누름 → `runtime.define` 호출 두 번의 `clientRequestId`가 같음
- `open_run_detail_follows_restart`: 상세를 연 채 `runtime.changed`(rev 증가) + 새 PID → 상세의 PID가 새 값
- E2E `runs.spec.ts`(2개)
  - `echo` 작업을 정의 → 시작 → 로그에 줄 → 종료 코드 표시
  - 서비스(`python3 -m http.server $DEVBOX_PORT`) 시작 → 포트 칩 → 중지 → 되돌리기 토스트

---

### Task 14: 하단 패널의 로그·문제 탭, 상태 표시줄, 알림

**Files:**
- Create: `app/src/features/runs/BottomLogs.tsx`, `app/src/features/problems/ProblemsPanel.tsx`
- Create: `crates/core/src/notify.rs`(데몬 알림 주제 `notify.raised`)
- Modify: `app/src/shell/StatusBar.tsx`, `app/src/features/notifications/AttentionBridge.tsx`

**Interfaces:**
- 하단 패널 `로그` 탭은 고정한 소스를 보이고, 고정이 없으면 현재 프로젝트의 최근 실행을 보인다. `문제` 탭은 `observe.problems`(파일·줄·메시지, 누르면 열기)를 보인다.
- 상태 표시줄: "실행 n · 포트 5173, 8080 · WSL 6.1/20GB"(누르면 해당 화면)
- 데몬 알림 주제 `notify.raised { kind, title, body, target }`
  - 예약 실패와 서비스 재시작 한도 초과에 쓴다.
  - 화면은 알림 센터에 넣고, Windows 토스트는 S1 `notify_attention`과 같은 경로로 띄운다(tag `run-<id>`).

**핵심 테스트:**
- `ProblemsPanel` 렌더·이동 단위
- 데몬 `notify.raised`가 서비스 한도 초과에 한 번만 나오는지 L2(Task 4 시험에 붙임)

---

### Task 15: 비밀 설정·에이전트 테스트 실행 전환·MCP 도구

**Files:**
- Create: `app/src/features/settings/SecretsSettings.tsx`
- Modify: `crates/agents/src/actions.rs`(`agents.test`), `crates/cli/src/mcp.rs`, `app/src/features/agents/detail/RunTab.tsx`

**Interfaces:**
- 설정 › 비밀:
  - 이름 목록과 "이 PC에 값 있음/없음"을 보인다. [값 입력] 입력란은 `type=password`이고 보내면 비운다. [삭제]
  - 키 상태(열림·잠김·불일치)와 [복구 문자열 보기·입력]이 있다.
  - 불일치면 상단 배너 "비밀 키가 다릅니다 · [복구 문자열 입력][비밀 다시 입력]"(01-design §6.5)이다.
- `agents.test`:
  - S1에서는 터미널 세션이었다. 이제 runtime 작업 `[agent] test`를 그 작업 폴더에서 실행하고 `runId`를 돌려준다.
  - 실행 탭은 `LogView`로 보인다. 결과가 실패면 [에이전트에게 실패 로그 보내기]를 띄운다. 마지막 200줄을 인용해 `agents.send`로 보낸다.
- MCP 도구 추가:
  - `devbox_runs`(읽기)
  - `devbox_logs{ runId, lines ≤ 500 }`(읽기, 비밀 값 가림)
  - `devbox_problems`(읽기)
  - `devbox_run_task{ projectId, id }`(쓰기, `[mcp] write = true`일 때만)

**핵심 테스트:**
- `mcp_logs_never_include_secret_values`: 비밀을 출력하는 작업의 로그를 MCP로 읽으면 값이 `****`로 가려짐
- `agents_test_runs_in_the_agent_worktree`(L2)
- `SecretsSettings` 단위(값 입력 후 입력란 비움, 목록에 값이 나오지 않음)

```bash
git add -A && git commit -m "feat(runs): add secrets settings, agent test runs and MCP run tools"
```

---

### Task 15b: S3·S5 동시 진행 준비 — 공용 색인 DB와 코드 보기

S2 다음에는 S3(기록)과 S5(개발 도구)를 한 세션이 두 트랙으로 동시에 진행한다(사용자 결정 2026-10-08, 00-roadmap §3.6). 두 쪽이 다 쓰는 바탕을 여기서 먼저 만든다. 그래야 두 브랜치가 같은 것을 따로 만들거나 서로를 기다리지 않는다.

**Files:**
- Create: `crates/core/src/index_db.rs`
- Create: `app/src/ui/code/{CodeView.tsx,theme.ts,languages.ts}` · Test: `app/src/ui/code/CodeView.test.tsx`
- Modify: `crates/core/src/lib.rs`, `app/package.json`(`@codemirror/state`·`view`·`language`·`language-data`·`commands`·`search`), `.gitattributes`

**Interfaces:**
- `IndexDb::open(path: &Path) -> Result<IndexDb, CoreError>`: `index.db`(파생 데이터, 백업 없음, 01-design §4.4)를 WAL로 연다. 쓰기 연결 하나 + 읽기 연결은 S0b `Db`와 같은 방식이다.
- `IndexDb::ensure_domain(&self, domain: &str, version: u32, create_sql: &str) -> Result<bool, CoreError>`:
  - `index_schema(domain TEXT PRIMARY KEY, version INTEGER)`에 적힌 버전이 다르거나 없으면, 그 도메인의 테이블(`<domain>_` 접두사)만 지우고 `create_sql`로 다시 만든 뒤 `true`(다시 색인해야 함)를 돌려준다. 같으면 `false`.
  - 다른 도메인의 테이블은 건드리지 않는다. S3(`notes`·`search`)과 S5(`api`)가 각자 자기 도메인으로 부른다. 그래서 동시에 진행해도 서로의 색인을 지우지 않는다.
- 화면 `CodeView({ value, language?: string, readOnly?: boolean, onChange?: (text: string) => void, ariaLabel: string })`: CodeMirror 6 한 칸.
  - `languages.ts`의 `loadLanguage(nameOrFileName)`이 `@codemirror/language-data`에서 언어를 지연 로드한다.
  - `theme.ts`는 01-design §8.3의 토큰으로 편집기 색을 만드는 **유일한** 모듈이다.
  - 쓰는 곳: S3 `EditorHost`(노트), S4 코드 편집기, S5 요청·응답 본문.
- `.gitattributes`에 `Cargo.lock merge=lockfile`, `pnpm-lock.yaml merge=lockfile`, `app/src/rpc/gen/rpc.ts merge=lockfile`를 더한다. 각 clone에서 `git config merge.lockfile.driver true`가 필요하다(00-roadmap §3.3). rebase 중 충돌하면 위쪽(main) 판을 남기고, rebase가 끝난 뒤 한 번에 다시 만든다(00-roadmap §3.6).

**핵심 테스트:**
- `ensure_domain_recreates_only_that_domain`: `notes_x`·`api_y` 테이블에 행을 넣고 `api` 버전만 올림 → `api` 호출은 `true`이고 `api_y`가 비어 있음, `notes` 호출은 `false`이고 `notes_x` 행이 그대로
- `ensure_domain_is_idempotent`: 같은 버전으로 두 번 부르면 두 번째는 `false`
- `CodeView.test.tsx`: 텍스트 표시, `language="json"` 로드 뒤 구문 강조 요소가 생김, 편집하면 `onChange`가 새 텍스트로 불림, `readOnly`면 편집 안 됨

```bash
git config merge.lockfile.driver true
git add -A && git commit -m "feat(core): add a per-domain index database and a shared code view"
# 묶음 R 끝: PROGRESS.md의 묶음 R 행과 현재 위치를 고쳐 커밋한 뒤 push한다. PR·CI는 없다(00-roadmap §3)
pnpm check && git push origin v1/s2-runs
```

---

### Task 16: S2 완료 — 전환점 확인

**Files:**
- Modify: 이 문서 상태 줄, `PROGRESS.md`(S2 완료·PR 행·현재 위치, 다음은 S3·S5 동시 시작), `scripts/loc.sh` 실행 결과를 PR 본문에 기록

- [ ] **Step 1: SC9 측정:** 실행 중인 작업이 없을 때 데몬 RSS ≤ 60MB, CPU ≤ 0.5%(`/proc/<pid>/status`·`stat`를 1분 표본으로). 결과를 PR 본문에 쓴다. 넘으면 원인을 찾아 고친 뒤 머지한다.
- [ ] **Step 2: 실사용 확인(사용자, 10개)** — 설치 파일은 `bash scripts/dogfood.sh v1/s2-runs`로 받는다.
  1. 실제 프로젝트의 `pnpm dev`를 서비스로 정의 → 시작 → 로그 실시간 → 포트 칩으로 브라우저 열기
  2. 서비스를 일부러 죽여 자동 재시작, 한도 초과 시 Windows 알림 1회
  3. 앱을 닫았다 다시 열어도 서비스가 계속 돌고 로그가 이어진다.
  4. 예약 작업이 정해진 시각에 돌고, 실패하면 알림
  5. 빌드 오류가 문제 탭에 file:line으로 보이고 누르면 연다.
  6. 비밀 하나를 넣고 그 비밀을 쓰는 작업을 실행 → 동작하고, 로그·설정 화면에 값이 없다.
  7. `target = "windows"` 작업이 `C:\dev\<프로젝트>`로 동기화 뒤 PowerShell로 돈다.
  8. 컨테이너 목록·로그 보기, 컨테이너 재시작
  9. 에이전트 상세의 [테스트 실행]이 실행 로그로 보이고, 실패 로그를 에이전트에게 보낸다.
  10. 시작 구성 실행으로 서비스·터미널·에이전트가 한 번에 뜬다.
- [ ] **Step 3: 전환:** 사용자가 A·B·C를 새 빌드로 옮긴다. v0.9.0은 기록·편집·도구(E·D·F·G) 용도로만 쓴다. `PROGRESS.md` 세션 기록과 S2 행에 전환 날짜를 적는다.
- [ ] **Step 4: 상태 갱신·PR**

```bash
sed -i 's/^- 상태: 계획(과제 수준) · 미착수 · 시작 조건: S1 완료.*/- 상태: 완료(S2 PR 머지, 실사용 확인 10\/10, 전환 완료)/' docs/superpowers/plans/2026-10-08-devbox-v1/05-s2-runs.md
# PROGRESS.md: S2 행 완료일, PR 표의 S2 행, 현재 위치(다음: 트랙 1 S3 Task 1과 트랙 2 S5-1을 동시에 시작, 00-roadmap §3.6)를 고친다
git add docs && git commit -m "docs(plan): mark S2 complete and record the switch-over"
git push origin v1/s2-runs && gh pr create --base main --head v1/s2-runs --title "feat: runs and observation (S2)" --body "S2 실행·관찰 전체(Task 1–16), SC9 측정, 실사용 확인 10개 결과, 전환 날짜.

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
```
