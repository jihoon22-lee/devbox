# S2 실행·관찰 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

- 상태: **구현 완료·PR #642 열림·3회차 수정 검증 완료**(2026-10-21). b24d99d5 Frontend/Rust workspace/E2E CI PASS, 3회차 코드1건·보안0건 완료. Run 프로젝트 귀속 수정2개·독립 검토·영향 strict Clippy PASS. 최종 SHA 필수 CI·설치 시도·머지는 대기한다. 설치 전 interop errno110으로 수용1–11 미실행·12 FAIL/prod 불변 NOT_PROVEN은 보존하며 4회차 리뷰를 기다리지 않는다. 기반 `main@4cb0e833`, 인계 기록 `699d44e1` 보존.
- 상세화 수준: 실제 S0b·S1 계약과 Task 2–16(15b·15c 포함)의 실패 테스트 → 실패 확인 → 구현 → 통과 → 커밋 순서를 확정했다. 각 코드 조각은 해당 과제 구현의 시작점이며 그 아래 핵심 테스트 전부가 완료 조건이다. 테스트의 fixture helper는 해당 시험 파일에서 만든다. 구현 sketch는 전체 구현을 대체하지 않으며 `?`의 오류는 각 도메인의 typed `Fail<...>`로 매핑한다.

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

**Tech Stack:** S1 스택 + `croner` 3.0.1·`chrono` 0.4.45·`chrono-tz` 0.10(v0.9.0 `cron.rs` 이식의 실제 의존성; `cron`은 모듈 이름), `chacha20poly1305`, `zeroize`, `notify`(inotify, toml 감시), `@tanstack/react-virtual`.

**Spec:** [01-design.md](01-design.md) §2.1 실행·§4.3·§4.4·§6.3·§7(특히 "저장소 명령 신뢰 확인"과 "실행 범위 버전 2")·§9 `runtime`·`observe`·`secrets`·§9.1-10·§10 설치 채널·§11 수용 검증, [09-s0a-results.md](09-s0a-results.md)(R10 interop 판정)

## Global Constraints

- S0b·S1의 Global Constraints를 따른다.
- 작업 위치: S2 전체가 브랜치 `v1/s2-runs`(origin/main에서) 하나와 worktree `../devbox-wt/v1-s2-runs` 하나다. 인계 기록 `699d44e1` 다음에 Task 1(상세화)을 커밋한다. PR은 Task 16에서 한 번 연다(00-roadmap §3).
- 실행 unit 이름은 `devbox-<인스턴스>-run-<runId>`다. tmux·데몬 unit과 섞지 않는다. 테스트는 `devbox-test-<난수>.slice` 아래에서만 띄우고 끝나면 소유 unit/slice를 멈추고 정확한 소유 runtime drop-in 파일도 정리한다(01-design §4.3). 서비스 실패 unit은 관측·수동 reset까지 유지하고 정상 종료는 GC한다.
- 재시작 주체는 systemd 하나다. 데몬은 재시작을 직접 하지 않는다. 표시와 알림만 한다.
- 실행 결과의 원장은 `runs/<runId>.exit` 파일이다. transient unit은 끝나면 정리되므로 unit 상태를 결과로 쓰지 않는다.
- 비밀 평문은 DB·로그·파일·프로세스 인자·MCP 응답·RPC 결과에 나가지 않는다. 실행에는 `systemd-run --setenv`가 아니라 래퍼의 표준 입력으로 넘긴다. 이유: 인자는 `ps`·`systemctl show`에 보인다.
- 로그 줄 형식은 `<RFC3339 밀리초>\t<o|e|s>\t<줄>`이다(`s` = devbox 표시 줄). 64 MiB를 넘으면 앞을 자르고 표시 줄을 남긴다.
- **저장소 명령 신뢰(01-design §7 실행 범위 버전 2):** task/service/schedule 실행, `agents.test`, MCP `devbox_run_task`, 예약 실행은 S1의 `ProjectsService` 신뢰 확인(`devbox_projects::trust`)을 거쳐 승인된 불변 snapshot의 정의만 실행한다. `SCOPE_VERSION`을 2로 올리고 `required_scope_version`으로 최소 범위를 계산해 범위 1 정의만 있는 설정의 기존 승인은 유효하게 둔다. 직접 RPC·MCP·내부 호출도 데몬이 강제하고, 승인은 화면 확인 대화상자에서만 만든다. "이 PC에만" 정의는 승인 대상이 아니다.
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
| 상세화 | Task 1 | 인계 기록 다음 로컬 커밋 |
| O | Task 2–6 | 로컬 커밋·인계 |
| P | Task 7–10 | 로컬 커밋·인계 |
| Q | Task 11–12 | 로컬 커밋·인계 |
| R | Task 13–15c | 로컬 커밋·인계 |
| 마무리 | Task 16 | **S2 PR 열기** → 필수 CI 3개·자동 리뷰 3회 상한 판정(CONVENTIONS §8) → squash 머지 |

---

## 실제 코드 계약과 검증 명령 (Task 1 확정)

- RPC는 `devbox_protocol::{method, topic, Router, Ctx, Fail, Internal}`이다. `Router<Services>::register::<Method, _, _>(...)`를 `crates/cli/src/daemon/routes.rs`에 등록하고 `xtask/src/main.rs` 선언 목록에 더한 뒤 `pnpm gen`으로 TS를 만든다. `Ctx::open_stream() -> Result<StreamEnds, StreamError>`, `StreamEnds::{id, tx, input}`와 `StreamTx::{write, finish, closed}`를 사용한다. 새로운 전송 계층을 만들지 않는다.
- `devbox_core::{Db, Hub, Paths}`를 쓴다. `Db::write`/`write_checked_with_commit`의 트랜잭션 결과 rev를 `Hub::publish::<Topic>(rev, &payload)`에 보낸다. `Paths::{data_dir, index_db_path, socket, instance}`가 실제 경로다. `Services.runtime`은 이미 데몬 수명관리 `system::Runtime`이므로 실행 도메인 서비스 필드는 **`Services.runs: Arc<RuntimeService>`**로 한다.
- 테스트는 `crates/cli/tests/support::TestDaemon::{fixture, launch, client, restart}`와 `support/agents.rs`를 확장한다. 기본 fixture는 `DEVBOX_NO_SCOPE=1`이라 실제 systemd L2에서는 이 값을 제거하고 소유 test slice를 써야 한다. 이 fixture의 성공을 실제 systemd 수용 성공으로 대체하지 않는다.
- 화면은 `app/src/rpc/query.ts`의 `useRpcQuery`·`useRpcMutation`, `shell/SectionLayout.tsx`, `shell/BottomPanel.tsx`의 `registerBottomTab`·`useBottomTabs`, `features/terminal/TerminalView.tsx`를 재사용한다. `useProjectTrust`·`ProjectTrustDialog`·`visibleTrustText`가 S1 신뢰 UI다. `runtime.define` 생성 응답을 `setQueryData`에 반영하며 성공 뒤 목록 실패를 생성 실패로 되돌리지 않는다.
- 입력은 `devbox_terminal::validate_command`(64KiB·NUL 거부), 에이전트 입력은 `devbox_agents::validate_input`을 쓴다. 새 runtime이 terminal crate에 불필요하게 의존하지 않게 Task 2에서 문자열 길이/NUL 순수 검사만 `core::input::validate_command_text`로 옮기고 기존 도메인 오류 매핑을 유지한다(별도 이동 커밋). 목록은 256행·768KiB JSON 페이지, 로그 stream은 64KiB 청크·credit, 검색은 제한된 페이지로 반환한다.
- 도메인 의존 방향: `runtime → projects/core/protocol`, `observe → runtime/core/protocol`이다. `projects`는 runtime을 참조하지 않는다. 공통 실행 정의 DTO를 `projects::run_config`에 두고 runtime이 재수출·검증한다. `agents.test`의 runtime 연결은 CLI composition에서 하고 `agents ↔ runtime` 순환을 만들지 않는다.
- 새 crate 과제(Task 2·7·11)는 먼저 workspace member·Cargo.toml·lib.rs·테스트 파일의 빈 뼈대를 연결한다. `package not found`/테스트 0개는 RED가 아니며 실제 목표 테스트의 미구현 심벌 또는 assertion 실패를 확인한다. 과제별 RED/GREEN 명령은 아래 필터만 실행한다. `source ~/.cargo/env`, `CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2`, CPU 4·nice +10·지원 cgroup 6/8GiB·swap 1GiB를 지킨다. S2 중 좁은 검사도 현재 supervisor로 실행할 수 있다: `python3 scripts/verify-resources.py -- cargo test -p <crate> <filter> -- --test-threads=2`. 잠금 경합 exit 75는 실패 테스트 증거가 아니다. Vitest는 `python3 scripts/verify-resources.py -- pnpm --filter app exec vitest run <파일> --maxWorkers=2`(`app/package.json`의 실제 package 이름은 `app`). 전체 검사·E2E는 Task 16 한 번이다.
- 아래 모든 커밋 단계는 직접 영향 검사와 **별도 하위 에이전트 검토**, `pnpm exec biome ci .` 성공 뒤 지정 파일만 stage하고 `git -c user.name=jihoon22-lee -c user.email=188744452+jihoon22-lee@users.noreply.github.com commit -m '<메시지>'`로 실행한다. 중간 push·PR 없음. 문서 상세화 자체는 제품 실패 테스트를 만들지 않고 실제 코드 대조·실측·계획 완결성으로 검증한다.

### 신뢰 범위 2의 단일 계약

| 이름 | 계약·사용 위치 |
|---|---|
| `trust::required_scope_version(&ProjectConfig) -> Option<u32>` | 실행 정의 없음 `None`, S1 정의만 `Some(1)`, task/service/schedule 하나라도 있음 `Some(2)`. `has_executable`은 이 함수에 위임한다 |
| `store::approved(..., minimum_scope: u32)` | 정규 경로·전체 바이트 digest는 동일 비교, 저장된 scope는 `>= minimum_scope`. 승인 기록은 항상 현재 `SCOPE_VERSION=2` |
| `TrustOperation::Run` | 수동·의존·MCP·예약의 실행 정의. `operation_exec`에 실행 정의를 포함하고 `Launch`는 `launch.services`도 포함. `Test`는 계속 `[agent] test`만 가리킴 |
| `ProjectsService::run_snapshot(id) -> R<AuthorizedConfig>` | 한 번 읽고 해석한 바이트를 승인 확인한 불변 전체 설정. runtime은 이 snapshot의 정의로 의존 그래프까지 고정. thisPc 전용 실행은 승인 불필요하나 프로젝트 파일 의존이 있으면 snapshot 필요 |
| `LaunchSnapshot::run_config()` | 시작 구성 서비스에 필요한 승인된 실행 정의만 노출. CLI `daemon/launch.rs`가 이 snapshot을 `RuntimeService::start_with_snapshot`에 전달. 중간 파일 재읽기 금지 |
| `TrustPreview.run_definitions` | `Vec<RunDefinitionPreview>`의 typed DTO. 명령·cwd·env·대상·의존 순서·비밀 이름, schedule→task 연결을 포함. 기존 `ProjectConfig`의 `raw_*`는 JSON/TS에 나오지 않으므로 단순 상수 변경만으로 완료하지 않음 |
| `ProjectsService::update_run_config(id, edit) -> R<UpdatedRunConfig>` | typed `RunConfigEdit`(정의 추가·수정·삭제)을 받고 프로젝트별 쓰기를 직렬화한다. 직전 바이트·최소 scope 검증 → `toml_edit` 보존 쓰기 → 쓴 digest의 승인과 rev를 같은 `Db::write_checked_with_commit`에서 기록. `UpdatedRunConfig { config, rev }`를 runtime에 돌려주며 runtime은 해당 rev로 changed를 발행한다. 파일 실패는 승인0, DB 실패는 새 승인0·오류 반환. 직전 바이트가 달라졌으면 덮어쓰지 않음 |

프로젝트 정의의 원장은 TOML이며 runtime DB에는 thisPc 정의·실행 상태만 저장한다. 프로젝트 파일 정의를 runtime DB에 이중 저장하지 않는다. 프로젝트 파일 수정·승인 SQL·해당 rev는 ProjectsService의 typed helper가 소유하며 runtime은 projects SQL에 직접 접근하지 않는다. "같은 DB 쓰기"는 승인과 rev의 원자적 기록을 뜻한다.

명령·서비스 시작 전 필요한 snapshot과 모든 입력·의존을 검사하므로 거절 시 unit·작업 폴더·동기화 파일 생성 0이다. 설정 감시가 승인 자체를 만들지 않는다. `thisPc` 저장이 프로젝트 전체 승인을 승격시키지 않는다. MCP에는 승인 메서드를 노출하지 않는다.

### 600줄 근접 파일 분할

| 기존 파일(실측) | 시점과 이동 | 검증·별도 커밋 |
|---|---|---|
| `projects/src/config.rs` 563줄 | Task 2 선행: 파일 복사·경로 검사를 `config/copy.rs`, 기존 단위 시험을 `config/tests.rs`로 이동. `parse/load/read_config/copy_files` 호출 계약은 재수출로 유지. 실행 DTO는 새 `run_config.rs` | `cargo test -p devbox-projects config`와 trust 관련 직접 영향 시험 → `refactor(projects): split configuration parsing and file copying` |
| `agents/src/service.rs` 598줄 | Task 15 선행: 생성·worktree 예약 흐름을 `create.rs`, `changed/apply_event`를 `events.rs`로 이동. `AgentsService` 필드·생성자와 공개 호출 계약 유지 | `cargo test -p devbox-agents` 중 create/event/state 관련 필터와 CLI `agents` 직접 영향 시험 → `refactor(agents): separate task creation and state events` |
| `agents/src/actions.rs` 571줄 | Task 15 선행: 기존 test 준비를 `actions/test.rs`, 필요하면 review/rebase를 `actions/review.rs`로 이동. 신뢰·소유 worktree 확인은 그대로 유지한 뒤 별도 기능 커밋에서 runtime 전환 | CLI `agents_actions_safety`·`project_trust` 해당 test 회귀 → `refactor(agents): split test and review actions` |

분할은 기능 변경 전에 별도 커밋하며, 각 파일 600줄·React 컴포넌트 300줄 상한을 지킨다. PROGRESS에는 과제 범위 안의 선행 이동으로 기록하며 묶음·PR 경계는 유지한다.

---

### Task 1: 실제 코드에 맞춘 상세화

**Files:**
- Modify: 이 문서(`05-s2-runs.md`)

- [x] **Step 1:** S0b·S1에서 실제로 만들어진 이름을 확인해 이 계획의 Interfaces를 고친다. `PROGRESS.md` §5(계획과 달라진 점)를 먼저 읽는다.
  - `method!`·`Router`·`Ctx::open_stream`·`Db`·`Hub`·`TestDaemon` 보조 함수
  - 화면 쪽 `useRpcQuery`·`SectionLayout`·`useBottomTabs`·`TerminalView`
  - `09-s0a-results.md`의 R10(interop) 판정과 S1 실사용에서 나온 수정 사항
  - S1 신뢰 확인의 실제 이름: `devbox_projects::trust::{SCOPE_VERSION, AuthorizedConfig, SetupSnapshot, LaunchSnapshot}`, `ProjectsService::{authorized_config, trust_preview, trust_approve}`, `TrustOperation`, `ProjectsError::{TrustRequired, TrustPreviewStale}`(`projects.trust_required`·`projects.trust_preview_stale`), 저장 표 `projects_command_trust`, 판정 함수 `has_executable`·`operation_exec`(`crates/projects/src/trust.rs`)
- [x] **Step 1b: S1 이월 확인(2026-10-14 검토).** `PROGRESS.md` §5 맨 앞의 "S2 착수 요약"을 기준으로 다음을 이 문서에 반영하거나 결과를 PROGRESS에 적는다.
  - 파일 분할 계획: S2가 고치는 `crates/agents/src/actions.rs`(571줄)·`crates/projects/src/config.rs`(563줄)·`crates/agents/src/service.rs`(598줄)는 600줄 상한에 가깝다. Task 2·15 상세화에서 먼저 나눌 모듈을 정한다(동작 변경 없는 이동은 별도 커밋).
  - 신뢰 범위 2의 판정 함수와 `TrustOperation` 추가 값(예: `Run`)을 Task 2·4·5·15에 같은 이름으로 적는다.
  - Codex 실제 worktree 실행(S1에서 PASS 근거 없음): 사용자 Codex 설정을 바꾸지 않고, `test-<난수>` 인스턴스와 `/home/jihoon/projects/` 아래 소유 임시 저장소에서 worktree 작업 1개를 만들어 도구 없는 PONG 지시만 보낸다. 폴더 신뢰 화면이 뜨는지, 뜨면 devbox가 입력 대기로 보여 주는지 기록하고 소유 저장소·worktree·branch·unit을 정리한다. 막히면 PROGRESS §2에 적고 S2는 계속한다.
  - CI E2E의 원인 미확정 tmux 3.4 서버 종료(S1 c68c923e)는 PROGRESS §2 미결 항목이다. Task 15b에서 E2E 실패 산출물에 tmux 버전·서버 로그를 남기도록 한다.
- [x] **Step 2:** Task 2–16(15b·15c 포함) 각각을 `writing-plans` 형식(실패 테스트 코드 → 실패 확인 → 구현 코드 → 통과 → 커밋)으로 펼친다. 아래 "핵심 테스트"와 [00-roadmap §7](00-roadmap.md) 추적표에서 S2에 걸린 시험(AR-F02·F04·F14·F15)과 입력값을 그대로 포함한다.
- [x] **Step 3:** `09-s0a-results.md`의 R10·DPAPI 실험 결과로 Task 11의 키 풀기 경로를 정한다. 데몬 interop 성공 여부와 무관하게 Windows 앱의 `secrets.unlock` fallback을 구현·시험한다. 데몬 직접 해제는 지원 환경에서만 사용한다(키 방식은 Q1에서 DPAPI로 확정).
**Step 1b 실측 결과(2026-10-20):** Codex CLI 0.163.0, `test-c9f77ba13a46`의 실제 `agents.create(worktree=true)`로 소유 임시 저장소의 `agent/s2-pong-probe`를 만들었다. 폴더 신뢰 화면이 나타났고 45초 관찰 동안 devbox 상태는 `preparing → running`, `waiting` 관측은 0이었다. **PONG 미완료·입력 대기 표시 FAIL**이며 신뢰 확인을 선택하지 않았다(사용자 Codex 설정을 쓰게 되므로). Ctrl-C 뒤 `toolExited=true/state=review` 종료는 관측했다. Codex 설정 SHA256 불변, 소유 저장소·worktree·branch·drop-in·socket 정리, scope `not-found/inactive` 확인. 증거: Git common-dir `devbox-verification/s2-task1-20261020/{codex-result,codex-states,codex-after-interrupt,codex-config-before,codex-config-after}.json`·`codex-pane.txt`·runner. 이 결과는 S2 비차단 이월이며 이번 상세화에서 S1 기능 수정을 추가하지 않는다. 준비 runner의 최초 시도는 fixture `XDG_RUNTIME_DIR` 때문에 user bus에 못 붙어 실패했고, 자원을 정리한 뒤 실제 user bus+고유 test socket으로 바로잡았다. 실제 CLI 실행은 한 번이다.

- [x] **Step 4:** 별도 검토의 환경·인스턴스 전달, ThisPc 실행 예외, 저장 소유 경계, 출력 순서 지적을 보정하고 재검토·Biome CI(276개 파일)를 통과해 로컬 커밋한다(PR 없음). PR은 Task 16에서 한 번만 연다.

```bash
git add docs/superpowers/plans/2026-10-08-devbox-v1/{05-s2-runs,PROGRESS}.md
git -c user.name=jihoon22-lee -c user.email=188744452+jihoon22-lee@users.noreply.github.com commit -m "docs(plan): detail S2 against the S1 code"
```

---

### Task 2: 실행 정의 — toml + 이 PC 정의 병합

**단계별 실행:**
- [x] **1 · 실패 테스트:** `crates/runtime/src/defs_tests.rs`·projects `trust_tests`·CLI `runtime_definitions.rs`에 아래 출발 assertion과 이 과제의 핵심 테스트를 먼저 작성한다.
```rust
let merged = merge(vec![file("build", "echo file")], vec![local("build", "echo local")])?;
assert_eq!(merged.defs[0].source, DefinitionSource::ProjectFile);
assert_eq!(merged.shadowed.len(), 1);
assert_eq!(required_scope_version(&config_with_task()), Some(2));
```
- [x] **2 · 실패 확인:** `cargo test -p devbox-runtime merge_prefers_project_file_and_reports_shadowed`를 자원 제한 아래 실행한다. 미구현 타입/행동 때문에 실패했는지 확인하고 RED 출력을 artifact에 남긴다(환경·잠금 실패는 RED가 아님).
- [x] **3 · 구현:** 선행 config/input 분할을 기존 시험으로 확인·별도 커밋한다. `run_config` typed DTO·`RuntimeError`·의존 DFS 검증·thisPc DB 저장을 구현한다. 프로젝트 파일은 ProjectsService::update_run_config의 typed 편집 helper에 맡겨 파일 수정·승인/rev를 한 도메인이 소유하게 한다. 반환된 config/rev로 runtime 결과와 changed를 갱신하며 프로젝트 정의를 runtime DB에 이중 저장하지 않는다. `required_scope_version`과 `store::approved` 최소 버전 판정, typed trust preview, Run operation을 같은 과제에서 연결한다. 신뢰 오류는 typed match로 `RuntimeError::ProjectTrustRequired { id } = "projects.trust_required"`에 매핑한다. 생성기 `TsExporter.codes`는 BTreeSet이므로 기존 코드와 공존하며 runtime 메서드의 typed error union에 포함된다(문자열 오류 탐지 금지). cron 구문 검증은 Task 5에서 쓸 v0.9.0 parser의 순수 부분을 여기서 함께 이식해 중복 parser를 만들지 않는다. `RuntimeService::{list,define,undefine}`를 CLI routes/state/migrations·xtask에 등록하고 300ms watcher는 변경 때만 rev를 발행한다. `runtime.define` 결과는 `{ rev, definition }`이며 수정할 때 같은 ID를 보존한다.

```rust
pub fn required_scope_version(c: &ProjectConfig) -> Option<u32> {
    if !c.raw_tasks.is_empty() || !c.raw_services.is_empty() || !c.raw_schedules.is_empty() {
        Some(2)
    } else if has_scope_one_commands(c) { Some(1) } else { None }
}
// Query: same id/path/digest AND scope_version >= required_scope.
// ProjectFile: projects.update_run_config(id, edit) owns checked TOML write + approval/rev.
// ThisPc: runtime DB definition write; file definitions are never duplicated in runtime DB.
```

- [x] **4 · 통과:** 같은 RED 명령과 직접 영향 시험으로 GREEN을 확인한다. 모든 핵심 테스트 및 프로젝트 신뢰·등록 회귀를 실행한다. 외부 변경 직후 저장/동시 저장과 파일쓰기·DB쓰기 실패를 주입해 승인 부당 승격/중복 생성이 없음을 확인한다. `pnpm gen`으로 생성 타입을 확인한다.
- [x] **5 · 검토·커밋:** 별도 검토·Biome CI 뒤 `feat(runtime): add execution definitions and scope two trust`. 실패·미실행 항목을 PROGRESS에 구분해 기록한다.

**Files:**
- Create: `crates/runtime/{Cargo.toml,src/lib.rs,src/model.rs,src/defs.rs,src/store.rs}`
- Modify: `crates/projects/src/config.rs`·`trust.rs`·`model.rs`·`store.rs`, `crates/cli/src/daemon/{routes,state,mod}.rs`, `xtask/src/main.rs`
- Create: `crates/projects/src/run_config.rs`(직렬화 가능한 실행 정의), `config/{copy,tests}.rs`(위 분할 계획). `raw_tasks`·`raw_services`·`raw_schedules`는 이미 public이며 `serde(skip_serializing)`·`ts(skip)` 상태다. 새 공개화 대신 단일 파싱·타입 미리보기 계약을 만든다.

**Interfaces:**
- `TaskDef { id, command, cwd?, env: BTreeMap, target: Linux|Windows, depends_on: Vec<id>, worktree?: "main"|"<agent id>", secrets: Vec<name> }`
- `ServiceDef { 위 + restart: No|OnFailure|Always, restart_sec, start_limit: { burst, interval_sec }, ports_hint: Vec<u16>, ready_timeout_sec: u32(기본 60) }`
  - health 검사 필드는 없다. 재시작 원인은 프로세스 종료뿐이다(01-design §9.1-17, AR-F02).
  - `depends_on`에 서비스가 있으면 그 서비스가 "준비됨"(Task 8)이 될 때까지 기다린 뒤 다음을 시작한다. `ports_hint`가 비어 있는 서비스는 시작 즉시 준비됨이다.
- `ScheduleDef { id, task, cron, tz?, missed: Skip|RunOnce }`
- `Definition { source: ProjectFile|ThisPc, def }`
- 저장 경계: 프로젝트 정의는 `.devbox/devbox.toml`이 원장, thisPc 정의·runs는 runtime DB가 원장이다. `ProjectsService::update_run_config(id, RunConfigEdit) -> R<UpdatedRunConfig { config, rev }>`가 프로젝트별 쓰기 직렬화·직전 바이트 재검증·파일 수정·승인/rev DB 쓰기를 소유한다. runtime은 helper 결과 rev로 `runtime.changed`를 발행하고 다른 도메인의 SQL이나 프로젝트 정의 DB 사본을 만들지 않는다.
- `merge(file_defs, local_defs) -> (Vec<Definition>, Vec<Shadowed>)`: 같은 ID는 프로젝트 파일이 이긴다. 가려진 정의는 화면에 출처와 함께 보인다.
- 검증 오류 `RuntimeError::DefInvalid { id, reason }`: 순환 의존, 없는 의존, 빈 명령, 잘못된 cron
- 메서드:
  - `runtime.list{ projectId }` → `{ rev, defs, runs: 최근 실행 요약 }`
  - `runtime.define{ projectId, def, saveTo: project|thisPc }`(mutation): 프로젝트 파일에 쓰면 `toml_edit`로 해당 표만 바꾸고 주석·순서를 보존한다.
  - `runtime.undefine{ projectId, id }`(mutation, always)
- 프로젝트 `.devbox/devbox.toml` 변경 감시(`notify`, 300ms 디바운스) → `runtime.changed`. 다시 읽은 설정의 승인 상태(승인됨·낡음·필요 없음)를 `runtime.list` 결과에 함께 싣는다.
- 신뢰 범위 2(01-design §7):
  - `SCOPE_VERSION = 2`. 설정의 실행 정의 최소 범위를 계산해, task/service/schedule이 없으면 범위 1 이상 승인을, 있으면 범위 2 승인을 요구한다. 승인 미리보기에 task/service/schedule의 명령·작업 폴더·환경 변수 이름과 값·대상·의존 순서·비밀 이름을 더한다(비밀 값은 없음).
  - `runtime.define`/`undefine`이 프로젝트 파일에 쓸 때: 쓰기 직전 바이트가 유효한 상태(필요한 범위로 승인됨, 또는 실행 정의가 없어 승인 불필요)였으면 ProjectsService typed helper가 쓴 바이트의 digest로 현재 `SCOPE_VERSION`(2) 승인과 rev를 같은 DB 쓰기에서 기록한다(프로젝트 정의 DB 행은 만들지 않음). 승인이 필요한데 승인과 다른 바이트였으면(밖에서 고침) 기록하지 않는다. `saveTo: thisPc`는 승인과 무관하다.

**핵심 테스트:**
- `merge_prefers_project_file_and_reports_shadowed`
- `rejects_dependency_cycles_with_the_cycle_path`(`a → b → a`의 경로를 오류에 담음)
- `define_into_project_file_preserves_comments`(주석과 다른 표가 그대로)
- `watch_reloads_on_external_edit`(파일을 고치면 `runtime.changed` 수신, 승인 상태가 낡음으로 바뀜)
- `scope_one_approval_stays_valid_without_run_definitions`·`run_definitions_require_scope_two_approval`
- `define_into_approved_file_refreshes_the_approval`·`define_after_external_edit_keeps_the_approval_stale`·`first_define_into_file_without_run_definitions_needs_no_prompt`
- `project_definition_write_is_serialized_and_rejects_changed_bytes`·`project_file_write_failure_creates_no_approval`·`approval_db_failure_does_not_report_save_success`·`project_definitions_are_not_duplicated_in_runtime_db`

---

### Task 3: `devbox exec` 래퍼와 `devbox run-exited`

**단계별 실행:**
- [x] **1 · 실패 테스트:** `crates/runtime/src/logfile_tests.rs · crates/cli/tests/runtime_wrapper.rs`에 아래 출발 assertion과 이 과제의 핵심 테스트를 먼저 작성한다.
```rust
let output = run_wrapper(["sh", "-c", "echo out; echo err >&2; exit 3"]).await;
assert_eq!(output.exit.code, Some(3));
assert_eq!(output.marker_text_set(), set![("o", "out"), ("e", "err")]);
// Independent stdout/stderr pipes do not guarantee cross-stream ordering.
assert!(!output.argv.contains("owned-token"));
```
- [x] **2 · 실패 확인:** `cargo test -p devbox-cli --test runtime_wrapper wrapper_records_lines_with_stream_markers_and_exit_code`를 자원 제한 아래 실행한다. 미구현 타입/행동 때문에 실패했는지 확인하고 RED 출력을 artifact에 남긴다(환경·잠금 실패는 RED가 아님).
- [x] **3 · 구현:** `main.rs`/`lib.rs`에 Exec·RunExited 명령을 연결하고 process group·TERM 전달, bounded line reader, log writer/rotation, atomic exit 파일을 구현한다. `run-exited`는 기존 wrapper 결과를 덮지 않는다. run attempt 시작 전에 이전 attempt 결과를 분리해 오래된 exit가 새 실행을 끝난 것으로 만들지 않는다. stdin 비밀 파서는 NUL 구분을 읽고 자식 환경만 채우며 로그 기록 전 값 가림 필터를 둔다(Task 11에서 실제 키 저장소와 연결).

```rust
let secrets = read_secret_stdin().await?;
let mut child = spawn_process_group(command, &secrets)?;
copy_masked_streams(&mut child, &mut writer, &secrets).await?;
write_exit_atomic(&paths, run_id, child.wait().await?).await?;
// run-exited creates a fallback only when this attempt has no wrapper exit.
```

- [x] **4 · 통과:** 같은 RED 명령과 직접 영향 시험으로 GREEN을 확인한다. 핵심 시험에 invalid UTF-8·1MiB 줄·SIGTERM 자식 그룹·동시 stdout/stderr·오래된 exit/중복 exited 이벤트를 포함한다. 비밀 전달 시험의 판정은 원시 로그 평문이 아니라 자식 내부 값 일치 후 고정 ACK다.
- [x] **5 · 검토·커밋:** 별도 검토·Biome CI 뒤 `feat(runtime): add execution wrapper and durable exit records`. 실패·미실행 항목을 PROGRESS에 구분해 기록한다.

**Files:**
- Create: `crates/cli/src/exec.rs`, `crates/cli/src/run_exited.rs`, `crates/runtime/src/logfile.rs`
- Modify: `crates/cli/src/main.rs`

**Interfaces:**
- `devbox exec --run <runId> [--attempt <attemptId>] [--secrets-stdin] -- <명령...>`
  - 자식을 띄운다. 비밀이 있으면 표준 입력의 `NAME=값\0` 목록을 읽어 자식 환경에만 넣는다.
  - stdout·stderr를 줄 단위로 `runs/<runId>.log`에 쓴다(`logfile::Writer`, 64 MiB 회전).
  - 끝나면 `runs/<runId>.exit`(JSON `{ attemptId, code, signal?, endedMs, by: "wrapper" }`)를 쓴다.
  - 데몬 소켓으로 `runtime.exited`의 typed `{ runId, attemptId }`를 보낸다(500ms 내 best-effort). Task 4가 실제 handler/현재 Run 상태를 연결한다.
  - SIGTERM을 받으면 자식 프로세스 그룹에 전달한다. 직접 자식 종료 뒤 남은 그룹은 TERM→200ms→KILL, 출력 drain은 최대1초로 끝내며 원래 자식 코드/신호를 그대로 반환한다.
  - attempt 선택은 명시 옵션 → `INVOCATION_ID` → UUID. 현재 `.attempt`, 이전 attempt의 `.retired`와 별도 `.exit`를 같은 run 잠금 아래 원자적으로 기록해 오래된 결과가 새 실행을 끝내지 못하게 한다. 실제 systemd 재시작에서는 고정 `--attempt`를 재사용하지 않는다.
- `devbox run-exited --run <runId> [--attempt <attemptId>]`(ExecStopPost):
  - 이 attempt의 `.exit`가 없을 때만 `$SERVICE_RESULT`·`$EXIT_CODE`·`$EXIT_STATUS`로 `{ by: "systemd" }` 결과를 쓴다. 명시 옵션/`INVOCATION_ID`가 없으면 현재 attempt를 사용한다.
  - 선택한 ID와 `INVOCATION_ID`가 같을 때만 ExecStopPost→restart 순차 계약을 이용해 begin 이전 실패도 확정한다. 이미 retired된 ID는 항상 무시하며 일반 wrapper 보고는 현재 attempt만 허용한다. 실제 systemd 순차성 검증은 Task 4다.
- `logfile::Writer::write_line(stream, bytes)`
  - 시각은 받은 시각이다.
  - 줄이 1 MiB를 넘으면 자르고 `…(잘림)`을 붙인다.
  - UTF-8이 아니면 손실 변환한다.
- `logfile::read_range(path, from_offset, max_lines) -> (lines, next_offset)`
  - 내부 reader: byte offset, 최대256줄, 각 물리 줄 최대 `3×1MiB+128` 바이트. 미완성 마지막 줄은 offset을 소비하지 않으며 EOF 초과 offset은0으로 재설정한다. Task 7의 64KiB stream 청크·검색 RPC 768KiB 예산은 별도 강제한다.

**핵심 테스트:**
- `wrapper_records_lines_with_stream_markers_and_exit_code`: `sh -c 'echo out; echo err >&2; exit 3'` → 두 줄(`o`·`e`), exit 3
- `secrets_from_stdin_reach_the_child_but_not_argv`: 자식이 `$TOKEN` 일치 여부를 검사해 고정 ACK만 출력하고, `/proc/<pid>/cmdline`에는 값이 없는지 확인. 값을 그대로 출력하는 별도 redaction fixture도 원시 로그에 평문이 없어야 함
- `log_rotation_keeps_the_tail_and_marks_truncation`: 상한을 작게 주입해 확인
- `run_exited_does_not_overwrite_wrapper_result`
- `run_exited_records_killed_wrapper`: 래퍼를 `kill -9` → `ExecStopPost` 흉내 → `by: systemd`, `signal: KILL`

---

### Task 4: 실행 시작·중지·재시작·맞춤

**단계별 실행:**
- [x] **1 · 실패 테스트:** `crates/runtime/src/launch_tests.rs · crates/cli/tests/runtime_lifecycle.rs`에 아래 출발 assertion과 이 과제의 핵심 테스트를 먼저 작성한다.
```rust
let args = unit_args(&service_no_restart(), "r1", &paths, Some(&owned_slice));
assert!(args.iter().any(|s| s == "Restart=no"));
assert_eq!(start_with_stale_trust().await.created_units, 0);
```
- [x] **2 · 실패 확인:** `cargo test -p devbox-runtime unit_args_for_service_with_restart_policy`를 자원 제한 아래 실행한다. 미구현 타입/행동 때문에 실패했는지 확인하고 RED 출력을 artifact에 남긴다(환경·잠금 실패는 RED가 아님).
- [x] **3 · 구현:** `core::systemd`는 setup의 기존 systemctl 동작을 보존해 이동하고 transient/stop/reset_failed/show를 추가한다. `RuntimeService`를 `Services.runs`에 연결, start/restart는 `authorize_run_graph`의 출처 판정→필요한 `TrustOperation::Run` snapshot 확인→전 의존 입력 검사→unit 생성 순서다. thisPc-only 그래프는 프로젝트 승인을 요구하지 않는다. `Run`에 `ready`, PID, attempt와 `restarts`를 포함한다. systemd만 재시작하며 runtime은 exit·unit 상태와 DB를 reconcile한다. Task 14 알림의 작은 `notify.raised` 타입·발행자를 먼저 만들어 한도 초과를 한 번 발행한다. CLI launch는 서비스·터미널·에이전트 전체 snapshot으로 preflight한다. readiness는 주입 가능한 검사 함수로 Task 4의 순서·timeout을 완성하고 Task 8에서 실제 proc 구현을 연결한다.

```rust
pub async fn start(&self, p: StartRun) -> Result<Run, Fail<RuntimeError>> {
    let prepared = self.authorize_run_graph(&p).await?;
    self.start_prepared_graph(prepared).await
}
// authorize_run_graph: resolve sources; project-file nodes require a snapshot, ThisPc-only does not.
// start_prepared_graph: preflight all nodes → ordered start/readiness → persist + changed.
// manual restart: authorize current snapshot → reset_failed → fresh attempt.
```

- [x] **4 · 통과:** 같은 RED 명령과 직접 영향 시험으로 GREEN을 확인한다. 순수 인자 시험 뒤 고유 slice L2를 실행한다. `exit 1`, burst 3, restart=no 10초/0회, daemon 재시작·수동 reset-failed, stale trust 자원 0, 3초 준비·2초 timeout을 그대로 검사한다. CI systemd 불가만 명시 ignore, WSL 실행은 필수다.
- [x] **5 · 검토·커밋:** 별도 검토·Biome CI 뒤 `feat(runtime): manage transient runs and reconcile service state`. 실패·미실행 항목을 PROGRESS에 구분해 기록한다.

**Files:**
- Create: `crates/runtime/src/launch.rs`, `crates/runtime/src/reconcile.rs`, `crates/runtime/src/service.rs`
- Create: `crates/core/src/systemd.rs`(S0b `crates/cli/src/setup.rs`의 `systemctl` 호출 helper를 이리로 옮기고 `run_transient`·`stop`·`reset_failed`·`show`를 더한다. 오류는 `CoreError`)

**Interfaces:**
- `Run { id, projectId, defId, kind: Task|Service|Scheduled, state: Starting|Running|Restarting|Exited|Failed|Stopped, exit?, startedMs, endedMs?, restarts, unit, ready, pid?, port?, attemptId? }`
- `launch::unit_args(def, run_id, paths, slice?) -> Vec<String>`
  - `unit_args_with_executable(..., executable)`는 순수 인자 빌더이며 기존 `unit_args`는 current_exe 편의 호출이다. daemon의 ExecutionConfig는 실제 실행 바이너리를 전달한다. 단위 테스트로 전체 배열을 고정한다.
  - 속성: `--unit`, `--collect`, `WorkingDirectory`, `EnvironmentFile=<paths.instance_config_dir>/env`, 비밀 없는 `Environment=PYTHONUNBUFFERED=1 FORCE_COLOR=1 DEVBOX_PORT=…`, `KillMode=control-group`, `MemoryAccounting=yes`, `ExecStopPost=<devbox> --instance <i> run-exited --run <id>`. EnvironmentFile은 setup::capture_env의 whitelist(PATH·LANG·LC_*·PNPM_HOME 등)만 담은 기존 인스턴스 env 파일을 쓴다. 비밀 평문을 env 파일에 추가하지 않는다.
  - main exec·매 attempt helper·ExecStopPost 모두 global `--instance <paths.instance>`(또는 동일한 `DEVBOX_INSTANCE`)와 동일한 HOME/XDG_CONFIG_HOME/XDG_DATA_HOME/XDG_STATE_HOME/XDG_RUNTIME_DIR를 전달한다. 각 프로세스의 Paths 재계산 결과가 데몬과 같아야 하며 test/accept가 기본 prod 디렉터리·소켓·exit 파일을 읽거나 쓰면 안 된다. unit-file용 setup의 raw 경로/% 이스케이프는 보존한다. transient D-Bus 인자는 `%`를 그대로 전달하고 단어의 따옴표/역슬래시만 quote한다(systemd가 파일 저장 때 specifier를 escape하므로 이중 escaping 금지).
  - `--property=Type=exec`와 `--expand-environment=no`를 쓴다. 서비스는 `--collect`와 `CollectMode=inactive`를 함께 써 정상 종료만 GC하고 실패는 관측·수동 reset까지 보존한다. systemctl/systemd-run 제어 자식만 `/run/user/<uid>`를 사용하며 실행 unit의 HOME/XDG는 Paths 그대로다.
  - captured EnvironmentFile은 그대로 두고 정의 env를 wrapper의 실제 child argv `/usr/bin/env -- KEY=VALUE ... /bin/bash -c command`에 최종 적용한다(EnvironmentFile이 Environment 속성보다 우선하기 때문). `child_args`와 core의 전체 argv 64KiB/NUL 검사를 preflight와 wrapper가 공유한다.
  - `DEVBOX_PORT`는 명시 env/ports_hint → 선택 agent의 기존 port → main의 빈 loopback ephemeral port 순서다. env와 hint가 다르면 거절하고, 같은 그래프의 자동 포트는 중복 없이 골라 Run/인자에 고정한다. 자동 포트는 수동 재시작에도 보존한다. 임시 bind를 닫은 뒤 실제 서비스 bind까지의 경합은 일반 bind 실패로 처리한다.
  - 서비스는 `Restart`·`RestartSec=1s`·`RestartSteps=5`·`RestartMaxDelaySec=30s`·`StartLimitBurst`·`StartLimitIntervalSec`를 더한다(빠른 반복 종료의 간격이 1초 → 30초로 늘어남, AR-F14, IR-5). `restart = "no"`면 `Restart=no`다.
- 메서드:
  - `runtime.start{ projectId, id, worktree? }`(mutation): 의존 작업을 순서대로 실행한다. 앞 단계가 실패하면 멈춘다.
  - `runtime.stop{ runId }`(mutation, reversible)
  - `runtime.restart{ runId }`(mutation): 실패 상태면 `reset-failed` 뒤 시작한다.
  - `runtime.exited{ runId, attemptId }`(내부): Task 3의 현재 attempt 종료 파일을 읽어 맞추며 호출자가 종료 내용을 덮지 않는다.
  - `runtime.runs{ projectId?, limit }`(query)
- 데몬이 시작할 때 맞춘다: `systemctl --user list-units 'devbox-<i>-run-*' --output=json` + `.exit` 파일 → 상태를 갱신한다.
- 서비스 재시작은 `systemctl show -p NRestarts,ActiveState,Result`를 실행 이벤트 때와 10초 간격(서비스가 있을 때만)으로 읽어 `restarts`를 갱신한다.
- 한도 초과는 `failed`이며 영속 start_limit_notified 표식과 `notify.raised`를 한 번 발행한다. `Result=start-limit-hit`를 우선하되 systemd 259가 이전 `exit-code` 등을 유지하는 경우, restart 정책·현재 종료 원인·failed/subfailed·NRestarts>=burst·burst/interval>0가 모두 맞을 때만 한도 초과로 판정한다([systemd #32203](https://github.com/systemd/systemd/issues/32203)). 재시작은 계속 systemd만 한다.
- 시작 구성의 `services`를 `prepare_launch_snapshot`에서 같은 승인 snapshot과 한 번 읽은 local 정의로 합친 DAG로 만든다. 여러 root가 공유하는 의존은 1회만 생성한다. terminal/agent까지 전체 preflight를 끝낸 뒤 실행하며 `StartedRun` collector로 readiness/의존 실패에도 생성된 services/runIds를 LaunchFailed에 포함한다. runtime→agents 의존 없이 CLI 주입 resolver가 agent 소유 worktree/path/port를 확인한다.
- `runtime.start`·`runtime.restart`·의존 작업 시작은 `authorize_run_graph`로 ProjectFile 노드가 있을 때만 unit 생성 전 신뢰를 확인하고 승인 snapshot의 정의로 `unit_args`를 만든다. thisPc-only 그래프는 같은 prepared graph 경로를 쓰되 승인 snapshot이 필요 없다. 승인이 없거나 낡으면 자원을 만들지 않고 `projects.trust_required`를 돌려준다. systemd `Restart=`의 자동 재시작은 확인하지 않는다(이미 만든 unit).

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
- `unit_args_preserve_instance_environment_file`·`exec_helper_and_stop_post_share_instance_and_xdg_paths`: test/accept + 소유 XDG root fixture에서 EnvironmentFile·exec/helper/ExecStopPost의 인스턴스/경로 인자를 비교하고, 같은 Paths의 로그·exit·secret socket만 가리키며 prod 접근0을 확인한다.
- systemd user 세션을 CI에서 못 쓰면(S0a 판정) L2 중 systemd 시험은 `#[ignore = "needs systemd user"]` + WSL 로컬 필수로 둔다(00-roadmap 실행 규칙).

**Task 4 구현 경계:** 기본 readiness는 ports_hint 없는 실행만 준비 처리하고, 주입 검사로 실제 L2 3초 준비·2초 timeout/검사 정지·stop 경합을 검증한다. Task 8이 proc 관찰을 같은 Readiness 함수에 연결한다. Windows target(Task 12), secrets/`{{secret:...}}`(Task 11)은 현재 typed 사전 거절하며 무주입 Linux 실행으로 대체하지 않는다. Task 5 예약/Task 7 로그 RPC/Task 14 UI는 미구현 상태를 유지한다.

---

### Task 5: 예약(cron)·놓친 예약·실패 알림

**단계별 실행:**
- [x] **1 · 실패 테스트:** 새 예약의 현재 시각 baseline/API 테스트를 먼저 등록했다. cron 원본 10개·어댑터 4개도 구현 전에 등록했다. 이후 missed·신뢰·완료·수명 확장 검증을 추가했으며, 삭제/재추가와 재승인 직후 재편집은 각각 별도 assertion/timeout RED로 확인했다. 아래는 검증한 핵심 계약이다.
```rust
h.scheduler.tick(true).await?; // 최초 baseline
h.advance(100);              // fake clock, minutes
h.scheduler.tick(true).await?; // 재시작 missed 정책
h.scheduler.tick(false).await?;
h.scheduler.drain().await;
assert_eq!(h.count(), 1);
assert_eq!(stale.outcome().await, Some(ScheduleOutcome::SkippedTrust));
```
- [x] **2 · 실패 확인:** `cargo test -p devbox-runtime scheduler`를 자원 제한 아래 실행한다. 초기 Scheduler/SchedulesQuery/schedules 미구현 API와 cron occurrence API/adapter 미구현 오류를 기록했다. `devbox-verification/s2-task5/{scheduler-red,cron-red,readd-red,approval-coalesced-red}.log`가 RED 근거다. fixture payload 호출 오타와 개발 중 serde_json 의존성 오류는 기능 RED가 아닌 컴파일 수정으로 구분한다.
- [x] **3 · 구현:** `git show v0.9.0:crates/runtime-engine/src/core/cron.rs`의 `CronSchedule::parse`·`validate_cron`·`next_occurrences_in_timezone`·`next_wall_occurrences`와 기존 DST 시험을 이식한다. `croner` parser를 그대로 사용하며 별도 cron 라이브러리로 바꾸지 않는다. 기존 테스트를 `cron_tests.rs`로 분리해 파일 600줄 상한을 지키고, scheduler가 한 시각을 요구할 때만 아래 thin adapter를 둔다. Scheduler는 다음 시각 sleep_until·변경 알림·shutdown cancellation을 선택한다. `last_fired`/skip 결과를 DB에 기록하고 중복 fire를 막는다. 매 fire와 missed RunOnce는 `authorize_graph` 공통 경로로 schedule 자체·task·의존의 출처를 함께 확인하며, 하나라도 ProjectFile이면 `run_snapshot`을 요구한다. 실행은 그 snapshot과 thisPc 정의를 고정한 `PreparedGraph`만 받는다. ProjectsService의 typed `approval_stamp`는 기존 승인 기록 변화만 표시하며 권한으로 쓰지 않는다. 이를 runtime state에 저장해 승인 이벤트가 합쳐져도 trust 알림을 승인 변경 전 1회로 묶는다. 같은 schedule ID의 cron 수정은 baseline만 갱신하고 approval_stamp가 같으면 알림 표식을 유지한다. 삭제/재추가는 새 identity로 처리한다.

```rust
loop {
    tokio::select! {
        _ = stop.cancelled() => break,
        _ = changed.recv() => {},
        _ = approvals.recv() => {},
        _ = tokio::time::sleep_until(deadline), if next.is_some() => {},
    }
    next = scheduler.tick(false).await?;
}
scheduler.stop().await; // worker 취소, 이미 시작한 systemd unit은 유지
// tick uses authorize_graph with the schedule source; trust denial persists SkippedTrust.
```

- [x] **4 · 통과:** cron 전체 15개(기존 10+adapter 4+이전 parser 계약 1), fake-clock scheduler 15개, 기존 runtime service/graph/watch 직접 영향 18개 PASS. 실제 CLI/systemd 소유 fixture 2개는 production 초기화·`runtime.schedules` RPC·Scheduled kind·비동기 exit7 실패 알림·중복 이벤트 억제·SIGTERM 종료/기존 unit 생존·재시작 복구·명시 stop의 stopped terminal/무알림·같은 승인 상태의 cron-only 수정 알림 억제를 확인했다. gen-ts 생성/--check PASS. 추가 직접 영향 결과와 raw/resource 파일은 PROGRESS 및 `s2-task5/summary.json`에 기록한다.
- [x] **5 · 검토·커밋:** 별도 검토 P2 두 건을 실제 RED→GREEN으로 고치고 최종 미해결0·Biome278 PASS 뒤 `feat(runtime): schedule trusted tasks with missed-run policies`로 로컬 커밋했다. 실패·미실행·claim-before-launch 한계와 근거는 PROGRESS 및 `s2-task5/{summary,review}.json`에 구분했다.

**Files:**
- Create/port: `crates/runtime/src/{cron,cron_tests,scheduler,scheduler_tests,schedule_model,schedule_service,schedule_store}.rs`
- Modify: runtime graph/execution/run_store/watch/lib, core notify, projects typed approval marker, CLI routes·실제 `runtime_schedules` fixture, 생성 `app/src/rpc/gen/rpc.ts`

**Interfaces:**
- 기존 API: `CronSchedule::parse`·`validate_cron`·`next_occurrences_in_timezone`·`next_wall_occurrences`를 이식한다(`croner` 3.0.1 parser, 기존 wall-time/DST 정책 유지).
- `cron::next_after<Tz: TimeZone>(expr: &str, tz: &Tz, after: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, CronError>`는 기존 함수 이름이 아니라 위 occurrence API에 count=1을 요청해 첫 항목을 반환하는 필요한 thin adapter다. parse 오류를 `None`으로 삼키지 않는다.
- `Scheduler`: 다음 실행 시각 하나만 잡고 `tokio::time::sleep_until`로 기다린다(폴링 없음). 정의가 바뀌면 다시 계산한다.
- 데몬 시작 시 놓친 예약:
  - `last_fired`(DB)와 지금 사이의 예약 시각이 있으면 `missed` 정책을 따른다.
  - `RunOnce`면 한 번 실행하고, `Skip`이면 "건너뜀"을 기록한다.
- 기존 core `notify.raised`의 `Notification { id, projectId, runId: string|null, task: string|null, reason }`를 확장한다. 기존 reason=`startLimit`을 보존하며 예약 실패는 `reason="schedule_failed"`다. 실제 생성된 target/dependency run ID가 있으면 넣고, preflight 실패에는 null을 쓴다. `save_run`과 schedule terminal 결과/알림 표식을 같은 DB transaction에 기록해 start 반환 뒤 종료도 반영하고 늦은 Running 결과가 terminal 상태를 덮지 못한다. 수동 stop은 stopped terminal로 기록하고 schedule_failed 알림을 보내지 않는다.
- 예약은 무인 실행이라 실행 시점마다 신뢰를 확인한다. 승인이 없거나 낡았으면 실행하지 않고 `skipped(trust)`를 기록하고 알림 `reason="schedule_trust_required", projectId, task, runId=null`를 승인이 바뀔 때까지 한 번만 띄운다. 놓친 예약(`RunOnce`)도 같다.
- 메서드 `runtime.schedules{ projectId }` → `{ items: [{ id, cron, next: string|null, last: { at, result }|null }] }`. 시각은 UTC RFC3339이며 result는 claimed/running/succeeded/failed/stopped/skippedMissed/skippedTrust다.
- runtime migration 3의 state(cursor/definition/approval marker)와 occurrence claim/result만 실행 원장이다. 새 정의·삭제 뒤 재추가는 현재 시각을 baseline으로 삼는다. startup missed는 구간당 한 번 소비하며, 반복 tick/restart는 CAS cursor+occurrence key로 중복 실행을 막는다.
- slow graph는 독립 worker로 실행한다. 종료는 scheduler worker를 취소하고 후속 노드를 시작하지 않으며, 이미 만들어진 systemd unit은 runtime 수명 정책대로 보존한다.
- claim commit 직후 unit 시작 전 프로세스 강제 종료는 claimed 상태로 남아 재실행하지 않는다. DB와 systemd 간 원자적 exactly-once를 보장한다고 표현하지 않는다.

**핵심 테스트:**
- 이식한 `cron.rs` 테스트 전부(DST 포함)
- `missed_run_once_fires_exactly_once_after_downtime`
- `missed_skip_records_without_running`
- `scheduler_recomputes_when_definitions_change`(가짜 시계)
- `stale_trust_skips_scheduled_run_and_notifies_once`(가짜 시계, 두 번째 예약 시각에도 알림 추가 없음)

---

### Task 6: 작업 자동 감지

**단계별 실행:**
- [x] **1 · 실패 테스트:** `crates/runtime/src/detect_tests.rs · fixture 폴더`에 아래 출발 assertion과 이 과제의 핵심 테스트를 먼저 작성한다.
```rust
let items = detect(&fixture("pnpm"), &existing).await?;
let dev = items.iter().find(|s| s.id == "dev").unwrap();
assert_eq!(dev.command, "pnpm dev");
assert_eq!(dev.kind, DefinitionKind::Service);
assert!(!spawn_marker.exists());
```
- [x] **2 · 실패 확인:** `cargo test -p devbox-runtime detect`를 자원 제한 아래 실행한다. 미구현 타입/행동 때문에 실패했는지 확인하고 RED 출력을 artifact에 남긴다(환경·잠금 실패는 RED가 아님).
- [x] **3 · 구현:** package.json/lockfile·Makefile·justfile·Cargo.toml·compose 파일을 읽는 parser만 구현한다. `RuntimeService::detect`는 기존 ID를 제외하고 제안 출처를 반환하며 어떤 도구도 실행하지 않는다. 제안 저장은 명령을 보여 준 뒤 기존 define 경로를 쓴다.

```rust
pub async fn detect(root: &Path, existing: &[String]) -> Result<Vec<Suggestion>, RuntimeError> {
    let files = read_detection_files(root).await?;
    Ok(parse_suggestions(&files).into_iter().filter(|s| !existing.contains(&s.id)).collect())
} // No Command::new / just --summary / make invocation.
```

- [x] **4 · 통과:** 같은 RED 명령과 직접 영향 시험으로 GREEN을 확인한다. 각 파일 fixture·lockfile 우선순위·기존 ID 제외·악의가 아니라 단순 명령 실행 marker인 just/make fixture에서 프로세스 0을 확인한다. 묶음 O 현재 위치·행·세션을 갱신한다.
- [x] **5 · 검토·커밋:** 직접15개·TS 생성/typecheck·독립 검토 PASS(미해결0)·Biome278 PASS 뒤 `feat(runtime): suggest tasks from project files`로 로컬 커밋한다. 실패·미실행 항목과 원시 근거는 PROGRESS 및 `s2-task6/{summary,review}.json`에 구분했다.

**Files:**
- Create: `crates/runtime/src/detect.rs`, `detect_tests.rs`, `fixtures/detect/`, `crates/cli/tests/runtime_detect.rs`
- Modify: runtime `lib.rs`·`model.rs`, CLI `daemon/routes.rs`, 생성된 `app/src/rpc/gen/rpc.ts`, 기존 오류 `app/src/rpc/messages.ts`

**실제 Interfaces·정책:**
- `detect(root: &Path, existing: &[String]) -> Result<Vec<Suggestion>, RuntimeError>`; `RuntimeService::detect(DetectRuns { project_id }) -> Result<Vec<Suggestion>, Fail<RuntimeError>>`.
- `RuntimeDetect = "runtime.detect"` query. 기존 runtime export가 xtask에 이미 연결돼 있으므로 `RuntimeDetect`를 export·CLI route에 등록하고 `pnpm gen`으로 TS를 생성한다.
- `Suggestion { id, command, kind: DefinitionKind::Task|Service, from: SuggestionSource }`. `kind`의 JSON은 `"task"|"service"`. 출처는 화면에 정확히 표시하도록 초안의 `"compose"`를 실제 파일명 두 값으로 구체화한다: `"package.json"|"Makefile"|"justfile"|"Cargo.toml"|"compose.yaml"|"docker-compose.yml"`.
- package `scripts` 중 `dev`·`start`·`serve`는 서비스, 나머지는 작업. lockfile 우선순위는 `pnpm-lock.yaml` → `yarn.lock` → `package-lock.json` → `npm-shrinkwrap.json` → `bun.lock` → `bun.lockb`; 없으면 npm. 기존 내장 명령과 겹쳐도 script를 실행하도록 `<manager> run <name>`을 사용하며 예시 `pnpm dev`만 안전한 축약을 유지한다. 공백·shell 특수문자는 공통 `sh_quote`, 선행 `-`는 `--` 경계를 쓴다. ID/command는 기존 define 검증을 통과해야 한다.
- Makefile `.PHONY`·정적 타깃과 justfile의 인자 없는 정적 recipe 이름을 직접 읽는다. 변수·pattern target·include 해석·매개변수 recipe 추론은 하지 않는다. Cargo.toml 존재는 `build`/`test`, compose 파일 존재는 `compose-up` 서비스 제안을 만든다.
- 동일 ID는 package → Makefile → justfile → Cargo → compose 순서로 첫 출처가 이긴다. compose.yaml이 docker-compose.yml보다 우선한다. ID 오름차순 반환이며 이 병합 **뒤** 기존 merged 정의 전체 ID를 제외한다(페이지 목록을 호출하지 않는다). 제외된 ID의 낮은 우선순위 출처는 되살아나지 않는다.
- 감지는 읽기 전 일반 파일 여부를 확인하며 파싱 파일당 1MiB, 반환 JSON은 768KiB로 제한한다. malformed package JSON은 위치를 포함한 `runtime.detect_parse`, 파일 I/O/UTF-8 실패는 `runtime.detect_io`(내부 원인 tracing), 크기 초과는 `runtime.detect_limit`을 반환한다. Make/just는 전체 언어 문법 검증기가 아닌 정적 헤더 인식기다.
- 어떤 프로세스도 실행하지 않으며 신뢰 승인을 만들거나 정의를 저장하지 않는다. Task 13 UI가 명령·출처를 표시한 뒤 기존 `runtime.define`으로 저장한다(프로젝트 파일이면 Task 2 승인 규칙). UI 자체는 Task 6에서 추가하지 않는다.

**핵심 테스트:** 각 파일 종류의 fixture 폴더로 제안 목록 비교. 이미 정의된 ID는 제외. 감지가 어떤 프로세스도 띄우지 않는다.

```bash
# 묶음 O 끝
# 묶음 O 끝: 과제별 좁은 검증·별도 검토 뒤 PROGRESS.md의 묶음 O 행과 현재 위치를 갱신해 로컬 커밋한다(00-roadmap §3).
```

---

### Task 7: 로그 — 실시간 tail·검색·소스 병합·파일 tail

**단계별 실행:**
- [x] **1 · 실패 테스트:** `crates/observe/src/logs_tests.rs · crates/cli/tests/run_logs.rs`에 아래 출발 assertion과 이 과제의 핵심 테스트를 먼저 작성한다.
```rust
let merged = merge_sources([source([at(1,"a"),at(3,"c")]), source([at(2,"b")])]);
assert_eq!(merged.map(|l| l.text).collect::<Vec<_>>(), ["a","b","c"]);
assert!(other_rpc_completes_while_log_credit_is_zero().await);
```
- [x] **2 · 실패 확인:** `cargo test -p devbox-observe merges_two_sources_in_timestamp_order`를 자원 제한 아래 실행한다. 미구현 타입/행동 때문에 실패했는지 확인하고 RED 출력을 artifact에 남긴다(환경·잠금 실패는 RED가 아님).
- [x] **3 · 구현:** observe crate와 typed source/filter/search DTO를 만든다. 정규화한 project/vault/var-log 읽기 경계를 확인하고 inotify tail→k-way merge→filter→Ctx open_stream credit 64KiB 송신을 연결한다. 회전 후 offset을 초기화하고 stream close는 reader·docker logs 자식만 종료한다. 검색은 256행/768KiB 한도와 다음 cursor를 반환한다.

```rust
let ends = ctx.open_stream()?;
let stream_id = ends.id;
background.spawn(async move {
    while let Some(line) = sources.next_filtered().await? {
        ends.tx.write(&encode_json_line(&line)?).await?; // transport credit applies
    }
    ends.tx.finish().await;
    Ok::<_, ObserveError>(())
});
Ok(StreamOpened { stream_id }) // Return before reading/waiting for stream credit.
```

- [x] **4 · 통과:** 같은 RED 명령과 직접 영향 시험으로 GREEN을 확인한다. 핵심 시험과 symlink 밖 읽기 거부, invalid regex, 조각 경계/다중 source/취소를 검사한다. 느린 reader 시험은 ping 지연과 buffer 상한을 함께 본다.
- [x] **5 · 검토·커밋:** 직접23개·TS/포맷·독립 최종 검토 PASS(미해결0)·Biome278 PASS 뒤 `feat(observe): stream and search merged execution logs`로 로컬 커밋했다. Docker 탭 parser 리뷰1건 실제 RED→GREEN과 실패·미실행 근거를 PROGRESS 및 `s2-task7/{summary,review}.json`에 구분했다.

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


**Task 7 실제 계약·검증(2026-10-20, 별도 검토·커밋 대기):**
- wire source는 `{kind:"run",runId}` / `{kind:"file",path}` / `{kind:"container",id}`, stream은 `stdout|stderr|system`이다. CLI가 등록 프로젝트 경로·D15 기본 `~/notes`·`/var/log`를 매 요청 주입한다. configurable vault는 S3 Task 4에서 연결한다. 절대 경로인 일반 파일만 읽으며 최초 missing은 `observe.source_unavailable`, 허용 루트 밖 symlink는 거절한다. Run은 DB 기록 조회 후 내부 log path를 얻는다.
- `tail`은 파일 EOF부터, container는 `docker logs --follow --timestamps --tail 0`부터다. `{offset}`은 파일 원시 byte 위치(EOF보다 크면 EOF), container는 stdout/stderr **각 pipe의 원시 byte 위치**를 건너뛴다. Run의 `RFC3339\t o|e|s \t text`를 JSONL로 변환한다. 파일의 EOF 조각은 한 번 내보내고, 뒤 append는 새 조각으로 읽는다.
- available head의 timestamp(ns)·요청 소스 순서로 합친다. container 내부 stdout/stderr도 head를 합치며 동률은 stdout 먼저다. idle source는 기다리지 않으므로 미래에 도착할 과거 시각까지 전역 정렬한다고 주장하지 않는다. 시각 없는 File 행은 읽은 시각이며 검색 진행은 timestamp가 아닌 byte cursor로 유지한다.
- inotify 알림 큐 1개, 소스 최대 8개, 각 File의 원시 행 `3×1MiB+128`·chunk 8KiB·merge head 1개, container의 pipe별 같은 행/chunk 상한·head 2개와 바깥 head 1개다. producer queue는 없다. UTF-8 lossy·JSON escape와 송신 중 DTO의 유한 복사본은 별도이며 위 숫자를 전체 프로세스 RSS 한도라고 보고하지 않는다. 너무 긴 live 행은 읽기를 종료한다. 한 JSONL 행이 64KiB보다 크면 여러 transport chunk로 나뉘며 초기 credit 256KiB 소진 뒤 추가 파일 읽기가 멈춘다.
- 회전 시 inode replacement·현재 길이 감소를 확인해 offset을 0으로 초기화한다. 요청 경로와 현재 canonical target의 부모 집합(최대 16개)을 감시하고 target 변경 때 옛 부모 감시는 해제한다. reopen은 읽기 경계를 다시 확인한다. close·channel disconnect·shutdown에서 reader를 끝내고 소유 `docker logs` 자식만 kill/wait한다. container-only EOF는 잔여 행 뒤 FIN, File이 함께 있으면 File tail을 계속한다.
- 검색은 1–256행·직렬화 JSON 768KiB이며, 응답 text는 UTF-8 경계 64KiB까지만 담고 `[log line truncated]`를 명시한다. cursor는 원본 행 전체를 소비하므로 큰 행·같은 timestamp가 반복되거나 빠지지 않는다. 역방향 reader는 8KiB block·요청당 source별 scan 8MiB, 한 거대 행까지 포함한 hard scan 64MiB다. scan 예산으로 빈 page와 nextCursor가 함께 올 수 있다.
- container 과거 검색은 소유 `docker logs --timestamps`를 최대 합계 64MiB 임시 stdout/stderr snapshot으로 받아 동일한 역방향 reader로 처리한다. 두 pipe를 별개 cursor로 고정해 도착 순서가 바뀌어도 진행을 유지한다. File은 inode와 첫 행 prefix, container는 pipe별 첫 행 prefix를 확인한다. 이미 offset 0인 소스는 새 append를 이전 검색에 섞지 않으며 새 검색에서 본다. snapshot·자식은 요청/취소 완료 때 정리한다.
- 직접 고유 23개(observe16·실제 CLI4·router1·한국어 메시지2), gen-ts/--check·typecheck·관련 Rust fmt PASS. 실제 CLI는 소유 systemd fixture의 실행 stdout/stderr→Run open/search와 File append/close·초기창 소진 후 ping+읽기 상한을 확인했다. 별도 zero-credit opener는 응답 선반환과 head 1개 상한을 확인했다. 실제 Docker 조작·Windows·전체 check/clippy/build/E2E·push/PR은 미실행이다. 독립 검토의 Docker 본문 탭 오인은 native Run/Docker/File 형식과 내부 snapshot 형식을 명시 분리하여 신규2개 RED→GREEN 및 영향 검색으로 수정했다. 원본 RED/환경·fixture 오류 구분·자원·source hash는 `devbox-verification/s2-task7/`에 보존한다.

---

### Task 8: 포트·프로세스·WSL 자원

**단계별 실행:**
- [x] **1 · 실패 테스트:** `crates/observe/src/{ports,processes,system}_tests.rs`의 proc·kill·system assertion을 구현 전에 작성했다. `crates/cli/tests/observe.rs`에 typed RPC·production callback·느린 fake Docker 격리 회귀를 추가했다.
```rust
assert!(is_listening_in(&proc_fixture, port)); // 127.0.0.1, ::1, 0.0.0.0, ::
assert!(!is_listening_in(&established_only, port));
assert!(matches!(kill(stale_start_time).await, Err(ObserveError::StateChanged)));
```
- [x] **2 · 실패 확인:** `cargo test -p devbox-observe is_listening_accepts_ipv4_ipv6_loopback_and_wildcard`를 자원 제한 아래 실행한다. 미구현 타입/행동 때문에 실패했는지 확인하고 RED 출력을 artifact에 남긴다(환경·잠금 실패는 RED가 아님).
- [x] **3 · 구현:** proc parser는 fixture root와 clock을 받는다. inode→PID→cgroup unit 매핑·CPU delta·메모리/PSI/df를 구현한다. kill은 UID·stat starttime을 실제 signal 직전 확인한다. Task 4의 readiness 검사에 실제 is_listening을 CLI에서 주입하여 runtime→observe 순환 의존을 피한다. 준비됨/timeout 후 해당 polling을 멈추며 재시작 정책은 건드리지 않는다.

```rust
pub fn is_listening(port: u16) -> bool {
    read_tcp_tables().iter().any(|row| row.state == Listen && row.port == port)
}
// Runtime receives a readiness callback at CLI composition; observe owns /proc parsing.
// kill checks UID + expected startTimeMs immediately before sending the signal.
```

- [x] **4 · 통과:** 같은 RED 명령과 직접 영향 시험으로 GREEN을 확인한다. 핵심 fixture와 소유 TCP listener/PID 재사용 거부를 확인한다. pid1은 권한 거부만 확인하고 signal을 보내지 않는다. services ports_hint empty/3초 open/2초 timeout 회귀를 재사용한다.
- [x] **5 · 검토·커밋:** 직접38개·타입 검사·독립 최종 검토 PASS(미해결0)·Biome278 PASS 뒤 `feat(observe): inspect ports processes and system resources`로 로컬 커밋했다. readiness cadence 리뷰 지적과 실패·미실행 근거를 PROGRESS 및 `s2-task8/{summary,review}.json`에 구분했다.

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

**Task 8 실제 계약(2026-10-20, 구현·직접 검증·독립 최종 검토·로컬 커밋 완료):**
- `ProcessInfo`는 `pid/name/uid/cwd?/startTimeMs/cpuPercent/rssBytes/unit?/runId?/projectId?`다. `startTimeMs`는 `/proc/stat btime × 1000 + stat22 ticks × 1000 / CLK_TCK`의 Unix ms(정수 내림)다. CPU는 같은 PID·시작 tick의 두 표본 간 `(utime+stime) delta / monotonic elapsed`이며 한 코어 100%, 멀티스레드는 100% 초과 가능하다. 첫 표본·PID 재사용·같은 시각 표본은 0이다. RSS는 stat24의 페이지 수 × OS page size다.
- `processes{projectId}`는 선택 프로젝트 경로 아래 cwd인 **전체** 프로세스를 먼저(CPU 내림차순), 그 외 CPU 상위 50개를 붙인다. 선택이 없으면 전체 CPU 상위 50개다. 동률은 PID 오름차순이다. cwd는 경로 component로 비교하며 중첩 등록에서는 가장 깊은 프로젝트를 표시한다. 사라진/읽을 수 없는 proc 엔트리는 건너뛰고 cwd/cgroup만 읽을 수 없으면 해당 필드를 null로 둔다.
- inode→PID는 오름차순 PID에서 먼저 확인한 소유자(공유 socket은 대표 PID 하나), cgroup은 가장 안쪽 `.service/.scope`를 사용한다. runtime 소유 `live_unit_runs()`는 history의 256개 제한 없이 live unit identity를 제공한다. 포트 주소는 IPv4·IPv6 전체 LISTEN을 인정하며 ESTABLISHED는 제외한다.
- `observe.kill`은 `mutation(always)`이며 성공하면 `{ pid }`를 돌려준다. live `/proc`만 사용해 UID와 원본 start tick을 signal 직전 다시 확인한다. PID 0·i32 범위 밖은 invalid_input, PID 1·다른 UID는 permission_denied, 소멸·시작시각 변경은 state_changed다. 관찰용 fixture root는 signal 대상을 바꾸지 않는다. proc 조회와 OS signal 사이 완전한 원자성은 보장하지 않는다.
- `observe.system`의 메모리/스왑은 바이트, PSI avg10/60/300은 Linux 백분율·totalUs는 누적 μs다. PSI 파일 미지원/권한/파싱 실패는 해당 cpu/memory/ioPressure=null이다. 홈 파티션은 `statvfs`로 df와 같은 total/available/used 바이트를 읽으며 미가용은 homeFilesystem=null이다. 필수 meminfo 미가용은 source_unavailable이다.
- Docker는 읽기 전용 `docker ps --format json`의 게시된 host 포트만 합친다(미게시 expose 제외, TCP/UDP·IPv4/IPv6·범위 지원). 프로그램 없음·비정상 종료·2초 timeout·1MiB 초과·잘못된 JSON은 container 행을 생략하며 OS 포트는 유지한다. 빈 Docker 목록과 미가용을 구별하는 별도 상태 필드는 아직 없다. 모든 검증은 fake CLI/미존재 실행 파일을 사용하며 실제 Docker는 실행하지 않는다.
- proc 조회는 semaphore 2와 spawn_blocking, proc 루트 엔트리 최대 65,536·fd 엔트리 최대 262,144·tcp 표 최대 4MiB·응답 최대 768KiB로 제한한다. 상한 초과는 limit_exceeded다. Docker는 동시에 최대 2개·2초 timeout 뒤 소유 자식 kill/wait다. readiness는 CLI에서 한 번의 tcp/tcp6 snapshot으로 모든 힌트 포트를 검사하며 PID/fd 스캔·접속 시도는 하지 않는다. runtime의 기존 50ms readiness poll을 1초로 맞췄다. 다음 sleep은 deadline을 넘지 않고 callback 전·후 및 sleep 뒤 deadline을 다시 검사한다. 준비/timeout 후 polling 종료·stop/attempt 경합·재시작 정책은 보존한다.

- 직접 고유 38개 PASS: observe unit26(초기 1회), CLI observe3, 실제 systemd readiness5, live unit lookup1, router1, 한국어 오류2. cgroup 조상 unit 선택 오류는 별도 assertion RED→GREEN, 프로젝트 전체 행 보존도 좁은 재검사 PASS다. 독립 검토에서 readiness 50ms 간격을 발견해 2초 동안 callback29회·늦은 첫 검사 뒤 잘못된 성공을 실제 RED2개로 재현했고, 1초 cadence·남은 400ms deadline sleep·기한 뒤 새 callback 없음 및 기존 readiness3개를 함께 GREEN5개로 확인했다. TS 생성·Rust 포맷·diff 검사 PASS. 원본 미구현 RED2건·중간 Done 필드 컴파일 오류1건을 포함한 원본 로그/자원/소스 해시는 Git common-dir `devbox-verification/s2-task8/`에 보존한다. 전체 검사·clippy/build/E2E/Windows·실제 Docker는 미실행(Task16), 최종 Biome·독립 검토는 다음 단계다.

---

### Task 9: 문제 추출

**단계별 실행:**
- [x] **1 · 실패 테스트:** `crates/observe/src/problems_tests.rs`에 아래 출발 assertion과 이 과제의 핵심 테스트를 먼저 작성한다.
```rust
assert_eq!(extract("src/x.ts(10,5): error TS1005: expected", root).unwrap().line, 10);
assert!(extract("../outside.go:10:5: error", root).is_none());
```
- [x] **2 · 실패 확인:** `cargo test -p devbox-observe problems`를 자원 제한 아래 실행한다. 미구현 타입/행동 때문에 실패했는지 확인하고 RED 출력을 artifact에 남긴다(환경·잠금 실패는 RED가 아님).
- [x] **3 · 구현:** 형식별 parser를 순수 함수로 구현하고 project-root 상대 경로·중복 key를 고정한다. rustc/traceback의 문맥이 필요한 경우 source별 작은 parser state에 직전 severity/message만 둔다. 실행 로그 ingest에서 추출하고 run 재시작 때 해당 source만 비운 뒤 observe.problems.changed를 발행한다.

```rust
pub fn extract(line: &str, project_root: &Path) -> Option<Problem> {
    let parsed = parse_supported_format(line)?;
    let file = resolve_within_project(project_root, &parsed.file)?;
    Some(Problem { file, ..parsed })
}
// Insert deduped (source,file,line,col,severity,message); restart clears only source.
```

- [x] **4 · 통과:** 같은 RED 명령과 직접 영향 시험으로 GREEN을 확인한다. rustc/tsc/eslint/biome/vitest/jest/python/go 실제 출력 fixture, 밖 경로·symlink·중복 제거·한 실행 restart가 다른 실행 문제를 지우지 않음을 검사한다.
- [x] **5 · 검토·커밋:** 직접50개·타입·독립 최종 검토 PASS(미해결0)·Biome278 PASS 뒤 `feat(observe): extract source problems from run logs`로 로컬 커밋했다. partial EOF 지적과 wrapper 경계 수정·실패/미실행 근거를 PROGRESS 및 `s2-task9/{summary,review}.json`에 구분했다.

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

**Task 9 실제 계약(2026-10-20, 구현·직접 검증·독립 최종 검토·Biome·로컬 커밋 완료):**
- `observe.problems{projectId, limit?, cursor?}`는 `{rev, items, nextCursor, truncated}`를 반환한다. limit 기본 100·범위 1–100, JSON escape를 포함한 페이지는 768KiB 이하다. cursor는 전역 메모리 rev+offset에 묶이며 변경 뒤 재사용은 `observe.invalid_cursor`다. `observe.problems.changed`의 payload·event rev·Subscribe ACK는 모두 같은 메모리 카운터다. DB rev는 읽기/중복/관찰시각 때문에 증가하지 않는다. 최초 추가·출처 재시작 clear·불완전 목록 표시가 실제로 바뀔 때만 이벤트를 낸다.
- 문제 source는 `run:<runId>`의 안정된 문자열이다. dedup key는 source/file/line/col/severity/message 전체이며 다른 Run의 같은 문제는 별개다. stdout/stderr마다 32행 이내 문맥을 따로 둔다. rustc/cargo header+location, tsc, file:line:col eslint/biome, Vitest/Jest stack, Python 마지막 traceback frame+exception, Go를 지원한다. ANSI CSI/OSC를 제거하며 Unicode/공백 파일명을 보존한다. 경로의 기존 component를 canonical 검사하고 프로젝트 밖 absolute/`..`/symlink를 버린다. 프로젝트 안의 아직 없는 소스 파일은 허용한다. file/message 각각 4KiB, parser 입력 64KiB를 넘는 진단은 버린다.
- 첫 문제 query 자체가 등록된 프로젝트의 원시 Run 로그 catchup 완료를 기다린다. `logs.open/search`도 Run의 프로젝트 및 명시 Run을 등록하지만 display/search filter·stream credit는 문제 수집에 적용하지 않는다. runtime 소유 typed helper로 **모든 live Run + 최근 완료 Run 128개**를 읽으며 history가 더 길면 truncated다. explicit 로그 lookup은 history 상한 밖의 Run도 추적한다. 이미 모은 과거 문제는 보관 상한 안에서 유지한다. observe가 runtime DB를 직접 조회하지 않는다.
- wrapper는 자식 실행 전 기존 3열 로그의 System 행에 공통 helper의 `실행 시도 시작: <attemptId>`를 기록한다. 현재 durable attempt와 일치하는 정확한 System marker 뒤 행만 수집한다. stdout/stderr의 가짜 marker, 이전 attempt의 같은 ms 행·검색 결과는 현재 문제를 만들지 않는다. 수동 재시작의 Starting+DB attempt 미확정 구간은 이전 durable ID를 쓰지 않는다. 읽기 뒤 identity를 다시 검사하고 바뀐 batch를 폐기한다. 미커밋 batch가 reader를 소유하므로 query/scan 취소도 다음 읽기에서 재생된다. marker 없는 이전 로그는 raw open/search에는 그대로 보이며 현재 attempt 문제로 귀속하지 않는다(v0.9 이전 없음).
- 독립 검토의 partial EOF 진단 조기 등록을 실제 RED로 재현해 Task9만 `next_complete()`로 LF까지 기다린다. partial System marker도 buffer에 보존하며 Task7 raw display의 EOF fragment 의미는 유지한다. 이전 wrapper가 행 중간에 끝난 경우 새 attempt 시작에서만 필요한 LF 하나를 보완한 뒤 marker를 쓴다(기존 bytes 보존). 완성 진단/marker·완료 캐시·raw 표시·실제 wrapper 새 경계 회귀 PASS, 독립 기능 재검토 PASS·미해결0이다. wire 불변으로 이전 TS 근거를 재사용한다.
- 회전은 잘라낸 위치에서 유효했던 마지막 System attempt marker **원본 행·timestamp**를 보존 tail 앞에 복원한다. tail 안의 더 새로운 marker는 그대로 전환되며 이전 attempt 행을 현재로 재분류하지 않는다. 복원 marker 길이도 회전 예산에 넣고 prefix는 기존 bounded line reader로 읽는다.
- 관찰은 TaskTracker·공유 inotify instance 1개(data 부모/runs 디렉터리 nonrecursive)·RuntimeChanged를 쓴다. 완료 Run은 reader를 닫고 file identity/size/mtime만 캐시하되, LF 전 pending fragment가 있으면 완료 캐시를 만들지 않아 다음 조회에서 재생한다. daemon당 등록 프로젝트 8개·explicit Run 32개·live reader 128개·source/cache 2,048개 상한 초과는 `observe.limit_exceeded`다. 원시 스캔은 직렬, 한 Run에서 68MiB를 넘게 읽으면 같은 오류다. 문제는 daemon당 고유 2,048개를 보관하고 이후 새 문제는 보관하지 않으며 해당 프로젝트 truncated=true를 유지한다. 이 표시는 daemon 세션 중 보관/이력 누락이 있었음을 뜻하며 재시작 clear로 공간이 생겨도 이미 놓친 행의 완전성을 주장하지 않는다. 공용 설정/UI/LSP는 추가하지 않았다.
- 직접 고유 **50개 PASS**: observe24, runtime logfile7, CLI problems6, server ACK/세대3, router1, wrapper3, 로그 RPC3+실제 소유 systemd Run1, 한국어 오류2. 초기 미구현 RED와 중복 2,100행 overflow·JSON escape 응답 크기·회전 marker 유실·partial 진단/marker·다음 wrapper marker 연결 실패 RED까지 7건을 보존했다. TS 생성/생성 확인/typecheck·Rust 포맷·diff 검사 PASS. supervisor 인자 누락·통합 컴파일 오류·취소 테스트 Debug 컴파일 오류·pnpm 옵션 위치 오류는 행동 RED/PASS와 별도로 기록한다. 원본/자원/소스 해시는 Git common-dir `devbox-verification/s2-task9/`에 둔다. 기존 서비스·실제 Docker/공유 네트워크를 바꾸지 않았으며 systemd 검사는 고유 소유 fixture만 정리했다. 최종 Biome278 PASS(cgroup peak232,513,536B·swap0). 전체 check/clippy/build/E2E·Windows는 Task16 전 미실행이다.

---

### Task 10: Docker 컨테이너

**단계별 실행:**
- [x] **1 · 실패 테스트:** `crates/runtime/src/containers_tests.rs`에 아래 출발 assertion과 이 과제의 핵심 테스트를 먼저 작성한다.
```rust
assert_eq!(container_args("abc", ContainerAction::Restart), ["restart", "abc"]);
assert!(!parse_unavailable_cli().available);
assert_eq!(parse_ps_fixture().items[0].compose_project.as_deref(), Some("owned"));
```
- [x] **2 · 실패 확인:** `cargo test -p devbox-runtime containers`를 자원 제한 아래 실행한다. 미구현 타입/행동 때문에 실패했는지 확인하고 RED 출력을 artifact에 남긴다(환경·잠금 실패는 RED가 아님).
- [x] **3 · 구현:** Docker CLI adapter는 argv 배열로만 호출하고 ps JSON·진단 reason을 typed 결과로 바꾼다. runtime.containers/container_action 등록·취소/종료 처리를 연결한다. Docker unavailable은 목록의 상태이며 전체 실행 화면 오류가 아니다.

```rust
let args = match action {
    ContainerAction::Start => ["start", id],
    ContainerAction::Stop => ["stop", id],
    ContainerAction::Restart => ["restart", id],
};
docker.command(&args).await?; // fixed argv, no shell; tests use a fake adapter.
```

- [x] **4 · 통과:** 같은 RED 명령과 직접 영향 시험으로 GREEN을 확인한다. 명령 조립·JSON 줄 fixture·daemon unavailable만 fake 실행기로 검증한다. 로컬 Docker 설치·시작·container/network 변경 없음. 묶음 P 진행 기록을 갱신한다.
- [x] **5 · 검토·커밋:** 별도 검토·Biome CI 뒤 `feat(runtime): list and control docker containers`. 실패·미실행 항목을 PROGRESS에 구분해 기록한다.

**Files:**
- Create: `crates/runtime/src/containers.rs`

**Interfaces:**
- `runtime.containers{}`(query): `docker ps -a --format json` → `{ id, name, image, state, status, ports, composeProject? }`. CLI가 없거나 데몬이 꺼져 있으면 `{ available: false, reason }`.
- `runtime.container_action{ id, action: start|stop|restart }`(mutation, `stop`은 reversible)
- 로그는 Task 7의 `Container` 소스로 본다.

**구현 계약·검증(로컬 완료):**
- 목록은 `ContainersResult { available, reason?: cliMissing|daemonUnavailable, items }`다. 각 행의 ports는 Docker 표시 문자열을 보존하고 `Labels`에서 `com.docker.compose.project`만 추출한다. 성공한 빈 응답은 available=true, CLI 부재·비정상 ps 종료는 available=false다. 비정상 ps의 내부 stderr는 tracing 진단으로만 남긴다.
- 목록 기한3초, start/stop/restart 기한30초(보통 Docker stop 유예10초 포함), stdout256KiB·stderr16KiB·목록1,024행 상한. fake 단위시험은 기한100ms 주입. timeout/상한/취소·future 폐기 때 소유 Docker CLI만 kill+wait한다. Docker 서버에서 이미 시작한 동작의 취소·원복을 보장하지 않으므로 시간 초과/취소 뒤 목록을 다시 조회한다. Docker에 더 긴 stop timeout이 설정된 경우도 클라이언트 기한은30초다.
- 메서드 정적 메타데이터는 mutation(reversible), 성공 결과는 `{ id, action, undo: ContainerActionParams|null }`. stop만 `{id,action:"start"}` Undo를 주며 start/restart는 null이다. Task13/14 UI는 목록 state=running일 때만 stop을 표시/사용하고 결과.undo가 있을 때만 Undo를 노출한다. Undo는 start 요청이며 외부 동시 변경을 포함한 상태 복원 트랜잭션이 아니다. 이번 과제는 UI나 동적 확인 프레임워크를 추가하지 않는다.
- 단위/fakeCLI/실제 typedRPC/라우터·한국어 오류 직접 검사, TS 생성·생성확인·typecheck를 수행한다. 원본 RED/GREEN·자원·소스해시는 Git common-dir `devbox-verification/s2-task10/`에 보관한다. 실제 Docker 실행·설치·daemon/container/network 조작은 하지 않았다. 고유13개·타입·독립 검토(미해결0)·최종 Biome278 PASS로 로컬 완료했다. 전체 검사·Windows·push/PR은 Task16 전 미실행이다.

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

**단계별 실행:**
- [x] **1 · 실패 테스트:** `crates/secrets/src/{crypto,keystore,store}_tests.rs · crates/cli/tests/runtime_secrets.rs · Windows unlock 단위 시험`에 아래 출발 assertion과 이 과제의 핵심 테스트를 먼저 작성한다.
```rust
let sealed = seal(&key, "TOKEN", &key_id, b"owned-token")?;
assert_eq!(open(&key,"TOKEN",&key_id,&sealed)?.as_slice(), b"owned-token");
assert!(open(&key,"OTHER",&key_id,&sealed).is_err());
assert_eq!(restart_secret_fixture().await.child_ack_count, 2);
```
- [x] **2 · 실패 확인:** `cargo test -p devbox-secrets seal_open_round_trip_and_wrong_aad_fails`를 자원 제한 아래 실행한다. 미구현 타입/행동 때문에 실패했는지 확인하고 RED 출력을 artifact에 남긴다(환경·잠금 실패는 RED가 아님).
- [x] **3 · 구현:** 아래 DPAPI 계약으로 crypto·store·SecretsService·unlock·복구 CLI를 구현한다. Task 3 redactor에 실제 참조값을 주고 별도 비밀 stream→anonymous pipe→exec stdin으로 매 attempt 주입한다. systemd argv/env/property에는 runId·참조 이름만 남긴다. 키 잠김/데몬 없음/누락이면 실제 사용자 자식 실행 0이며 systemd 정책에 따른 재시도만 허용한다.

```rust
// Each unit attempt invokes the owned helper, including systemd Restart attempts.
let values = secret_stream.resolve(&run_id).await?;
let (reader, writer) = anonymous_pipe()?;
let exec = spawn_exec_with_stdin(reader, run_id)?;
write_nul_pairs(writer, &values).await?;
// Locked/unavailable/missing returns before user-child spawn; zeroize values after use.
```

- [x] **4 · 통과:** 같은 RED 명령과 직접 영향 시험으로 GREEN을 확인한다. 핵심 시험 전체, 자동재시작 두 attempt에서 같은 비밀 재주입, daemon restart 후 재주입, daemon unavailable 자식0, split stdout chunk 비밀 가림을 확인한다. fallback은 interop 실패 주입으로 반드시 시험하고 실제 DPAPI/Windows 왕복은 Task 16에 남긴다.
- [x] **5 · 검토·커밋:** 별도 검토·Biome CI 뒤 `feat(secrets): seal per-machine secrets and inject them per run`. 실패·미실행 항목을 PROGRESS에 구분해 기록한다.

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

**DPAPI·재시작 계약(Task 1, R10 근거):**
- R10은 현재 호스트의 systemd 아래 절대 경로 interop·CurrentUser protect/unprotect 왕복만 PASS다(09 R10: 551.808/523.850ms). 로그인/재부팅 뒤 보장은 아니다. Task 11은 직접 해제 **그리고** Windows 앱 fallback을 항상 구현한다.
- direct는 `/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe -NoProfile -NonInteractive`에 고정 스크립트를 주고 blob/키는 stdin/stdout으로만 교환한다. argv·디스크 임시 파일·오류 본문에 평문을 넣지 않는다. 검증된 32바이트 키와 key_id만 메모리에 유지한다.
- 앱 연결 후 잠긴 상태/봉인 blob을 typed 내부 요청으로 읽고 Windows native DPAPI CurrentUser로 푼 뒤 기존 브리지의 앱 전용 `secrets.unlock`으로 전달한다. response는 Done/key status뿐이다. web UI/MCP에는 키/값을 반환하지 않는다. 실패는 잠김/키불일치로 유지하고 키를 조용히 교체하지 않는다. 최초 생성도 봉인 완료 뒤 blob+key_id를 확정한다. direct가 처음부터 unavailable인 경우에는 앱에서 무작위 키 생성→DPAPI 봉인→앱 전용 bootstrap(봉인 blob·key_id)→unlock 순서로 초기화한다. 기존 키가 있으면 bootstrap으로 덮지 않는다. blob과 키를 같은 공개 응답에 싣지 않는다.
- systemd `--pipe`의 첫 stdin은 자동 재시작에서 재사용되지 않는다. unit에는 runId·비밀 이름 참조만 남기고 **매 attempt** 소유 helper가 기존 instance Unix stream에서 비밀 payload를 받아 anonymous pipe로 `devbox exec --secrets-stdin`에 넣는다. 일반 RPC 결과·UI·MCP 경로로 평문을 반환하지 않는다. 별도 일반 권한 체계나 wrapper의 키 사본은 만들지 않는다.
- 데몬 unavailable/locked 또는 값 누락이면 새 사용자 자식은 실행하지 않는다. 이미 실행 중인 자식은 유지하며 다음 시도는 systemd 재시작 정책만 따른다. daemon 재시작 뒤에는 다시 해제해 매 attempt 주입한다. runtime이 systemd 외 재시작 루프를 만들지 않는다.
- 로그는 stdout/stderr 조각 경계의 비밀도 가리고, 복구 문자열은 사용자가 명시한 export 결과만 예외다. 자동 로그·진단에는 남기지 않는다. 값 가림은 완전한 데이터 유출 방지 sandbox가 아니라 승인된 비밀 문자열 노출 방지다(ADR0016).

**핵심 테스트:**
- `seal_open_round_trip_and_wrong_aad_fails`
- `key_mismatch_is_reported_not_silently_reset`
- `run_with_secret_hides_value_from_argv_unit_properties_logs_and_rpc`: `systemctl --user show <unit> -p Environment`·`/proc/<pid>/cmdline`·로그 파일·`runtime.runs` 결과에서 값 문자열을 검색해 없음을 확인
- `missing_secret_blocks_the_run_with_names`
- `recovery_string_restores_the_same_key_id`
- `windows_app_unlocks_when_daemon_interop_is_unavailable`: 데몬 Windows 호출 실패를 주입 → 잠금 상태 유지 → Windows 앱 DPAPI 해제·`secrets.unlock` → 동일 key_id로 비밀을 쓰는 실행 성공, 로그·RPC 응답에는 키·비밀이 없음. fallback은 필수 핵심 회귀 테스트다. R10 성공은 한 로그인 세션에 한정되므로 이를 생략하는 근거가 아니다.
- DPAPI blob 입출력 조립을 단위 검증하고, 직접 해제와 Windows 앱 fallback의 실제 왕복은 Task 16 에이전트 설치본 스모크에서 모두 확인한다.

**Task 11 구현 계약·직접 검증(2026-10-20, 독립 최종 검토·Biome278 PASS, 로컬 완료):**
- `devbox-secrets`가 XChaCha20-Poly1305 nonce24·AAD `name|key_id`, SHA256 전체32바이트 지문, PC별 `secrets_key/secrets_values` 테이블과 `[secrets] names`를 소유한다. 목록은 이름과 `hasValue`만 반환한다. `set/delete/import_key`와 명시적 `export_key`를 연결했고 export는 CLI 채널0만 허용한다. 복구 CLI 입력은 stdin, 출력은 사용자가 실행한 export만 예외다. 키·값·복구/입력 버퍼는 zeroize하며 mutation 멱등 저장소는 평문 params 대신 SHA256 지문을 보관한다.
- 직접 DPAPI는 고정 절대 PowerShell·고정 script·binary stdin/stdout만 사용하고 입출력10초·종료대기2초/64KiB 상한·취소/기한/상한 이후 소유 자식 kill+wait를 적용한다. `keys/data.key.dpapi`에는 봉인 blob만 저장하고 기존 키를 bootstrap으로 덮지 않는다. direct unavailable은 locked, 실제 DPAPI/키 지문 불일치는 `keyMismatch`와 typed 오류로 구분한다. 정상 unlock/import 뒤 mismatch를 해제한다.
- Windows native 연결은 기존 예약 채널0에서 Hello→Welcome→`secrets.key_status`를 수행한다. 기존 blob은 CurrentUser DPAPI로 해제하고, 최초 상태는 생성→봉인→`secrets.bootstrap`→`secrets.unlock` 순서다. 서버는 App+채널0을 요구하고 renderer는 Mux가 nonzero 채널로 다시 쓰며 native 응답은 화면에 전달하지 않는다. 키 상태는 blob·지문·locked/keyMismatch뿐, bootstrap/unlock 응답은 Done뿐이다.
- `RunDef.secrets` 및 env의 `{{secret:NAME}}`만 지원한다. command template은 거절하고 비밀을 주입할 env 항목은 systemd Environment와 `/usr/bin/env` argv 양쪽에서 제외한다. Run 내부 snapshot에는 참조 정의만 저장한다. 매 systemd attempt의 `exec-attempt`가 기존 instance Unix socket의 내부 `secrets.run` stream(Cli+채널0)에서 길이 표기·1MiB 상한 payload를 받아 익명 pipe로 `exec --secrets-stdin`에 넘긴다. 원래 참조값의 별도 가림 record는 자식 환경에 넣지 않으며 조합된 env 값과 원래 비밀 모두를 청크 경계 전에 가린다.
- 고유 **50개 PASS**: Rust48(crypto/store/keystore9·launch5·graph4·wrapper3·dedupe2·routes1·protocol router7·config1·native bridge5·native secrets7·실제 소유 systemd3·RPC/CLI1)+한국어 메시지2. TS 생성/확인·typecheck, `devbox-win` MSVC target 좁은 check도 PASS다. 앱의 실제 `NativeSecrets` production 상태 기계를 fake DPAPI와 live fixture 데몬 사이에 연결해 초기 bootstrap 및 daemon restart 후 같은 key_id unlock→비밀 Run 재주입을 확인했다. 자동재시작 ACK2, 값 누락/locked/unavailable 사용자 자식0, 기존 실행 유지, unit properties·helper/exec/자식 cmdline·로그·runtime.runs 비노출을 확인했다.
- 독립 검토에서 실행 중 `secrets` 섹션이 scalar/array로 바뀌면 panic이 나는 P2를 재현했다. checked TableLike 접근으로 typed 오류를 반환하고 원본 파일·값 미저장을 보존한다. 잘못된 names 형식과 유효한 inline table 보존까지 secrets9개 GREEN, 별도 재검토 PASS·미해결0이다. 최초48개에 신규2개를 더한 고유50개이며 무관한 검사는 반복하지 않았다. 최종 Biome278 PASS(`biome-final.log`, 0.923초·cgroup130,469,888B·swap0).
- 초기 crypto/native 미구현·unit helper 누락 RED3건, DPAPI stdin EOF가 닫히지 않아 fake 왕복이 10초 timeout 된 실제 실패1건을 보존했다. ChildStdin drop 후 왕복·복구 재봉인·취소/기한/출력 상한 소유 PID 회수 회귀가 PASS다. RawValue 변환 컴파일 오류1건은 행동 RED와 구분한다. 근거는 Git common-dir `devbox-verification/s2-task11/{summary,source-freeze,matrix-results,cleanup}.json`·raw log/resources다. 모든 검사는 common-dir 잠금·CPU4/nice10/jobs2/threads2/workers2·cgroup6/8GiB/swap1GiB로 실행했으며 소유 slice8개의 unit/drop-in 잔존0을 확인했다. 실제 Windows DPAPI/설치본 왕복·전체 check/clippy/build/E2E·push/PR은 Task16 전 미실행이다.

---

### Task 12: Windows 대상 실행

**단계별 실행:**
- [x] **1 · 실패 테스트:** `crates/runtime/src/windows_tests.rs · app/src-tauri Windows 실행 adapter 시험`에 아래 출발 assertion과 이 과제의 핵심 테스트를 먼저 작성한다.
```rust
let report = sync(&source, &target).await?;
assert_eq!(read(target.join("tracked.txt")), "changed");
assert!(target.join("node_modules/keep").exists());
assert_eq!(quote_powershell("a'b"), "'a''b'");
```
- [x] **2 · 실패 확인:** `cargo test -p devbox-runtime windows`를 자원 제한 아래 실행한다. 미구현 타입/행동 때문에 실패했는지 확인하고 RED 출력을 artifact에 남긴다(환경·잠금 실패는 RED가 아님).
- [x] **3 · 구현:** `core::config`의 config.local 전용 windows.root를 추가하고 git -z 목록의 변경 파일만 복사한다(deleted=0). `authorize_run_graph`의 prepared graph·입력·작업 폴더를 확인한 뒤에만 sync한다. 프로젝트 정의/의존이 포함될 때만 승인 snapshot을 요구하고, thisPc-only Windows 실행은 저장소 승인이 낡아도 허용한다. direct interop와 Windows app `run_windows` adapter는 같은 실행 ID·출력·종료 결과를 쓰며 shell 문자열의 cwd/argv quoting을 검증한다. 앱 위임 중 연결이 끊기면 상태를 실패/확인 필요로 남기며 조용히 중복 실행하지 않는다.

```rust
let graph = runtime.authorize_run_graph(&p).await?; // ThisPc-only needs no project snapshot.
let prepared = resolve_windows_target(graph, &config.windows)?;
let sync_report = sync(&prepared.source, &prepared.target).await?;
match interop.run(&prepared).await {
    Err(InteropUnavailable) => windows_bridge.run_windows(prepared).await,
    result => result,
} // Delegate only if no process was started; preserve runId and stdout/stderr origin.
```

- [x] **4 · 통과:** 같은 RED 명령과 직접 영향 시험으로 GREEN을 확인한다. 파일 변경·추가 대상 보존·공백/작은따옴표 경로·trust 미승인 sync0·interop 실패 fallback 계약을 fixture로 확인한다. Windows 자식 종료 보장은 추가하지 않고 UI 문구에 표시한다. 묶음 Q 기록을 갱신한다.
- [x] **5 · 검토·커밋:** 독립 최종 검토(미해결0)·Biome CI를 통과하고 `feat(runtime): run trusted tasks on Windows targets`로 로컬 완료한다. 실패·미실행 항목은 PROGRESS와 검증 artifact에 구분했다.

**Files:**
- Create: `crates/runtime/src/windows.rs`
- Modify: `crates/core/src/config.rs`(`config.local.toml`의 `[windows] root = 'C:\dev'`)

**Interfaces:**
- 대상 폴더 `windows_dir = <root>\<프로젝트 이름>`. 원장은 local-only `[windows] root` 하나이며 미사용 `pc.windows_root`는 엄격 파싱 오류로 제거한다(자동 설정 변경 없음).
- `sync(project_dir, windows_dir) -> SyncReport { copied, deleted, skipped }`
  - 대상 목록: `git ls-files -co --exclude-standard -z`
  - `/mnt/<drive>/...`로 바꾼 경로에 크기·mtime이 다른 파일만 복사하고 mtime을 보존한다. 목록에 없는 대상 쪽 파일은 지우지 않는다(`deleted=0`, `node_modules` 같은 Windows 쪽 산출물 보호). source/target의 링크 경로와 Windows 금지 파일명·대소문자 충돌은 복사 전 거절한다.
- 실행:
  - 기존 exec-attempt/exec 래퍼 안에서 고정 PowerShell bootstrap을 실행한다. cwd·command·env는 JSON stdin으로만 보내며 `Set-Location -LiteralPath`와 `ScriptBlock.Create`로 공백/작은따옴표를 보존한다. 비밀 값은 argv·스크립트 literal·공개 RPC에 넣지 않는다.
  - interop process spawn이 시작 전에 실패한 경우만 Windows 앱에 한 번 위임한다. App0의 `windows.next/output`, Cli0의 `windows.prepare/run/ready`는 공개 선언/renderer에서 제외한다. native 최대 4개, 출력 큐 16칸 및 ACK 기반 backpressure를 사용한다. 현재 run/attempt와 연결 세대를 대조하며 run별 native-attempt 파일이 재연결 재위임을 막는다.
  - 조용한 native 실행도 1초 내부 Heartbeat를 보낸다. CLI stream 종료의 ACK 오류는 해당 run/attempt token만 취소하고 직접 PowerShell kill/wait를 시도한다. Heartbeat는 로그·Ready에 반영하지 않는다.
  - Windows 서비스의 ports_hint는 Windows PowerShell TcpClient 또는 native Windows TcpStream에서 확인한다. WSL localhost/mirrored mode를 가정하지 않는다. 준비 알림은 먼저 reconcile하고 현재 attempt를 다시 확인한다.
  - 알려진 종료 코드는 기존 durable ExitRecord에 기록하고, 불명확한 native 완료는 worker→exec→exec-attempt의 SIGUSR2로 구분해 `RestartPreventExitStatus=SIGUSR2`로 자동 중복 시작을 막는다. 실제 exit78은 원래 코드와 재시작 정책을 유지한다.
- 화면 표시: "Windows에서 실행 · 자식 프로세스 종료는 보장하지 않음". 신뢰 미리보기에도 대상(Windows)과 실행 폴더를 보인다.

**핵심 테스트:**
- `sync_copies_changed_tracked_files_and_keeps_extra_target_files`(임시 폴더 두 개로)
- `windows_command_is_quoted_for_powershell`(작은따옴표 이스케이프)
- `this_pc_only_windows_run_ignores_stale_project_trust`·`windows_run_with_project_dependency_requires_current_snapshot`: thisPc-only+낡은 저장소 승인에서는 sync/실행 허용, ProjectFile 의존이 있으면 승인 전 sync/자원0
- 실제 실행은 Task 16 에이전트 설치본 검증

**직접 검증 기록(2026-10-20):** 고유93개(Rust90·프런트3), 실제 소유 systemd7개·native16·TS 생성/확인/typecheck·MSVC 좁은 check·Biome278 PASS. quiet stop과 대상 링크 RED를 포함한 원본/실패 분류/자원/최종38개 소스 해시는 `devbox-verification/s2-task12/`에 보존한다. 독립 최종 검토 PASS·미해결0, 소스41개·검증 artifact94개 해시 일치. 묶음 Q 로컬 완료이며 최종 상태와 커밋은 `root-completion.json`에 기록한다. 실제 Windows 앱/PowerShell/DPAPI·전체 검사·push/PR은 Task16 미실행이다.

```bash
# 묶음 Q 끝
# 묶음 Q 끝: 과제별 좁은 검증·별도 검토 뒤 PROGRESS.md의 묶음 Q 행과 현재 위치를 갱신해 로컬 커밋한다(00-roadmap §3).
```

---

### Task 13: 실행 섹션 화면

**단계별 실행:**
- [x] **1 · 실패 테스트:** `app/src/features/runs/{DefineDialog,LogView,model,RunList}.test.tsx · app/e2e/runs.spec.ts`에 아래 출발 assertion과 이 과제의 핵심 테스트를 먼저 작성한다.
```tsx
await saveDefinition(); // define succeeds, following list fails
expect(screen.getByRole("heading", { name: "작업 수정" })).toBeVisible();
await saveDefinition();
expect(createdIds()).toEqual(["task-1"]);
```
- [x] **2 · 실패 확인:** `pnpm --filter app exec vitest run src/features/runs/DefineDialog.test.tsx --maxWorkers=2`를 자원 제한 아래 실행한다. 미구현 타입/행동 때문에 실패했는지 확인하고 RED 출력을 artifact에 남긴다(환경·잠금 실패는 RED가 아님).
- [x] **3 · 구현:** RunsPage/목록·DefineDialog·ANSI model·가상 LogView를 작은 컴포넌트로 만들고 sections/router에 등록한다. define 결과로 ID/수정 모드 확정, 동일 요청 재전송과 새 수정 의도를 구분한다(기존 useRpcMutation의 성공 후 새 requestId 정책을 임의로 바꾸지 않음). 요청 응답을 잃은 retry만 같은 clientRequestId를 유지하고 저장 성공 뒤 수정은 같은 definition ID를 유지한다. runtime.changed rev로 상세 PID/runId를 갱신한다. trust UI는 Run preview와 제어문자 변환을 재사용해 승인 뒤 같은 동작 한 번만 재시도한다.

```tsx
const saved = await define.mutateAsync(draftWithExistingId);
setEditingId(saved.definition.def.id);
queryClient.setQueryData(["runtime.list", { projectId, cursor: null }], old => upsertDefinition(old, saved));
void queryClient.invalidateQueries({ queryKey: ["runtime.list", { projectId, cursor: null }] });
```

- [x] **4 · 통과:** 같은 RED 명령과 직접 영향 시험으로 GREEN을 확인한다. ANSI 16/256/truecolor·제어열 제거·follow 상태기계·목록 실패/멱등·restart 상세·stale trust 단위 시험을 실행한다. 대표 E2E 2개는 작성만 하고 Task 16에서 전체 실행에 포함한다.
- [x] **5 · 검토·커밋:** 별도 검토·Biome CI 뒤 `feat(runs): add execution management and live log views`. 실패·미실행 항목을 PROGRESS에 구분해 기록한다.

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

- 생성과 조회 실패의 분리(01-design §8.6 규칙 2, AR-F04): `DefineDialog`는 `runtime.define` 응답으로 바로 "수정" 모드(생성된 ID)로 바뀐다. 이어지는 `runtime.list` 조회가 실패하면 목록 위 오류 배너 + [다시 불러오기]만 보이고 대화상자는 수정 모드를 유지한다. 응답이 불명확한 재시도는 같은 `clientRequestId`를 쓴다. 성공 뒤 다시 저장은 생성된 definition ID의 수정이며 새 mutation ID를 써도 새 정의를 만들지 않는다.
- 열린 실행 상세(PID·실행 ID·재시작 수)는 `runtime.changed`의 rev로 갱신한다. 처음 읽은 값을 캐시로 붙들지 않는다(AR-F15).

**핵심 테스트:**
- `ansiToSpans` 단위
- `LogView`의 따라가기·멈춤 상태 기계 단위(`follow`·`userScrolled`·`newLines`)
- `define_success_then_list_failure_keeps_edit_mode_and_does_not_create_twice`: 가짜 클라이언트에서 `runtime.define` 성공 + `runtime.list` 실패 → 대화상자 제목 "작업 수정", [저장] 다시 누름 → 두 번째 `runtime.define`은 같은 definition ID의 수정, 새 생성은 1회(응답 불명확 재시도만 같은 `clientRequestId`)
- `open_run_detail_follows_restart`: 상세를 연 채 `runtime.changed`(rev 증가) + 새 PID → 상세의 PID가 새 값
- `start_with_stale_trust_shows_preview_then_retries_once`(가짜 클라이언트)
- E2E `runs.spec.ts`(2개)
  - `echo` 작업을 정의 → 시작 → 로그에 줄 → 종료 코드 표시
  - 서비스(`python3 -m http.server $DEVBOX_PORT`) 시작 → 포트 칩 → 중지 → 되돌리기 토스트

**실제 통합 계약·직접 검증(2026-10-20):**
- `runtime.list`의 256행/768KiB 페이지마다 definition별 live 우선·최근 결과를 같은 DB read rev로 보낸다. `readRuntimeList`는 같은 rev의 모든 페이지를 모으고 drift 재시도는 3회로 제한한다. 선택 상세는 별도 typed `runtime.get{runId} → {rev,run}`로 읽어 최근32/256회 밖의 실행·PID·재시작 수를 유지한다.
- `DefineResult`는 부분 응답이므로 기존 `useRpcMutation`은 그대로 두고, 프로젝트 파일 우선 upsert·낮은 rev 응답 거절을 적용했다. `runtime.list`에만 같은 rev의 정식 snapshot 교체를 허용하여 저장 직후 trust/runs가 낡게 남지 않는다. 다른 query의 revision 정책은 유지한다.
- 서비스 중지는 reversible이며 전역 `RunStopUndo`가 10초 토스트·섹션/프로젝트 전환 뒤 되돌리기를 처리한다. 같은 Run ID를 현재 신뢰로 재시작하고 승인 뒤 한 번만 재시도한다. 작업 중지는 확인하며, 컨테이너 되돌리기는 실제 stop 응답의 undo만 사용한다.
- `StoredRun.source_cwd`는 실제 실행의 정규 cwd를 보존한다. `observe.resolve_location`은 source run/파일/프로젝트를 기준으로 위치를 해석하고 Windows 사본 경로를 원본으로 매핑한다. 이전 cwd 미기록 실행의 상대 위치는 추측하지 않고 거절한다. native 기존 검증된 distro·Code.exe/Cursor.exe+cli.js argv에 `--goto`를 더했으며 http/https 전용 URL helper는 그대로다. Task14의 project-root-relative Problem.file은 이 run-cwd 계약과 구분해야 한다.
- ANSI 16/256/truecolor·굵게/밑줄·제어열 제거, ANSI 경계/한글/따옴표 공백 파일 링크, 가상화·follow·서버 검색/필터·8개 소스 병합·credit/dispose·10,000줄/4MiB 보관을 구현했다. Windows 폴더/종료 한계는 표시하고 WSL 래퍼 CPU/메모리를 Windows 자원으로 표시하지 않는다.
- 고유64개 PASS(프런트47·Rust17), TS 생성/check·앱/E2E 정적 타입·MSVC 좁은 check·파일/컴포넌트 상한 PASS. Define 실제 assertion2·오래된 live 누락1·ANSI 링크5·저장 우선순위/rev3 RED를 보존했다. fixture geometry/Provider 종료 기대·작성 중 타입/컴파일 오류는 별도 실패 분류다.
- 대표 E2E2개는 **작성만** 했다. runtime-only fixture는 고유 소유 slice/anchor·고정 env·CPU4/nice10·6/8GiB/swap1GiB·정확한 drop-in 정리를 사용하고 모든 browser fixture PATH에 소유 fake Docker를 둔다. helper mock 검증은 E2E 실행 성공이 아니다. 기존 systemd 로그 시험1 ignored·실제 Windows/편집기 실행·전체 검사·push/PR은 미실행이며 Task16에서 수행한다.
- 근거 `devbox-verification/s2-task13/{summary,source-freeze,test-names,static-checks,artifact-hashes}.json` 및 raw/resources. 제품·시험62개 digest `a0a55722fbe0c82901e23d6efc4a23b1851f2da6240540b79dd30df4e177cedf`; 독립 검토 PASS·미해결0. 완료 Biome 최초 줄바꿈1건을 포맷만 수정한 뒤 316개 PASS(0.582초·cgroup114,196,480B·swap0). 최종 상태·해시·커밋은 `root-completion.json`에 기록한다.

---

### Task 14: 하단 패널의 로그·문제 탭, 상태 표시줄, 알림

**단계별 실행:**
- [x] **1 · 실패 테스트:** `app/src/features/problems/ProblemsPanel.test.tsx · notifications/run notification 시험`에 아래 출발 assertion과 이 과제의 핵심 테스트를 먼저 작성한다.
```tsx
await emitRunFailure("run-1");
await emitRunFailure("run-1");
expect(notificationsFor("run-1")).toHaveLength(1);
expect(screen.getByRole("link", { name: /src\/x.rs:10/ })).toBeVisible();
```
- [x] **2 · 실패 확인:** `pnpm --filter app exec vitest run src/features/problems/ProblemsPanel.test.tsx --maxWorkers=2`를 자원 제한 아래 실행한다. 미구현 타입/행동 때문에 실패했는지 확인하고 RED 출력을 artifact에 남긴다(환경·잠금 실패는 RED가 아님).
- [x] **3 · 구현:** registerBottomTab으로 로그/문제를 연결하고 StatusBar는 visible 상태에서만 observe를 조회한다. Task 4 notify.raised 타입에 schedule 실패와 run target을 연결, AttentionBridge/알림센터/기존 notify_attention Windows 경로에 run tag를 전달한다. 문제 링크는 S2 외부 편집기이며 S4 내부 편집기를 미리 구현하지 않는다.

```tsx
useEffect(() => registerBottomTab({ id: "logs", label: "로그", render: () => <BottomLogs /> }), []);
const stats = useRpcQuery("observe.system", {}, { enabled: visible, refetchInterval: visible ? 3000 : false });
// AttentionBridge consumes notify.raised once per run/tag and uses existing native notify path.
```

- [x] **4 · 통과:** 같은 RED 명령과 직접 영향 시험으로 GREEN을 확인한다. 문제 이동·프로젝트별 고정 source·숨김 polling0·알림 중복제거 단위 시험, Task4 start-limit 알림1 L2 직접 영향 부분만 검사한다. Windows 토스트 실기는 Task16이다.
- [x] **5 · 검토·커밋:** 별도 검토·Biome CI 뒤 `feat(runs): connect logs problems and run notifications`. 실패·미실행 항목을 PROGRESS에 구분해 기록한다.

**Files:**
- Create: `app/src/features/runs/BottomLogs.tsx`, `app/src/features/problems/ProblemsPanel.tsx`
- Create: `crates/core/src/notify.rs`(데몬 알림 주제 `notify.raised`; Task 4에서 작은 발행 계약부터 만들고 여기서 화면 수신 연결)
- Modify: `app/src/shell/StatusBar.tsx`, `app/src/features/notifications/AttentionBridge.tsx`

**Interfaces:**
- 하단 패널 `로그` 탭은 고정한 소스를 보이고, 고정이 없으면 현재 프로젝트의 최근 실행을 보인다. `문제` 탭은 `observe.problems`(파일·줄·메시지, 누르면 열기)를 보인다.
- 상태 표시줄: "실행 n · 포트 5173, 8080 · WSL 6.1/20GB"(누르면 해당 화면)
- 기존 데몬 알림 주제 `notify.raised { id, projectId, runId: string|null, task: string|null, reason }`를 유지한다. `reason`은 `startLimit`·`schedule_failed`·`schedule_trust_required`다. 제목/본문/화면 target은 UI에서 구성한다.
  - 예약 실패와 서비스 재시작 한도 초과에 쓴다.
  - 화면은 알림 센터에 넣고, Windows 토스트는 S1 `notify_attention`과 같은 경로로 띄운다(tag `run-<eventId>`, 긴 ID는 별도 run namespace hash). event ID로 센터/토스트 재전송을 함께 억제하고 수동 재시작으로 run ID를 재사용한 새 실패는 표시한다.

**핵심 테스트:**
- `ProblemsPanel` 렌더·이동 단위
- 데몬 `notify.raised`가 서비스 한도 초과에 한 번만 나오는지 L2(Task 4 시험에 붙임)

---

**구현·검증 기록 (2026-10-21, 로컬 완료):**
- 기존 터미널을 유지한 로그/문제 탭, 프로젝트별 최대 8개 고정 source·최근 실행 fallback·기존 LogView 재사용·스트림 정리·조회 오류 재시도를 연결했다. 문제 목록은 100개 페이지를 같은 rev로 최대 2,048개 모으고 invalid cursor에는 최대 3회 새 snapshot을 읽는다. 프로젝트 변경/취소 시 이전 응답을 섞지 않는다.
- `RunLogMetadata`가 원본 `source_cwd`·Windows 사본 위치를 Problem ingest에 전달한다. Problem.file은 원본 cwd 안에서는 cwd 상대, 그 밖의 허용된 프로젝트 파일은 절대 경로다. 원본 cwd 없는 이전 기록의 상대 진단은 추측하지 않는다. 하위 cwd의 절대/상대 파일·소유 작업 폴더·Windows 진단을 실제 problems→resolve_location RPC로 열고 기존 프로젝트/심볼릭링크 경계를 검증했다.
- `runtime.summary {}` → `{ rev, active }`는 같은 DB snapshot에서 모든 live 상태를 센다. 대표 정의나 최근 256개 이력의 개수로 대체하지 않는다. 상태 표시줄은 runtime.changed로 갱신하며 system/ports만 가시 상태에서 3초 조회한다. 클릭은 `/runs` 목록 또는 `/runs?view=ports|system` 조회 화면으로 이동한다.
- 알림 센터·네이티브 pending/ack 경로는 정확한 project/run을 선택하고 run 없는 예약 승인/사전 실패는 task로 이동한다. Unicode/slash 정의 ID를 percent encoding하고 agent URI/tag·cold/warm 최신 intent를 보존했다. 센터 삭제·동일 알림 반복 클릭·프로젝트 수동 전환·run ID 재사용도 직접 검증했다.
- 직접 고유 **111개 PASS**(프론트62·Rust49)와 타입/생성/MSVC 좁은 check·변경 TS27개 Biome·Rust 포맷 PASS. 실패 분류/정확한 명령은 Git common-dir `devbox-verification/s2-task14/{summary,test-names,parent-commands,bottom-commands,notify-commands,failure-classification}.json` 및 raw/resources에 기록한다. Task4 실제 소유 systemd start-limit 알림 1회 L2 PASS. 전체 check/clippy/build/E2E·실제 Windows 토스트/앱은 Task16 미실행이며 push/PR도 하지 않았다. 독립 검토 PASS·미해결0, 소스47개·artifact99개 해시 일치. 완료 Biome331 PASS(0.619초·cgroup115,904,512B·swap0). 검토된 원본 summary/manifest는 인계 시점으로 보존하고 최종 상태·문서 해시·커밋은 `root-completion.json`에 기록한다.

### Task 15: 비밀 설정·에이전트 테스트 실행 전환·MCP 도구

**선행 분리 상태 (2026-10-21):** 첫 이동은 전후 고유34개·독립 검토(미해결0)·Biome331 PASS 뒤 로컬 완료했다. `service.rs` 598→282줄, 생성/포트 예약은 `create.rs` 228줄, `changed/apply_event`는 `events.rs` 124줄로 분리했다. 포트 단위 시험 1개도 구현과 함께 이동했으며 생성·이벤트·상태/상관관계·CLI 직접 영향 고유34개를 이동 전후 각각 통과했다(실행68개). 공개 계약·함수 본문·잠금/신뢰/소유 자원 의미를 보존한 이동이므로 인위적 RED를 만들지 않았다. 근거는 `devbox-verification/s2-task15-refactor-create/`다. 첫 이동의 로컬 커밋은 `557139f2`이며, 선행2 상태는 아래와 같다. 검토된 인계 summary/manifest는 보존하며 최종 상태·문서 해시·커밋은 `root-completion.json`에 기록한다. 기능 Task15·전체 검사·Windows·push/PR은 미실행이다.

**선행2 상태 (2026-10-21):** 기존 AgentsService::test 48줄을 `actions/test.rs`로 그대로 옮겼다. actions571→525줄·test57줄로 상한과 이후 test 연결 공간을 확보했으므로 계획의 선택 사항인 review/rebase 추가 이동은 하지 않았다. terminalId 결과·open/소유 worktree·Test snapshot·action lock 수명은 유지했다. CLI actions 안전8·기존 agents.test 실행1·신뢰 승인/거절2의 고유11개가 이동 전후 각각 PASS(실행22개), Rustfmt/diff/600줄 PASS다. 독립 검토 PASS·미해결0·완료 Biome331 PASS 뒤 로컬 완료했다. 근거 `devbox-verification/s2-task15-refactor-actions/`. 인계 시점 summary/manifest는 보존하고 최종 상태·문서 해시·커밋은 `root-completion.json`에 기록한다. 다음은 아래 기능 RED·runtime 전환이다.

**단계별 실행:**
- [x] **1 · 실패 테스트:** `crates/cli/tests/{runtime_agent_test,mcp_runs}.rs · SecretsSettings.test.tsx`에 아래 출발 assertion과 이 과제의 핵심 테스트를 먼저 작성한다.
```rust
let run = agent_test(&agent_in_worktree).await?;
assert_eq!(run.cwd, agent_in_worktree.workdir);
assert!(mcp_logs(&run.id).await.contains("[REDACTED]"));
assert_eq!(mcp_start_stale().await.created_units, 0);
```
- [x] **2 · 실패 확인:** `cargo test -p devbox-cli --test runtime_agent_test agents_test_runs_in_the_agent_worktree`를 자원 제한 아래 실행한다. 미구현 타입/행동 때문에 실패했는지 확인하고 RED 출력을 artifact에 남긴다(환경·잠금 실패는 RED가 아님).
- [x] **3 · 구현:** 위 두 agents 분할을 먼저 별도 커밋한다. `AgentsService::test_with_runner(id, runner)`(새 연결 API)은 action lock을 잡고 열림/소유 worktree/TrustOperation::Test를 확인한 `AgentTestSpec { project_id, cwd, command, snapshot }`을 runner에 넘긴다. runner future의 시작 결과까지 lock을 유지해 discard/rebase와 경합하지 않게 한다. CLI daemon/agent_test가 runner closure로 RuntimeService::start_agent_test를 호출해 `{ runId }` 응답을 만든다. agent crate가 runtime에 의존하지 않는다. MCP read 도구·write gate·secrets settings를 연결하고 RunTab은 마지막 200줄의 가려진 로그만 보낸다.

```rust
pub async fn test_agent(s: Arc<Services>, id: &str) -> Result<AgentTest, Fail<AgentsError>> {
    s.agents.test_with_runner(id, |prepared| async {
        let run = s.runs.start_agent_test(prepared).await.map_err(agent_run_failure)?;
        Ok(AgentTest { run_id: run.id })
    }).await
} // test_with_runner holds the action lock through the awaited start result.
```

- [x] **4 · 통과:** 같은 RED 명령과 직접 영향 시험으로 GREEN을 확인한다. 핵심 시험과 closed/unsafe worktree 거절·stale test snapshot·MCP write=false·승인 도구 부재·값입력 후 비움/목록 비노출·실패로그 200줄 상한을 검사한다.
- [x] **5 · 검토·커밋:** 별도 검토·Biome CI 뒤 `feat(runs): add secrets settings agent test runs and MCP tools`. 실패·미실행 항목을 PROGRESS에 구분해 기록한다.

**Files:**
- Create: `app/src/features/settings/SecretsSettings.tsx`
- Modify: `crates/agents/src/{actions,service,model}.rs`, `crates/cli/src/daemon/{routes,state}.rs`·`mcp.rs`, `app/src/features/agents/detail/RunTab.tsx`
- Create: `crates/agents/src/actions/test.rs`·`create.rs`·`events.rs`(분할 계획), `crates/cli/src/daemon/agent_test.rs`(도메인 간 실행 연결)

**Interfaces:**
- 설정 › 비밀:
  - 이름 목록과 "이 PC에 값 있음/없음"을 보인다. [값 입력] 입력란은 `type=password`이고 보내면 비운다. [삭제]
  - 키 상태(열림·잠김·불일치)와 [복구 문자열 보기·입력]이 있다.
  - `secrets.status{}`는 `{locked,keyMismatch}`만 공개한다. native `secrets.key_status`의 keyId/blob는 renderer에 노출하지 않는다.
  - `secrets.reset{}`는 `mutation(always)`이며 전체 로컬 값을 지운다는 확인 뒤 새 키를 만든다. 공유 이름은 보존한다. DPAPI 봉인/key-id별 파일 준비 뒤 DB의 값 삭제·keyId 변경을 한 transaction으로 커밋한다. DPAPI/파일/DB 실패 시 기존 값/키/blob를 보존하고 새 소유 파일만 정리한다. interop 불가 시 초기화하지 않고 오류를 표시한다(기존 Windows bootstrap/unlock fallback 유지).
  - set/import/delete/reset은 응답 손실 후 동일 입력에 같은 requestId를 재사용한다. 평문을 query/mutation cache에 저장하지 않고 SHA256 fingerprint+요청 메타데이터만 임시 보관한다. 복구 문자열은 명시 조회에만 표시하고 닫거나 화면이 바뀌면 지운다.
  - 불일치면 상단 배너 "비밀 키가 다릅니다 · [복구 문자열 입력][비밀 다시 입력]"(01-design §6.5)이다.
- `agents.test`:
  - S1에서는 터미널 세션이었다. 이제 runtime 작업 `[agent] test`를 그 작업 폴더에서 실행하고 `runId`를 돌려준다.
  - 실행 탭은 `LogView`와 `runtime.get`/`runtime.changed`로 ID·결과를 갱신한다. 실패 때 명시적 [에이전트에게 실패 로그 보내기]는 해당 run의 서버 가림 로그만 최근 200줄까지 받아 시간순으로 인용한다. 헤더·인용을 포함해 UTF-8 64KiB를 넘으면 명시적으로 생략하며 닫힌 에이전트/중복 클릭/취소를 차단한다. 조회는 최대 256페이지·전체 5초이며 LogView 검색·브라우저 버퍼·raw 파일을 쓰지 않는다.
  - `RunKind::AgentTest`를 일반 정의 대표/`runtime.restart`에서 제외한다. 같은 이름의 일반 task와 섞지 않으며 재실행은 새 `agents.test`의 소유권·Test 신뢰 확인을 거친다. 내부 snapshot의 digest·정규 경로가 시작 전 달라졌으면 자원 생성 없이 거절한다. 실제 cwd를 보존해 작업 폴더의 로그/문제 위치를 연다.
- MCP 도구 추가:
  - `devbox_runs{projectId?}`(읽기, 최근 최대 100개)
  - `devbox_logs{ runId, lines ≤ 500 }`(읽기, 기본 200줄·비밀 값 가림·합계 768KiB·최대 64페이지·전체 5초; 256행 이하 서버 페이지를 모아 시간순 반환, 제한 시 truncated 표시)
  - `devbox_problems{projectId}`(읽기, 최대 100개와 서버 nextCursor; 전체 문제라고 표시하지 않음)
  - `devbox_run_task{ projectId, id }`(쓰기, `[mcp] write = true`일 때만). 신뢰 승인이 없거나 낡으면 실행하지 않고 `projects.trust_required`를 돌려준다. MCP로 승인을 만들거나 미리보기를 승인할 수 없다.
- `agents.test`는 S1처럼 `TrustOperation::Test` snapshot의 `[agent] test`만 runtime으로 실행한다.

**핵심 테스트:**
- `mcp_logs_never_include_secret_values`: 비밀을 출력하는 작업의 로그를 MCP로 읽으면 값이 기존 서버 계약 `[REDACTED]`로 가려짐(초기 `****`는 예시였으며 응답 문자열을 변환하지 않음)
- `agents_test_runs_in_the_agent_worktree`(L2)
- `mcp_run_task_requires_trust_and_cannot_approve`
- `SecretsSettings` 단위(값 입력 후 입력란 비움, 목록에 값이 나오지 않음)

```bash
# 아래 단계별 검증·별도 검토 뒤 지정 메시지로 커밋한다.
```

**구현·직접 검증 기록 (2026-10-21, 로컬 완료):**
- 선행2 `3e43ef95` 뒤 AgentsService의 awaited runner lock·CLI 도메인 연결·runtime AgentTest 종류·신뢰 snapshot digest 재확인·실제 소유 cwd 보존을 구현했다. 동일 이름 일반 task와 representative/재시작 경계를 구분하고 에이전트 실행 탭·명시 실패 로그 전송을 연결했다. 대화형 AgentTask.terminalId는 유지한다.
- 비밀 공개 상태 metadata·값/복구 입력 비움·명시 복구 보기·확인된 재입력 초기화와 파일/DB 실패 보존을 구현했다. MCP 읽기3/쓰기1 도구는 직접 호출에서도 write gate와 기존 runtime 신뢰를 강제하며 승인 도구·자동 mutation 재시도는 추가하지 않았다. 실제 비밀 출력의 기존 `[REDACTED]` 가림을 보존한다.
- 직접 **고유73개 PASS**(부모30: agents/runtime/CLI12·UI18, MCP15, Secrets28). 실제 소유 systemd 작업 폴더/동일 이름 충돌/재실행·MCP 비밀 출력/신뢰 차단을 포함한다. 초기 terminalId→runId·RunTab·MCP·비밀 탭/요청ID assertion RED와 fixture 준비/비동기 타이밍/로그 형식 기대/잠금75를 구분했다. 최초 부모 UI 묶음의 helper6·기존Actions7 PASS는 그대로 재사용하고, fixture 알림 대기만 고친 RunTab5를 따로 통과했다.
- 앱/E2E/acceptance 정적 타입·생성 타입 check·변경 Rust39 포맷/TS·CSS12 Biome·diff·파일600/컴포넌트300 PASS. 제품/시험52개 digest `dbf77bf5664d654361f0a41c06666942adb0cc8398e5ca2d52537996825e620e`. 정확한 명령·시험명·raw/resources·실패 분류·동결 해시는 Git common-dir `devbox-verification/s2-task15/{summary,source-freeze,test-names,parent-commands,failure-classification,static-checks,documentation-freeze,artifact-hashes}.json` 및 MCP/Secrets 하위 기록에 모았다.
- **독립 검토 PASS·미해결0·완료 Biome337 PASS로 로컬 완료**했다. 제품52개·문서2개·artifact92개 해시와 고유73개(Rust40·UI33)를 확인했다. Biome는 0.910초·cgroup117,723,136B·swap0이다. 검토된 인계 summary/manifest는 보존하고 최종 상태·문서 해시·커밋은 `root-completion.json`에 기록한다. 전체 check/clippy/build/E2E·실제 Windows 앱/DPAPI·배포·push/PR은 미실행(Task16)이며 fake DPAPI 시험을 Windows 실기 PASS로 표시하지 않는다. native 소스는 바꾸지 않아 MSVC check를 반복하지 않았다. 다음은 Task15b이며 이번 변경에 Task15b/15c/16 기능은 없다.

---

### Task 15b: S3·S5 동시 진행 준비 — 공용 색인 DB·코드 보기·검사 대기

S2 다음에는 S3(기록)과 S5(개발 도구)를 한 세션이 두 트랙으로 동시에 진행한다(사용자 결정 2026-10-08, 00-roadmap §3.6). 두 쪽이 다 쓰는 바탕을 여기서 먼저 만든다. 그래야 두 브랜치가 같은 것을 따로 만들거나 서로를 기다리지 않는다.

**단계별 실행:**
- [x] **1 · 실패 테스트:** `core index_db 시험 · CodeView.test.tsx · scripts 자원 supervisor 회귀 시험`에 아래 출발 assertion과 이 과제의 핵심 테스트를 먼저 작성한다.
```rust
assert!(db.ensure_domain("api", 2, API_SCHEMA)?);
assert_eq!(read_notes_row(&db)?, "keep");
assert!(!db.ensure_domain("api", 2, API_SCHEMA)?);
assert_eq!(contended_default_lock().exit_code, 75);
```
- [x] **2 · 실패 확인:** `cargo test -p devbox-core ensure_domain`를 자원 제한 아래 실행한다. 미구현 타입/행동 때문에 실패했는지 확인하고 RED 출력을 artifact에 남긴다(환경·잠금 실패는 RED가 아님).
- [x] **3 · 구현:** IndexDb는 현재 Db의 WAL/단일 writer 패턴을 재사용하고 버전별 접두사 테이블만 transaction으로 재생성한다. CodeView/theme/languages와 의존성·생성물 merge 속성을 추가한다. supervisor --wait/--lock 구현은 기본 비차단 유지, deadline 60분/fixture 짧은 주입, check/E2E/dogfood 진입점 전달과 잠금 중복 방지를 포함한다. E2E 실패 수집은 해당 fixture tmux의 버전·서버 exit/log만 기록한다.

```rust
if read_domain_version(tx, domain)? == Some(version) { return Ok(false); }
drop_domain_tables(tx, domain)?;
tx.execute_batch(create_sql)?;
write_domain_version(tx, domain, version)?;
Ok(true) // all statements commit atomically; other domains remain unchanged.
```

- [x] **4 · 통과:** 같은 RED 명령과 직접 영향 시험으로 GREEN을 확인한다. 핵심 시험, 잘못된 domain/다른 domain 보존·migration rollback·CodeView readOnly·검사/default와 windows-acceptance 독립·서로 다른 worktree 공통 잠금·기한 자식0·기존 자원제한 유지 회귀를 수행한다. `.github`+`scripts` 합계1500줄/워크플로3개 제한도 확인한다.
- [x] **5 · 검토·커밋:** 별도 검토·Biome CI 뒤 `feat(core): prepare shared indexing code views and verification locks`. 실패·미실행 항목을 PROGRESS에 구분해 기록한다.

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
# 아래 단계별 검증·별도 검토 뒤 지정 메시지로 커밋한다.
# 묶음 R 끝: 과제별 좁은 검증·별도 검토 뒤 PROGRESS.md의 묶음 R 행과 현재 위치를 갱신해 로컬 커밋한다(00-roadmap §3).
```

**Task15b 구현·검증 기록 (2026-10-21, 독립 검토·완료 Biome PASS·로컬 완료):**
- `IndexDb`는 기존 Db의 writer/read pool 연결을 작은 내부 모듈로 공유하며, `index_schema`와 WAL만 만들고 백업은 만들지 않는다. ASCII 소문자+숫자 도메인 이름(`index`·`sqlite` 예약, `_` 불가), 정확한 접두사, FTS5 shadow table, 같은 버전 멱등, schema/metadata rollback, 동시 writer·읽기 snapshot을 검증했다. 개발자 schema SQL은 도메인 내부 객체만 정의하고 transaction 제어·교차 도메인 FK를 넣지 않는 계약이다.
- CodeView는 실제 CodeMirror 입력·JSON 강조·readOnly·props 재설정·언어 로드 경쟁/실패·unmount/StrictMode·한국어 검색을 검증했다. 6개 요청 의존성과 semantic tags용 직접 의존성 `@lezer/highlight`를 pnpm9 lock에 반영했고, 단일 theme은 기존 CSS 토큰을 사용한다. S3/S4/S5 화면은 구현하지 않았다.
- supervisor 기본 비차단 `lock`/`latest.json`은 유지한다. `--wait`는 최대3600초(시험 `DEVBOX_VERIFY_WAIT_SECONDS`도 양수·상한 검증), 기한 초과75/검사 대기 초과/자식0이다. named lock·`--no-lock`은 고유 `run-<id>.json`을 써서 동시 보고서 충돌을 막는다. check/E2E는 기본 대기+공통 잠금 한 번이며 잠금 해제/변경 옵션을 거절한다. dogfood 기본 호출은 직접 실행하고 명시 선두 `--wait --lock windows-acceptance --`만 supervisor로 전달한다. Task16은 **외부 supervisor가 다운로드→설치→검증→제거 전체를 감싸고 그 안에서 기본 dogfood를 호출**한다. standalone dogfood 잠금은 다운로드 범위뿐이다. 채널 옵션은 Task15c에서 구현한다.
- E2E private PATH의 tmux wrapper만 `-vv`를 쓰고 소유 디렉터리에 로그·명령 exit를 남긴다. 실제 new-session은 기존 `-c`를 유지한다. 버전·정리 전후 server 존재/PID·probe exit/signal·server/client/output log tail을 정리 후 artifact에 붙이며, 로그 수집 실패도 본문에 남긴다. daemonized server의 직접 wait exit code를 얻었다고 주장하지 않는다. 소유 tmux 실제 fixture만 실행했고 전체 E2E와 S1 비정상 종료 원인 확인은 미실행이다.
- **고유73개 PASS**: IndexDb12+기존Db18, CodeView18, scripts18, tmux7. missing module/type RED와 잠금 assertion RED, index 잠금75, 초기 jsdom geometry·병렬 시험 타입 오류, tmux fork 직후 cwd 관찰 경합을 구분했다. cwd 시험은 실제 값 readiness로 보정해 diagnostic2개만 재실행, 직전 통과한 cleanup/finalizer5개는 재사용했다. app/E2E/acceptance 타입·gen-check·frozen lock·변경 포맷·actionlint·bash syntax·diff PASS. 공통 Git 설정은 이미 true여서 변경하지 않았다. CI scope gate·시험 fixture를 공통화하고 읽을 수 있는 함수 구분을 유지해 `.github`+`scripts` **1497줄/2 workflows**(시험 포함), Task15c 잔여3줄이다.
- 독립 검토 R1에서 같은 도메인의 parent-first DROP이 FK에 막힘을 재현했다(NO ACTION SQLite787·RESTRICT1811). 정식 사례표 RED 뒤 재생성 transaction 안 `defer_foreign_keys=ON`으로 고쳤으며 두 action·동일 버전 false·다른 도메인 보존·실패 시 행/버전 rollback·다음 write의 즉시 FK 검사 복귀를 검증했다. 최종 Index12개 PASS, 공유 Db는 변경 없어 앞선18개 PASS를 재사용했다. 재검토 해결·추가 지적0이다.
- 제품/시험29개 digest `e5fc69b2ca6d38aff5cafdc84e8300020cf75810dc7754309d31a105131e2013`. 근거 `devbox-verification/s2-task15b/{summary,source-freeze,test-names,parent-commands,failure-classification,parent-static-checks,documentation-freeze,artifact-hashes}.json` 및 raw/resources. 독립 최종 검토 PASS·미해결0·완료 Biome344 PASS로 로컬 완료했다. 제품29개·문서2개·artifact77개 해시를 대조했고 Biome는 0.909초·cgroup196,661,248B·swap0이다. 검토된 인계 summary/manifest는 보존하고 완료 문서 해시·커밋은 `root-completion.json`에 기록한다. 다음은 Task15c다. 전체 check/clippy/build/E2E·Windows 실제 앱/배포·push/PR은 Task16 미실행이다.

---

### Task 15c: 수용 전용 설치 채널 (사용자 결정 2026-10-14)

S2 머지 뒤 사용자가 새 빌드를 prod로 쓰기 시작하면, 01-design §11의 "기존 v1 설치본·`devbox@prod`가 있으면 멈춰 묻기" 때문에 이후 모든 설치본 스모크가 멈춘다. S3·S5 두 트랙이 같은 prod 자원에 설치하면 서로 덮어쓴다. 그래서 에이전트 수용 검증 전용 `accept` 채널을 만든다(01-design §6.1·§10·§11).

**단계별 실행:**
- [x] **1 · 실패 테스트:** `app/src-tauri core channel 시험 · crates/cli/tests/setup_accept.rs · dogfood parser fixture`에 아래 출발 assertion과 이 과제의 핵심 테스트를 먼저 작성한다.
```rust
assert_ne!(channel(Prod).unit_template, channel(Accept).unit_template);
setup_accept(&owned_home)?;
assert_eq!(fingerprint(prod_binary), before_binary);
assert_eq!(fingerprint(prod_template), before_template);
```
- [x] **2 · 실패 확인:** `cargo test -p devbox-cli --test setup_accept accept_setup_leaves_prod_binary_and_template_untouched`를 자원 제한 아래 실행한다. 미구현 타입/행동 때문에 실패했는지 확인하고 RED 출력을 artifact에 남긴다(환경·잠금 실패는 RED가 아님).
- [x] **3 · 구현:** build-time Channel enum/설정표를 단일 원장으로 만들어 connection/deploy/supervisor/setup/hook CLI 경로에 전달한다. Tauri accept override·NSIS 식별자/AUMID/protocol/autostart·CI prod/accept 산출물·receipt channel과 dogfood --channel을 연결한다. 런타임 채널 변경 설정 없음. accept setup/cleanup은 prod template/bin에 쓰지 않는다.

```rust
pub enum AppChannel { Prod, Accept, Dev }
impl AppChannel {
    pub fn instance(self) -> &'static str { match self { Self::Prod => "prod", Self::Accept => "accept", Self::Dev => "dev" } }
    // identifier, protocol, deploy directory and unit template derive from this same value.
}
// build-time accept override is preserved through setup, hook binary and cleanup paths.
```

- [x] **4 · 통과:** 같은 RED 명령과 직접 영향 시험으로 GREEN을 확인한다. 핵심 시험과 identifier/프로토콜/앱data/경로표·owned-home setup 바이트/mtime 불변·receipt channel mismatch 거절을 실행한다. Windows NSIS self-check·실기 불변 비교는 Task16으로 남긴다. 묶음 R 기록을 갱신한다.
- [x] **5 · 검토·커밋:** 별도 검토·Biome CI 뒤 `feat(windows): add an isolated acceptance install channel`. 실패·미실행 항목을 PROGRESS에 구분해 기록한다.

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
# 아래 단계별 검증·별도 검토 뒤 지정 메시지로 커밋한다.
```

**구현·직접 검증 기록 (2026-10-21, 로컬 완료):**
- `protocol::channel` 단일 표와 native compile env, accept/dev Tauri override·merged config/self-check 대조, identifier 기반 앱 data/단일 인스턴스/autostart/AUMID, productName 기반 NSIS 폴더/제거 registry를 연결했다. Windows는 `win-package.ps1`이 cargo와 Tauri에 같은 채널/config를 전달하며 직접 Windows debug 명령에는 dev override가 필요하다. Linux core 검사는 무옵션으로 유지한다.
- accept config/bin은 `devbox-accept`, dev는 `devbox-dev`; 기존 prod 및 임의 test-* CLI 경로를 유지한다. data/state는 `devbox/<instance>`. 실제 accept unit은 `devbox-accept@accept.{socket,service}`·`devbox-accept-tmux@accept.service`, 별도 템플릿을 사용한다. 원래 XDG root·hook 실행 파일·OS 알림 URI를 맞추고 Windows root 미설정 오류는 유지한다.
- 동일 필수 Windows job에서 PR/manual의 두 산출물과 태그 prod만, 채널별 target/receipt·dogfood channel mismatch 거절을 구현했다. 공통 receipt/env·Python fixture로 구조 중복을 줄여 scripts+.github1500줄/2 workflows를 유지했다.
- 지정 setup RED→GREEN 및 실제 Secrets.save_name의 prod bytes/mtime 불변 포함 **고유79개 PASS(Rust75/Python4)**. 별도 PowerShell 모의2 suite·build-script8 case·MSVC accept compile·포맷/diff/600줄 PASS. 최초 MSVC 타입 오류와 WSL interop 환경 오류는 수정/재검증 기록을 보존한다. 실제 NSIS/Windows 앱 실행·전체 검사·설치 수용·push/PR은 Task16 미실행이다.
- 제품30개 digest `a78bc281b95279eb5768c92e8d256112e11c8faa24a75a2d8af87a51858b3dc1`, Git common-dir `devbox-verification/s2-task15c/`에 raw/resources/명령/시험명/제품·문서 별도 manifest를 기록했다. 독립 최종 검토 PASS·미해결0·완료 Biome344 PASS로 로컬 완료했다. 제품30개·문서2개·artifact81개 해시를 대조했고 Biome 0.603초·cgroup118,235,136B·swap0이다. 검토된 인계 manifest는 보존하며 완료 문서 해시·커밋은 `root-completion.json`에 기록한다.

---

### Task 16: S2 완료 — 전환점 확인

**관찰 UI 통합 보완 (2026-10-21, 로컬 완료):**
- 승인된 01-design §2·§8.2의 미연결 관찰 동작을 기존 실행 화면에 연결했다. `view=processes`는 현재 프로젝트를 전달하고 서버의 cwd 우선/나머지 CPU 상위50 순서를 유지한다. 포트·프로세스·WSL 화면은 실행 목록에서 이동하며, 숨김 상태 조회/3초 polling 및 프로젝트 변경 이전 응답 분리를 검증했다.
- 포트 브라우저는 기존 `openExternal`을 사용하며 wildcard IPv4/IPv6를 해당 loopback으로 바꾼다. UDP/잘못된 주소·포트는 비활성화한다. `PortInfo.startTimeMs: number|null`은 소켓 fd 스캔 전후 동일 start_ticks에서 계산하므로 상위50 프로세스 조회에서 누락된 PID도 확인 가능하다. stat/boot 확인 불가·container 행은 종료 불가다.
- 종료는 PID·이름·시작 시각을 보여 주고 고정한 snapshot으로 `observe.kill(term)`을 한 번 호출한다. stale 오류 뒤 자동 재시도하지 않고 조회 완료까지 버튼을 잠근 후 새 확인을 받는다. 본인 UID·PID 재사용 거절은 기존 백엔드 계약을 유지하며 시험은 소유 fixture/자식과 mock만 사용했다.
- 실제 RED Rust1(시작 시각 null)·UI3(동작/조회 누락)를 보존했다. 첫 GREEN의 stale 갱신 전 재클릭 실패1을 수정했고, 직접 고유33개(프런트21·Rust12), TS 생성·앱 타입 검사가 PASS다. 생성 주석 때문에 생긴 whitespace는 원본 주석을 바꾸고 `pnpm gen`으로 재생성했다. 근거 `devbox-verification/s2-task16/observe-ui/`.
- 이 기록은 관찰 UI 보완의 좁은 검증이다. 독립 검토 PASS·미해결0·완료 Biome350 PASS이며 제품21개·문서2개·artifact54개 해시를 대조했다. Biome0.896초·cgroup122,421,248B·swap0. 최종 문서/커밋은 `root-completion.json`에 기록한다. 첫 전체 검사·SC9·실제 Windows 설치 수용·필수 CI3·PR은 **미실행**이며 Task16/S2 완료로 표시하지 않는다.

**최종 로컬 검증 기록(2026-10-21):** 최초 전체 검사 Rust954 PASS/33 ignored·Vitest601 PASS/2 FAIL·E2E8 PASS/3 FAIL·Clippy FAIL을 보존했다. 원인별 수정 뒤 고유140개·strict Clippy·실패 E2E3·타입 PASS이며 통과한 무관한 검사는 반복하지 않았다. B1–B4 및 로그 초기 이력/진단 tmux PID/fixture를 고쳤고 제품75개 동결 digest `635d0359ebb1b4c83557d880b9e8c13bbb2244803ab207e2a022c31bfe486c9b`의 독립 검토 PASS·미해결0이다. SC9 60.000603초 CPU0.233331%·RSS39,899,136B PASS, 규모52,174줄·600/300 초과0·scripts1,500줄·workflow2·E2E11개다. 근거 `devbox-verification/s2-task16/{full,integration-fixes,branch-review,sc9}/` 및 PROGRESS §5. 실제 Windows PowerShell IPv6 회귀1개 PASS는 설치본 수용 PASS가 아니다. 수용 도구 지적4건은 수정·독립 재검토 PASS이며 Windows Parser2개도 PASS다. 최초 Windows CI39509088000은 native self_check의 collapsible_if1건으로 실패했고 같은 Windows-cfg strict Clippy RED→GREEN·기존 self-check3개 PASS로 보정했다. 새 SHA 실제 Windows CI·12개 설치 수용은 미완료다. 근거 `windows-clippy-fix/`와 PROGRESS §5.

**PR 수용 기록:** caf16863 Windows run39510897220·receipt/hash는 PASS. 설치 driver는 읽기 전용 ParseFile/Windows fingerprint errno110으로 설치 전 FAIL했고 기능1–11 NOT_RUN·12 FAIL·prod 불변 NOT_PROVEN이다. 소유 잔여0, 호스트 설정 변경 없음. #642 첫 코드 리뷰 scheduler1건과 CI 세션 assertion1건은 직접26개·영향 strict Clippy·독립 검토 PASS로 보완했다. 첫 보안 리뷰는 지적0건으로 완료됐으며 최종 CI·후속 리뷰·머지는 아직 완료되지 않았다. 자세한 근거와 회차 판정은 PROGRESS §5 및 `devbox-verification/s2-task16/pr/`를 따른다. 2회차는 코드3건·보안0건이며 readiness 귀속/선택 프로젝트/비밀키 DB 실패 복구와 CI3건을 보완했다. 직접 고유95개·독립 검토·영향 strict Clippy PASS, 3회차 리뷰·최종 CI/설치 시도·머지는 대기한다. 각 실패 원본과 소스 해시는 `pr/round2/`에 보존했다. 3회차는 코드1건·보안0건으로 완료했다. Run 프로젝트 귀속 우선 판정 수정은 직접2개·독립 검토·Clippy PASS이며 최종 SHA CI/설치 시도 뒤 squash한다. 자동 리뷰 상한3회를 채웠으므로 4회차를 기다리지 않고 차단 기준만 적용한다.

**단계별 실행(통합·수용 과제):**
- [x] **1 · 수용 실패 기준 고정:** 아래 12개 항목·SC9·필수 CI 3개·별도 검토·자동 리뷰 회차의 상태표를 artifact에 먼저 만들고 시작 상태를 `미실행`으로 둔다. Task 13의 E2E 2개와 기존 suite, 다중 로그/분리 창 부하, fallback·accept 불변 회귀를 포함한다. 제품 미구현/실패를 PASS로 바꾸지 않는다.
- [x] **2 · 실패 확인/최종 검사:** 과제별 GREEN 뒤 처음으로 `bash scripts/check.sh`(내부 supervisor→check-inner)를 실행해 공통 잠금/자원 제한 안에서 전체 check·clippy·build·E2E를 한 번 수행한다. supervisor로 check.sh를 다시 감싸 이중 잠금을 만들지 않는다. 이미 check가 포함한 검사를 별도 명령으로 반복하지 않는다. 실패는 해당 범위만 수정·재실행한다. SC9는 실행0인 데몬의 RSS/CPU 1분 표본과 명령/시각을 저장한다.
- [x] **3 · 구현·수정:** 실제 발견 결함만 좁은 회귀→수정→GREEN으로 해결한다. `python3 xtask/loc.py` 규모·E2E 총40개 이하·600/300줄 경계를 확인하고 브랜치 전체 별도 검토를 마친다. 불가 항목은 구체적 사유/증거와 함께 남긴다.

```bash
bash scripts/check.sh
# After focused repairs, branch review, Biome and final commit:
git push -u origin v1/s2-runs
gh workflow run windows.yml --ref v1/s2-runs
# Verify exact source SHA/artifact hashes; hold windows-acceptance across install→cleanup.
# Open S2 PR only here, then require final CI + bounded review before squash merge.
```

- [ ] **4 · 완료 커밋·설치 수용:** PROGRESS를 `구현 완료·PR 수용 대기`로 갱신하고 Biome CI 뒤 `chore(runs): record S2 verification and acceptance readiness`로 커밋한다. 최초 push→windows.yml dispatch→해당 SHA accept 산출물 검증→windows-acceptance 잠금 안 12개 스모크. 소유 리소스만 정리하며 수정이 필요하면 영향 검사·새 SHA 수용을 남긴다.
- [ ] **5 · PR·판정·머지:** Task16에서 S2 PR을 처음 연다. 최종 SHA 필수 CI3개, 자동 리뷰 최대3회(한도면 별도리뷰/미실행 기록), 회차별 묶음 수정 push를 끝내고 보호 규칙 그대로 squash 머지한다. 하네스가 병합을 막으면 사용자에게 요청한다. 실제 머지 확인 뒤 PROGRESS를 갱신하고 S3·S5 동시 다음 단계를 안내한다.

**Files:**
- Modify: 이 문서 상태 줄, `PROGRESS.md`(S2 완료·PR 행·현재 위치, 다음은 S3·S5 동시 시작), `python3 xtask/loc.py` 실행 결과를 PR 본문에 기록

- [x] **Step 1: SC9 측정:** 실행 중인 작업이 없을 때 데몬 RSS ≤ 60MB, CPU ≤ 0.5%(`/proc/<pid>/status`·`stat`를 1분 표본으로). 결과를 PR 본문에 쓴다. 넘으면 원인을 찾아 고친 뒤 머지한다.
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
