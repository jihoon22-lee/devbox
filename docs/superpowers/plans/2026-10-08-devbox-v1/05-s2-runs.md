# S2 실행·관찰 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

- 상태: 계획(과제 수준) · Task 1 미착수 · S1 #640 머지로 시작 조건 충족. `v1/s2-runs` 준비 완료. 2026-10-14 착수 전 보강: 저장소 명령 신뢰 범위 2(Task 2·4·5·6·12·13·15), 수용 전용 설치 채널(Task 15c), Windows 수용 잠금(Task 15b), 자동 리뷰 3회 상한(Task 16), S1 이월 확인(Task 1)
- 상세화 수준: 과제·파일·인터페이스·핵심 테스트까지 적었다. **Task 1에서 실제 S0b·S1 코드에 맞춰 단계별 코드까지 상세화한 뒤 실행한다.**

**Goal:** 프로젝트의 작업·서비스·예약을 devbox가 띄우고 지켜보게 한다.
- 대상: 실시간 로그(여러 소스 병합), 포트·프로세스·컨테이너, 문제(file:line), WSL 자원
- 함께 하는 일: 비밀 값 주입, Windows 대상 실행, 시작 구성의 서비스, MCP 실행·로그 도구

S2가 끝나면 **선택적 조기 사용 전환점**이다. 에이전트·실행·터미널(A·B·C)을 새 빌드로 옮겨 써도 된다고 안내하며 기다리지 않는다. 기능 완성 뒤 실사용이 기본이다.

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

**Spec:** [01-design.md](01-design.md) §2.1 실행·§4.3·§4.4·§6.3·§7(특히 "저장소 명령 신뢰 확인"과 "실행 범위 버전 2")·§9 `runtime`·`observe`·`secrets`·§9.1-10·§10 설치 채널·§11 수용 검증, [09-s0a-results.md](09-s0a-results.md)(R10 interop 판정)

## Global Constraints

- S0b·S1의 Global Constraints를 따른다.
- 작업 위치: S2 전체가 브랜치 `v1/s2-runs`(origin/main에서) 하나와 worktree `../devbox-wt/v1-s2-runs` 하나다. Task 1(상세화)이 이 브랜치의 첫 커밋이다. PR은 Task 16에서 한 번 연다(00-roadmap §3).
- 실행 unit 이름은 `devbox-<인스턴스>-run-<runId>`다. tmux·데몬 unit과 섞지 않는다. 테스트는 `devbox-test-<난수>.slice` 아래에서만 띄우고 끝나면 slice를 멈춘다(01-design §4.3).
- 재시작 주체는 systemd 하나다. 데몬은 재시작을 직접 하지 않는다. 표시와 알림만 한다.
- 실행 결과의 원장은 `runs/<runId>.exit` 파일이다. transient unit은 끝나면 정리되므로 unit 상태를 결과로 쓰지 않는다.
- 비밀 평문은 DB·로그·파일·프로세스 인자·MCP 응답·RPC 결과에 나가지 않는다. 실행에는 `systemd-run --setenv`가 아니라 래퍼의 표준 입력으로 넘긴다. 이유: 인자는 `ps`·`systemctl show`에 보인다.
- 로그 줄 형식은 `<RFC3339 밀리초>\t<o|e|s>\t<줄>`이다(`s` = devbox 표시 줄). 64 MiB를 넘으면 앞을 자르고 표시 줄을 남긴다.
- **저장소 명령 신뢰(01-design §7 실행 범위 버전 2):** task/service/schedule 실행, `agents.test`, MCP `devbox_run_task`, 예약 실행은 S1의 `ProjectsService` 신뢰 확인(`devbox_projects::trust`)을 거쳐 승인된 불변 snapshot의 정의만 실행한다. `SCOPE_VERSION`을 2로 올리되 범위 1 정의만 있는 설정의 기존 승인은 유효하게 둔다. 직접 RPC·MCP·내부 호출도 데몬이 강제하고, 승인은 화면 확인 대화상자에서만 만든다. "이 PC에만" 정의는 승인 대상이 아니다.
- **수용 설치본:** Task 15c 이후 Windows 설치본 스모크는 `accept` 채널만 쓰고 prod 설치본·`devbox@prod`를 바꾸지 않는다(불변 확인용 읽기 전용 비교만). 스모크는 `windows-acceptance` 잠금으로 한 번에 하나다(00-roadmap §2.2).
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
| 승인 뒤 누군가 `.devbox/devbox.toml`의 task 명령을 바꿈 | 수동 시작·MCP 실행은 `projects.trust_required`로 거절, 예약은 실행하지 않고 `skipped(trust)`·알림 1회, 실행 중인 서비스는 그대로 | Task 2·4·5·15 |
| 화면에서 정의를 프로젝트 파일에 저장 | 저장 직전 바이트가 승인됐거나 실행 정의가 없어 승인이 필요 없었으면 새 digest 승인이 함께 기록돼 재승인이 뜨지 않음(첫 작업 저장 포함). 밖에서 먼저 고쳤으면 기록하지 않음 | Task 2 |
| S1에서 승인했고 task/service/schedule이 없는 프로젝트 | 범위 2가 되어도 다시 묻지 않음 | Task 2 |
| 사용자가 prod 설치본을 쓰는 중에 수용 스모크 | `accept` 채널만 설치·제거, prod 앱·`devbox@prod`·공유 바이너리·unit 템플릿 불변 | Task 15c·16 |

---

## 작업 묶음

S2 전체가 브랜치 `v1/s2-runs` 하나, PR 하나다(00-roadmap §3). 작업 위치는 worktree `../devbox-wt/v1-s2-runs`다.
묶음은 로컬 커밋·세션 인계 지점이다. 과제별 좁은 검증과 별도 검토 뒤 `PROGRESS.md` 묶음 행·현재 위치를 갱신한다. 묶음별 push와 전체 검사는 하지 않는다. 하위 프로젝트 끝에 브랜치 전체 검토 결과를 PROGRESS에 남기고 최종 상세 검증·Biome CI 뒤 push·PR을 한 번 진행한다(00-roadmap §3).
PR은 마지막 묶음이 끝난 뒤 한 번 열고, 그 CI가 S2 전체를 한 번 검사한다.

| 묶음 | 과제 | 끝나면 |
|---|---|---|
| 상세화 | Task 1 | 커밋(이 브랜치의 첫 커밋) |
| O | Task 2–6 | 로컬 커밋·인계 |
| P | Task 7–10 | 로컬 커밋·인계 |
| Q | Task 11–12 | 로컬 커밋·인계 |
| R | Task 13–15c | 로컬 커밋·인계 |
| 마무리 | Task 16 | **S2 PR 열기** → 필수 CI 3개·자동 리뷰 3회 상한 판정(CONVENTIONS §8) → squash 머지 |

---

### Task 1: 실제 코드에 맞춘 상세화

**Files:**
- Modify: 이 문서(`05-s2-runs.md`)

- [ ] **Step 1:** S0b·S1에서 실제로 만들어진 이름을 확인해 이 계획의 Interfaces를 고친다. `PROGRESS.md` §5(계획과 달라진 점)를 먼저 읽는다.
  - `method!`·`Router`·`Ctx::open_stream`·`Db`·`Hub`·`TestDaemon` 보조 함수
  - 화면 쪽 `useRpcQuery`·`SectionLayout`·`useBottomTabs`·`TerminalView`
  - `09-s0a-results.md`의 R10(interop) 판정과 S1 실사용에서 나온 수정 사항
  - S1 신뢰 확인의 실제 이름: `devbox_projects::trust::{SCOPE_VERSION, AuthorizedConfig, SetupSnapshot, LaunchSnapshot}`, `ProjectsService::{authorized_config, trust_preview, trust_approve}`, `TrustOperation`, `ProjectsError::{TrustRequired, TrustPreviewStale}`(`projects.trust_required`·`projects.trust_preview_stale`), 저장 표 `projects_command_trust`, 판정 함수 `has_executable`·`operation_exec`(`crates/projects/src/trust.rs`)
- [ ] **Step 1b: S1 이월 확인(2026-10-14 검토).** `PROGRESS.md` §5 맨 앞의 "S2 착수 요약"을 기준으로 다음을 이 문서에 반영하거나 결과를 PROGRESS에 적는다.
  - 파일 분할 계획: S2가 고치는 `crates/agents/src/actions.rs`(571줄)·`crates/projects/src/config.rs`(563줄)·`crates/agents/src/service.rs`(598줄)는 600줄 상한에 가깝다. Task 2·15 상세화에서 먼저 나눌 모듈을 정한다(동작 변경 없는 이동은 별도 커밋).
  - 신뢰 범위 2의 판정 함수와 `TrustOperation` 추가 값(예: `Run`)을 Task 2·4·5·15에 같은 이름으로 적는다.
  - Codex 실제 worktree 실행(S1에서 PASS 근거 없음): 사용자 Codex 설정을 바꾸지 않고, `test-<난수>` 인스턴스와 `/home/jihoon/projects/` 아래 소유 임시 저장소에서 worktree 작업 1개를 만들어 도구 없는 PONG 지시만 보낸다. 폴더 신뢰 화면이 뜨는지, 뜨면 devbox가 입력 대기로 보여 주는지 기록하고 소유 저장소·worktree·branch·unit을 정리한다. 막히면 PROGRESS §2에 적고 S2는 계속한다.
  - CI E2E의 원인 미확정 tmux 3.4 서버 종료(S1 c68c923e)는 PROGRESS §2 미결 항목이다. Task 15b에서 E2E 실패 산출물에 tmux 버전·서버 로그를 남기도록 한다.
- [ ] **Step 2:** Task 2–16(15b·15c 포함) 각각을 `writing-plans` 형식(실패 테스트 코드 → 실패 확인 → 구현 코드 → 통과 → 커밋)으로 펼친다. 아래 "핵심 테스트"와 [00-roadmap §7](00-roadmap.md) 추적표에서 S2에 걸린 시험(AR-F02·F04·F14·F15)과 입력값을 그대로 포함한다.
- [ ] **Step 3:** `09-s0a-results.md`의 R10·DPAPI 실험 결과로 Task 11의 키 풀기 경로를 정한다. 데몬 interop 성공 여부와 무관하게 Windows 앱의 `secrets.unlock` fallback을 구현·시험한다. 데몬 직접 해제는 지원 환경에서만 사용한다(키 방식은 Q1에서 DPAPI로 확정).
- [ ] **Step 4:** 커밋(PR 없음). PR은 Task 16에서 한 번만 연다. 자동 리뷰 회차(3회 상한)는 PR을 연 뒤의 push마다 세므로, 상세화·중간 과제에서 PR을 열면 회차를 미리 써 버린다.

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
- 프로젝트 `.devbox/devbox.toml` 변경 감시(`notify`, 300ms 디바운스) → `runtime.changed`. 다시 읽은 설정의 승인 상태(승인됨·낡음·필요 없음)를 `runtime.list` 결과에 함께 싣는다.
- 신뢰 범위 2(01-design §7):
  - `SCOPE_VERSION = 2`. 설정의 실행 정의 최소 범위를 계산해, task/service/schedule이 없으면 범위 1 이상 승인을, 있으면 범위 2 승인을 요구한다. 승인 미리보기에 task/service/schedule의 명령·작업 폴더·환경 변수 이름과 값·대상·의존 순서·비밀 이름을 더한다(비밀 값은 없음).
  - `runtime.define`/`undefine`이 프로젝트 파일에 쓸 때: 쓰기 직전 바이트가 유효한 상태(필요한 범위로 승인됨, 또는 실행 정의가 없어 승인 불필요)였으면 쓴 바이트의 digest로 현재 `SCOPE_VERSION`(2) 승인을 같은 DB 쓰기에서 기록한다. 승인이 필요한데 승인과 다른 바이트였으면(밖에서 고침) 기록하지 않는다. `saveTo: thisPc`는 승인과 무관하다.

**핵심 테스트:**
- `merge_prefers_project_file_and_reports_shadowed`
- `rejects_dependency_cycles_with_the_cycle_path`(`a → b → a`의 경로를 오류에 담음)
- `define_into_project_file_preserves_comments`(주석과 다른 표가 그대로)
- `watch_reloads_on_external_edit`(파일을 고치면 `runtime.changed` 수신, 승인 상태가 낡음으로 바뀜)
- `scope_one_approval_stays_valid_without_run_definitions`·`run_definitions_require_scope_two_approval`
- `define_into_approved_file_refreshes_the_approval`·`define_after_external_edit_keeps_the_approval_stale`·`first_define_into_file_without_run_definitions_needs_no_prompt`

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
- 시작 구성의 `services`(S1에서 무시하던 것)를 이제 `projects.launch`에서 시작한다. 시작 구성 전체가 같은 승인 snapshot을 쓴다(S1 규칙).
- `runtime.start`·`runtime.restart`·의존 작업 시작은 unit을 만들기 전에 신뢰를 확인하고 승인 snapshot의 정의로 `unit_args`를 만든다. 승인이 없거나 낡으면 자원을 만들지 않고 `projects.trust_required`를 돌려준다. systemd `Restart=`의 자동 재시작은 확인하지 않는다(이미 만든 unit).

**핵심 테스트(L2, 실제 systemd user):**
- `task_runs_to_completion_and_reports_exit`
- `service_restarts_on_failure_until_start_limit_then_fails_once`: `exit 1` 서비스, `StartLimitBurst=3`
- `manual_restart_resets_the_start_limit`
- `restart_no_service_is_not_restarted_after_exit`: `restart = "no"` 서비스가 `exit 1` → 10초 뒤 `NRestarts=0`, 상태 `Failed`(AR-F02b 입력)
- `daemon_restart_reconciles_finished_and_running_runs`: 실행 중 데몬 재시작 + 그사이 끝난 작업
- `depends_on_runs_in_order_and_stops_on_failure`
- `start_requires_current_trust_and_creates_no_unit_when_stale`: 승인 뒤 명령을 바꾸면 거절·unit 없음, 재승인 뒤 새 명령으로 시작
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
- 예약은 무인 실행이라 실행 시점마다 신뢰를 확인한다. 승인이 없거나 낡았으면 실행하지 않고 `skipped(trust)`를 기록하고 알림 `{ kind: "schedule_trust_required", projectId, task }`를 승인이 바뀔 때까지 한 번만 띄운다. 놓친 예약(`RunOnce`)도 같다.
- 메서드 `runtime.schedules{ projectId }` → `{ items: [{ id, cron, next, last: { at, result } }] }`

**핵심 테스트:**
- 이식한 `cron.rs` 테스트 전부(DST 포함)
- `missed_run_once_fires_exactly_once_after_downtime`
- `missed_skip_records_without_running`
- `scheduler_recomputes_when_definitions_change`(가짜 시계)
- `stale_trust_skips_scheduled_run_and_notifies_once`(가짜 시계, 두 번째 예약 시각에도 알림 추가 없음)

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

- 제안은 파일을 읽기만 하고 실행하지 않는다. `just --summary`처럼 도구를 부르는 감지는 저장소 내용을 실행할 수 있으므로 쓰지 않고 `justfile`을 직접 읽는다. 제안을 추가할 때 명령을 보여 주고 `runtime.define`으로 저장한다(프로젝트 파일이면 Task 2 승인 규칙).

**핵심 테스트:** 각 파일 종류의 fixture 폴더로 제안 목록 비교. 이미 정의된 ID는 제외. 감지가 어떤 프로세스도 띄우지 않는다.

```bash
# 묶음 O 끝
# 묶음 O 끝: 과제별 좁은 검증·별도 검토 뒤 PROGRESS.md의 묶음 O 행과 현재 위치를 갱신해 로컬 커밋한다(00-roadmap §3).
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
# 묶음 P 끝: 과제별 좁은 검증·별도 검토 뒤 PROGRESS.md의 묶음 P 행과 현재 위치를 갱신해 로컬 커밋한다(00-roadmap §3).
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
  - `unseal() -> Result<Key, SecretsError>`: 같은 경로로 푼다. 데몬의 Windows 호출이 불가능하면 Windows 앱이 풀어 `secrets.unlock{ key }`로 전달한다.
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
- `windows_app_unlocks_when_daemon_interop_is_unavailable`: 데몬 Windows 호출 실패를 주입 → 잠금 상태 유지 → Windows 앱 DPAPI 해제·`secrets.unlock` → 동일 key_id로 비밀을 쓰는 실행 성공, 로그·RPC 응답에는 키·비밀이 없음. fallback은 필수 핵심 회귀 테스트다. R10 성공은 한 로그인 세션에 한정되므로 이를 생략하는 근거가 아니다.
- DPAPI blob 입출력 조립을 단위 검증하고, 직접 해제와 Windows 앱 fallback의 실제 왕복은 Task 16 에이전트 설치본 스모크에서 모두 확인한다.

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
- 화면 표시: "Windows에서 실행 · 자식 프로세스 종료는 보장하지 않음". 신뢰 미리보기에도 대상(Windows)과 실행 폴더를 보인다.

**핵심 테스트:**
- `sync_copies_changed_tracked_files_and_keeps_extra_target_files`(임시 폴더 두 개로)
- `windows_command_is_quoted_for_powershell`(작은따옴표 이스케이프)
- 실제 실행은 Task 16 에이전트 설치본 검증

```bash
# 묶음 Q 끝
# 묶음 Q 끝: 과제별 좁은 검증·별도 검토 뒤 PROGRESS.md의 묶음 Q 행과 현재 위치를 갱신해 로컬 커밋한다(00-roadmap §3).
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
- 신뢰: 시작·재시작에서 `projects.trust_required`를 받으면 S1의 신뢰 확인 대화상자(같은 표시 전용 제어 문자 변환)를 task/service/schedule 항목까지 넓혀 띄운다. 승인 뒤 같은 동작을 한 번 다시 시도한다. 목록 행에는 "설정이 바뀌어 다음 시작에 재승인 필요"를 보인다.
- `model.ts`의 `ansiToSpans(text)`: SGR 16·256·truecolor·굵게·밑줄. 그 밖의 제어열은 지운다.

- 생성과 조회 실패의 분리(01-design §8.6 규칙 2, AR-F04): `DefineDialog`는 `runtime.define` 응답으로 바로 "수정" 모드(생성된 ID)로 바뀐다. 이어지는 `runtime.list` 조회가 실패하면 목록 위 오류 배너 + [다시 불러오기]만 보이고 대화상자는 수정 모드를 유지한다. 저장을 다시 눌러도 같은 `clientRequestId`를 쓴다.
- 열린 실행 상세(PID·실행 ID·재시작 수)는 `runtime.changed`의 rev로 갱신한다. 처음 읽은 값을 캐시로 붙들지 않는다(AR-F15).

**핵심 테스트:**
- `ansiToSpans` 단위
- `LogView`의 따라가기·멈춤 상태 기계 단위(`follow`·`userScrolled`·`newLines`)
- `define_success_then_list_failure_keeps_edit_mode_and_does_not_create_twice`: 가짜 클라이언트에서 `runtime.define` 성공 + `runtime.list` 실패 → 대화상자 제목 "작업 수정", [저장] 다시 누름 → `runtime.define` 호출 두 번의 `clientRequestId`가 같음
- `open_run_detail_follows_restart`: 상세를 연 채 `runtime.changed`(rev 증가) + 새 PID → 상세의 PID가 새 값
- `start_with_stale_trust_shows_preview_then_retries_once`(가짜 클라이언트)
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
  - `devbox_run_task{ projectId, id }`(쓰기, `[mcp] write = true`일 때만). 신뢰 승인이 없거나 낡으면 실행하지 않고 `projects.trust_required`를 돌려준다. MCP로 승인을 만들거나 미리보기를 승인할 수 없다.
- `agents.test`는 S1처럼 `TrustOperation::Test` snapshot의 `[agent] test`만 runtime으로 실행한다.

**핵심 테스트:**
- `mcp_logs_never_include_secret_values`: 비밀을 출력하는 작업의 로그를 MCP로 읽으면 값이 `****`로 가려짐
- `agents_test_runs_in_the_agent_worktree`(L2)
- `mcp_run_task_requires_trust_and_cannot_approve`
- `SecretsSettings` 단위(값 입력 후 입력란 비움, 목록에 값이 나오지 않음)

```bash
git add -A && git commit -m "feat(runs): add secrets settings, agent test runs and MCP run tools"
```

---

### Task 15b: S3·S5 동시 진행 준비 — 공용 색인 DB·코드 보기·검사 대기

S2 다음에는 S3(기록)과 S5(개발 도구)를 한 세션이 두 트랙으로 동시에 진행한다(사용자 결정 2026-10-08, 00-roadmap §3.6). 두 쪽이 다 쓰는 바탕을 여기서 먼저 만든다. 그래야 두 브랜치가 같은 것을 따로 만들거나 서로를 기다리지 않는다.

**Files:**
- Create: `crates/core/src/index_db.rs`
- Create: `app/src/ui/code/{CodeView.tsx,theme.ts,languages.ts}` · Test: `app/src/ui/code/CodeView.test.tsx`
- Modify: `scripts/verify-resources.py`(검사 잠금 대기 옵션) 및 해당 회귀 테스트
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
- `.gitattributes`에 `Cargo.lock merge=lockfile`, `pnpm-lock.yaml merge=lockfile`, `app/src/rpc/gen/rpc.ts merge=lockfile`를 더한다. 각 clone에서 `git config merge.lockfile.driver true`가 필요하다(00-roadmap §3.3). main 반영 중 충돌하면 main 판을 기준으로 실제 의존성·RPC 선언에 맞춰 다시 만들고, 최종 결과를 검증한다(00-roadmap §3.6).

**검사 잠금·자원(사용자 결정 2026-10-13):**
- `scripts/verify-resources.py`에 `--wait`를 추가한다. 기본은 Git common-dir 비차단 잠금 경합 시 exit 75를 유지하고, 대기 모드는 최대 60분 기다린 뒤 exit 75와 "검사 대기 초과"를 보고한다. 이 결과는 검사 실패와 구분해 다시 시도한다.
- 잠금 이름을 받는 `--lock <name>`을 더한다. 기본은 지금의 검사 잠금이고, `windows-acceptance`는 Windows 설치본 스모크 전체(다운로드 확인→설치→검증→제거·정리) 동안 잡는다(00-roadmap §2.2·§3.6). 두 잠금은 서로 독립이다.
- E2E가 실패하면 산출물에 tmux 버전과 해당 시험 tmux 서버의 종료 상태·로그를 남긴다(S1 c68c923e의 원인 미확정 서버 종료 추적, PROGRESS §2).
- S3·S5 동시 진행의 전체 검사(`scripts/check.sh`)·E2E는 대기 모드로 한 번에 하나만 실행한다. `scripts`는 동시 진행 중 수정할 수 없는 공유 파일이므로 옵션과 호출 경로를 S2에서 끝낸다.
- 트랙별 좁은 검사는 잠금 없이 실행하되 트랙당 Cargo jobs 2·Rust test threads 2·Vitest workers 2(합 4), CPU 4·nice +10, 지원 cgroup high 6GiB/max 8GiB·swap 1GiB를 지킨다.
- 트랙 2는 `CARGO_TARGET_DIR=$HOME/.cache/targets/devbox-s5`를 쓰고 S5 머지 뒤 정리한다. wsl-resource-guard 개입 또는 `killed:*` 등 메모리 압박 시 한 트랙을 잠시 멈춘다.

**핵심 테스트:**
- 잠금 회귀: `--lock windows-acceptance`와 검사 잠금이 서로 막지 않음, 기본 경합 즉시 exit 75, `--wait` 잠금 해제 후 명령 실행, 기한 초과 exit 75·대기 초과 문구·자식 미실행, 서로 다른 worktree가 같은 common-dir 잠금을 공유함을 짧은 fixture 기한으로 확인한다. 기존 CPU·nice·cgroup 제한도 유지되는지 확인한다.
- `ensure_domain_recreates_only_that_domain`: `notes_x`·`api_y` 테이블에 행을 넣고 `api` 버전만 올림 → `api` 호출은 `true`이고 `api_y`가 비어 있음, `notes` 호출은 `false`이고 `notes_x` 행이 그대로
- `ensure_domain_is_idempotent`: 같은 버전으로 두 번 부르면 두 번째는 `false`
- `CodeView.test.tsx`: 텍스트 표시, `language="json"` 로드 뒤 구문 강조 요소가 생김, 편집하면 `onChange`가 새 텍스트로 불림, `readOnly`면 편집 안 됨

```bash
git config merge.lockfile.driver true
git add -A && git commit -m "feat(core): add a per-domain index database and a shared code view"
# 묶음 R 끝: 과제별 좁은 검증·별도 검토 뒤 PROGRESS.md의 묶음 R 행과 현재 위치를 갱신해 로컬 커밋한다(00-roadmap §3).
```

---

### Task 15c: 수용 전용 설치 채널 (사용자 결정 2026-10-14)

S2 머지 뒤 사용자가 새 빌드를 prod로 쓰기 시작하면, 01-design §11의 "기존 v1 설치본·`devbox@prod`가 있으면 멈춰 묻기" 때문에 이후 모든 설치본 스모크가 멈춘다. S3·S5 두 트랙이 같은 prod 자원에 설치하면 서로 덮어쓴다. 그래서 에이전트 수용 검증 전용 `accept` 채널을 만든다(01-design §6.1·§10·§11).

**Files:**
- Modify: `app/src-tauri/src/core/connection.rs`(`INSTANCE`), `app/src-tauri/src/core/deploy.rs`·`app/src-tauri/src/platform/deploy.rs`·`supervisor.rs`(배치 경로), `crates/cli/src/setup.rs`(unit 템플릿·바이너리 경로)
- Create: `app/src-tauri/tauri.accept.conf.json`(Tauri 설정 덮어쓰기)
- Modify: `.github/workflows/windows.yml`(PR·수동 실행에서 `devbox-setup-accept` 산출물 추가, 태그 릴리스는 prod만), `scripts/dogfood.py`·`scripts/dogfood.sh`(채널 선택, 기본 prod), `scripts/win-package.ps1`(채널별 self-check)

**Interfaces:**
- 채널은 빌드할 때 정한다(예: `DEVBOX_APP_CHANNEL=accept` 컴파일 시 환경 변수). 릴리스 빌드 기본은 prod, 디버그는 지금처럼 dev다. 실행 중 바꾸는 설정은 두지 않는다.
- `accept` 채널이 prod와 달라야 하는 것: Windows identifier `io.github.jihoon22lee.devbox.accept`·제품 이름·설치 폴더·`%LOCALAPPDATA%` 앱 데이터·프로토콜 `devbox-accept://`·자동 시작 항목·단일 인스턴스 키·토스트 AUMID, WSL 인스턴스 `accept`·채널 전용 바이너리 경로·unit 템플릿 이름. prod의 `~/.local/share/devbox/bin/devbox`와 `devbox@.service`를 쓰거나 지우지 않는다.
- 같은 PC에서 prod와 `accept`가 동시에 실행돼도 된다. 전역 단축키는 OS 전역이라 prod가 먼저 잡았으면 `accept` 등록이 실패하고, 그 항목만 미실행으로 기록한다.
- 에이전트 hook이 부르는 CLI 경로도 채널 전용 바이너리를 가리킨다.
- `scripts/dogfood.sh <브랜치> [--channel accept]`: 수용 스모크는 `accept`를 받는다. 사용자 dogfood는 기본 prod 그대로다.

**핵심 테스트:**
- 단위: 채널별 identifier·인스턴스·바이너리 경로·unit 템플릿 이름 표(prod·accept·dev)
- `accept_setup_leaves_prod_binary_and_template_untouched`(임시 HOME에 prod 바이너리·템플릿을 먼저 두고 `accept` setup → prod 파일 바이트·mtime 불변)
- Windows CI: 두 NSIS 모두 self-check, receipt에 채널 기록
- 실제 확인은 Task 16 스모크에서 한다. 사용자 prod 자원이 있으면 해시·unit 상태를 설치 전후로 읽기 전용 비교해 불변을 확인한다. 없으면 "비교 대상 없음"으로 기록하고 위 단위 시험이 근거다.

```bash
git add -A && git commit -m "feat(windows): add an isolated acceptance install channel"
```

---

### Task 16: S2 완료 — 전환점 확인

**Files:**
- Modify: 이 문서 상태 줄, `PROGRESS.md`(S2 완료·PR 행·현재 위치, 다음은 S3·S5 동시 시작), `scripts/loc.sh` 실행 결과를 PR 본문에 기록

- [ ] **Step 1: SC9 측정:** 실행 중인 작업이 없을 때 데몬 RSS ≤ 60MB, CPU ≤ 0.5%(`/proc/<pid>/status`·`stat`를 1분 표본으로). 결과를 PR 본문에 쓴다. 넘으면 원인을 찾아 고친 뒤 머지한다.
- [ ] **Step 2: 에이전트 설치본 검증(12개)** — 설치 파일은 `bash scripts/dogfood.sh v1/s2-runs --channel accept`로 받고, `verify-resources.py --wait --lock windows-acceptance` 안에서 진행한다(Task 15b·15c).
  에이전트가 PR 전에 00-roadmap §2.2·01-design §11의 Windows 설치본 스모크를 수행한다. `scripts/dogfood.sh`와 같이 해당 브랜치 최종 SHA의 `windows.yml` 산출물에서 저장소·workflow·run·source SHA·SHA256을 확인하고, WSL에서 PowerShell 절대 경로로 설치·실행·제거한다. 아래 기능은 소유 fixture로 확인하고 앱·데몬 로그, user systemd unit 상태, 진단 정보와 필요시 스크린샷을 근거로 남긴다. UI 자동화가 불가능한 항목만 미실행으로 기록한다. prod 설치본·`devbox@prod`는 사용자 자원이라 바꾸지 않고, 설치 전후 해시·unit 상태를 읽기 전용으로 비교해 불변을 기록한다. 소유 표식 없는 `accept` 채널 자원이 이미 있으면 제거하지 않고 질문하며, UAC·재부팅·사용자 클릭이 필요하면 멈춰 보고한다. v0.9.0(`com.devbox.*`, DevboxSuite)·기존 데이터는 보존하고 이번 스모크가 만든 설치 경로·데이터·WSL unit·소켓만 정리한다. PASS·FAIL·미실행은 PROGRESS와 PR 본문에 구분한다. 자동 시험 성공을 Windows 실기 PASS로 바꾸지 않는다.
  1. 실제 프로젝트의 `pnpm dev`를 서비스로 정의 → 시작 → 로그 실시간 → 포트 칩으로 브라우저 열기
  2. 서비스를 일부러 죽여 자동 재시작, 한도 초과 시 Windows 알림 1회
  3. 앱을 닫았다 다시 열어도 서비스가 계속 돌고 로그가 이어진다. 로그 여러 개를 분리 창까지 동시에 따라가도 브리지 재연결이 0이다(S1에서 브리지 송신 큐 16칸 포화가 재연결로 이어진 사례가 있음).
  4. 예약 작업이 정해진 시각에 돌고, 실패하면 알림
  5. 빌드 오류가 문제 탭에 file:line으로 보이고 누르면 연다.
  6. 비밀 하나를 넣고 그 비밀을 쓰는 작업을 실행 → 동작하고, 로그·설정 화면에 값이 없다. 데몬 Windows 호출이 실패하는 경우에도 Windows 앱 DPAPI 해제·`secrets.unlock` fallback으로 실행되는지 반드시 확인한다.
  7. `target = "windows"` 작업이 `C:\dev\<프로젝트>`로 동기화 뒤 PowerShell로 돈다.
  8. 소유 fixture의 컨테이너 목록·로그 보기. 재시작 등 Docker 조작은 일회성 hosted CI 또는 독립 VM에서만 확인하고 로컬 기존 Docker·서비스·공유 네트워크는 변경하지 않는다.
  9. 에이전트 상세의 [테스트 실행]이 실행 로그로 보이고, 실패 로그를 에이전트에게 보낸다.
  10. 시작 구성 실행으로 서비스·터미널·에이전트가 한 번에 뜬다.
  11. 신뢰 범위 2: 승인 전 시작은 자원 0, 승인 뒤 시작, 설정을 밖에서 바꾸면 수동 시작 거절·예약 `skipped(trust)`·알림 1회, 화면에서 정의 저장은 재승인 없이 이어짐.
  12. `accept` 채널 격리: 설치·제거 전후 prod 설치본·`devbox@prod`·공유 바이너리·unit 템플릿 불변.
- [ ] **Step 3: 전환 안내:** S2 머지 뒤 "에이전트·실행·터미널은 새 빌드로 옮겨 써도 된다"고 선택적 조기 사용만 안내하고 기다리지 않는다. 실사용은 기능 완성 뒤가 기본이다. 사용자가 옮기면 날짜를 PROGRESS에 적고, 알려 준 문제는 PROGRESS §2에 기록해 진행 중인 브랜치에서 고친다.
- [ ] **Step 4: 상태 갱신·PR**

- PR 전에는 상태 줄을 직접 편집해 `구현 완료·최종 검증 결과 기록·PR 수용 대기`로 적는다. `sed` 패턴으로 완료를 추정하지 않는다. 하위 프로젝트 끝 브랜치 전체 검토와 최종 상세 검증·Biome CI를 마치고, 결과·미실행 항목을 PROGRESS와 PR 본문에 남긴다.
- 00-roadmap §3에 따라 push·PR은 한 번 진행한다. 필수 CI `Frontend (pnpm)`·`Rust (Cargo workspace)`·`Rust (Windows)`와 자동 리뷰 3회 상한 판정(CONVENTIONS §8: 1–3회 지적 처리, 4회째부터 차단 기준만 수정·나머지는 PROGRESS §8 후속 목록) 뒤 squash 머지한다. 회차 수·지적 수·판정·이월 항목을 PR 본문에 남긴다.
- 실제 머지를 확인한 뒤 상태 줄을 직접 `완료(S2 PR 머지)`로 고치고 PROGRESS의 S2 행·PR 표·현재 위치(다음: 트랙 1 S3 Task 1·트랙 2 S5-1)를 갱신한다. PR 전 수용 대기를 완료로 기록하지 않는다.
