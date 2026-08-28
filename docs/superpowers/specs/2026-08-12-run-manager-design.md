# run-manager — 예약 실행·서비스 관리자 설계

- 날짜: 2026-08-12
- 브랜치: `docs/run-manager/design-spec`
- 범위: 신규 앱 `apps/run-manager`의 설계. **이 문서 자체가 산출물이며 구현은 하지 않는다.**
- 저장소 현재 사실: `crates/process` 추출은 이미 `origin/main`의
  `75b8af2 refactor(crates): extract process utilities`로 머지되었다. 이 문서는
  그 crate를 두 번째 소비자인 run-manager에서 사용하는 설계이며, process 추출 작업을
  다시 전제로 두지 않는다.

## 배경

개발 환경에는 `python main.py`처럼 정해진 시각에 한 번 실행하고 끝나는 작업과
`uv run uvicorn`, `npm start`, `docker run`처럼 계속 살아 있어야 하는 프로세스가
함께 존재한다. 이 둘을 같은 실행 목록에서 관리하면 사용자는 한 화면에서 활성화,
수동 실행, 이력, 로그를 확인할 수 있지만, 실행 수명과 실패 정책은 서로 다르다.
run-manager의 목적은 예약 실행(크론 잡)과 상시 실행(서비스)을 한 곳에서 관리하는
것이다.

현재 저장소에는 이 앱의 기능을 그대로 재사용할 구현이 없다. 대신 세 가지 운영
패턴은 이미 검증되어 있다.

- activity-timeline은 `setup`에서 `app_local_data_dir()`를 얻고 디렉터리를 만든 뒤
  `core::db::init(&dir.join("data.db"))`로 SQLite를 열며, 상태를 `app.manage`하고
  폴러와 트레이를 시작한다(`apps/activity-timeline/src-tauri/src/lib.rs:21-33`).
- 같은 앱의 `spawn_poller`는 `tauri::async_runtime::spawn` 안에서
  `tokio::time::interval`을 기다리고 전역 상태를 확인한 뒤 작업을 반복한다
  (`apps/activity-timeline/src-tauri/src/commands/tracking.rs:45-68`).
- 트레이는 `setup_tray`로 Show/Quit 메뉴를 만들고(`apps/activity-timeline/src-tauri/src/lib.rs:46-73`),
  창 닫기를 숨김으로 바꿔 백그라운드 작업을 유지한다
  (`apps/activity-timeline/src-tauri/src/lib.rs:35-40`).

따라서 run-manager는 새로운 외부 스케줄러를 붙이는 앱이 아니라, 이 저장소가 이미
사용하는 **트레이 상주 + 백그라운드 tokio 루프 + SQLite** 조합에 실행 대상과 이력
정책을 더하는 앱이다.

### 범위

**Phase 1 — job**: 이름, 명령, 작업 디렉터리, 환경변수, 실행 대상(Windows 또는
WSL 배포판)을 정의한다. cron 표현식과 빌더 UI(프리셋·직접 입력), 다음 실행 시각
N개 미리보기, 활성/비활성, 지금 실행, 중복 실행 정책, 실행 이력, stdout/stderr
분리 회전 로그 tail, 실패 Windows toast 알림을 제공한다. 예약 실행은 영속적인
occurrence claim으로 한 occurrence를 한 번만 소비한다. `kill-previous`와 데몬
종료에 필요한 Windows Job Object/WSL 내부 PID·PGID·SID session/group 제어도 Phase 1 실행 어댑터의
기초로 둔다. 이는 service UI를 앞당기는 것이 아니라 job의 명시된 취소 정책을
안전하게 구현하기 위한 수명 경계다.

**Phase 2 — service**: start/stop/restart, 데몬과 함께 자동 시작, 재시작 정책과
백오프, 프로세스 생존·포트 열림 헬스체크, 라이브 로그 tail을 추가한다. Phase 1의
Windows Job Object와 WSL 내부 PID·PGID·SID session/group 실행 어댑터를 service start/stop/restart가
재사용하며, service 전용 재시작·헬스체크 정책만 Phase 2에서 얹는다. 서비스는
Phase 1이 독립적으로 검증된 뒤에 착수한다.

**제외**: 의존성 그래프(A가 끝난 뒤 B 실행), 원격 호스트, 컨테이너 오케스트레이션,
Slack 등의 외부 알림 채널, 한 잡의 결과를 조건으로 다른 잡을 트리거하는 기능은
이 설계의 범위가 아니다.

## 결정과 근거

### 1. 자체 스케줄러 데몬을 소유한다

Windows Task Scheduler와 WSL crontab을 cron 백엔드로 프록시하지 않는다. run-manager가
자체 cron 표현식과 실행 시각을 소유하고, Task Scheduler에는 스케줄을 태스크로
복제하지 않는다. 자동 시작 자체는 뒤의 확정 결정처럼 시작프로그램 폴더 바로가기로
처리한다. 즉 외부 스케줄러가 run-manager의 잡 목록을 대신 실행하는 구조를 만들지
않는다.

`schtasks`는 단순한 반복만 표현할 수 있다. 예를 들어 `*/7 * * * *`는
`/SC MINUTE /MO 7`로 옮길 수 있지만, `0 9,13,18 * * *`는 하루 세 시각이므로
태스크 세 개로 쪼개야 한다. `0 0 13 * 5`의 cron DOM-OR-DOW 의미(매월 13일 **또는**
금요일)와 대응하는 단일 Task Scheduler 개념도 없다. `@reboot`와 초 단위 표현도
없다. 이 변환을 계속 추가하면 UI에 보이는 하나의 잡이 여러 외부 태스크로 분해되고,
표현식과 실제 실행 이력의 기준이 달라진다.

WSL crontab은 더 강한 전제를 요구한다. `/etc/wsl.conf`의 `systemd=true`,
`cron.service` enabled, WSL2 인스턴스가 떠 있는 상태가 모두 필요하고, Windows
로그온만으로 WSL 인스턴스가 자동 기동하지 않는다. 반면 자체 데몬은 어느 실행 대상이든
동일한 cron 의미론을 적용하고, WSL 잡을
`wsl.exe -d <distro> -- bash -lc <script>`로 실행한다. 이 호출 자체가 WSL 인스턴스를
기동하므로 crontab 서비스와 인스턴스 생존 여부를 별도로 관리할 필요가 없다.

기존 activity-timeline의 구조를 그대로 적용하는 이유도 여기에 있다. `lib.rs`의
초기화 순서(`apps/activity-timeline/src-tauri/src/lib.rs:21-33`)와 tokio 폴러
(`apps/activity-timeline/src-tauri/src/commands/tracking.rs:45-68`)만으로 데몬의
생명주기와 반복 실행을 설명할 수 있다. 외부 스케줄러별 변환 계층이라는 새 개념을
추가하지 않는다.

### 2. job과 service를 타입으로 분리한다

`uvicorn`, `npm start`, `docker run`은 실행 후 종료를 전제로 한 cron job이 아니다.
이들을 매 시각 다시 실행하면 이전 프로세스가 살아 있는 동안 새 프로세스가 누적된다.
따라서 `jobs.kind`를 다음 판별자로 둔다.

| 타입 | 의미 | 타입별 정책 |
|---|---|---|
| `job` | 한 번 실행하고 종료하는 작업 | 중복 실행: `skip` / `queue` / `kill-previous` |
| `service` | 계속 살아 있는 프로세스 | 재시작: `never` / `on-failure` / `always` + 백오프 |

Phase 1은 `job`만 구현한다. cron 계산, 데몬, 트레이, 실행 이력, 로그 tail, 빌더
UI를 먼저 검증한 뒤 Phase 2에서 `service`를 얹는다. service의 재시작·헬스체크
정책은 미루되, Phase 1에서 이미 요구하는 `kill-previous`와 orderly shutdown을
위해 프로세스 트리 제어(Job Object)와 WSL session/group 제어는 실행 어댑터에 넣는다.
그 결과 Phase 2는 새 spawn 경계를 만들지 않고 이 기반 위에 service 수명 정책만
추가한다.

### 3. 실행 대상별 어댑터와 명령 경계를 둔다

Windows 대상은 Windows 프로세스 실행 어댑터를 사용하고, WSL 대상은 배포판을 명시한
`wsl.exe -d <distro> -- bash -lc <script>` 호출 어댑터를 사용한다. 기존
wsl-desktop도 `CommandBuilder::new("wsl.exe")`로 `-d`를 조립하고, cwd가 있을 때
`--`, `bash`, `-lc`를 추가해 자식 프로세스를 시작한다
(`apps/wsl-desktop/src-tauri/src/commands/terminal.rs:73-85`). run-manager는
터미널 세션이 아니라 job/service의 stdout/stderr를 별도 파일로 연결한다.
이 코드는 argv 구성 형태만 참고한다. 그 구현의 cwd 문자열 조립
(terminal.rs:81)은 raw single quote에 의존하므로 run-manager의 안전한 quoting
구현으로 재사용하지 않는다.

Windows의 `command`는 사용자가 입력한 command line을 `cmd.exe /D /S /C <command>`의
shell syntax로 전달한다. custom `CreateProcessW` contract에서는 executable
application name과 command-line quoting을 분리하고, `/C` 뒤의 사용자 command
syntax를 normal argv처럼 quote해 literal로 바꾸지 않는다. `cwd`는 Windows process
creation 구조체의 current directory로, Windows env는 `Command::env` 의미로
inherit/override한다.

WSL의 `command`도 사용자가 입력한 shell command이지만, run-manager가 생성하는
cwd/env prefix는 command 문자열에 raw로 이어 붙이지 않는다. WSL 경로는 WSL-native
경로로 저장하고, Windows 경로 입력은 실행 전에 `wslpath -u`를
`wsl.exe -d <distro> -- wslpath -u -- <windows_path>`처럼 argv로 변환한다. 변환에
실패하면 실행하지 않고 `spawn_failed` 이력으로 남긴다. 최종 실행은
`wsl.exe -d <distro> --cd <wsl_path> -- setsid bash --noprofile --norc -lc
<wrapper+command>`이며, cwd는 `--cd` argv, env values는 `Command::env` 및
`WSLENV` names로만 전달한다. `bash -lc` script/args에는 secret env를 넣지 않는다.
각 `wsl.exe` argument는 `Command::arg` 경계(또는 같은 Windows argv quoting
encoder)를 통해 하나의 argv로 전달하고, `-lc` 뒤의 wrapper+user-command는 정확히
하나의 NUL-free argument가 되도록 safely quote한다. wrapper의 run ID/attempt token은
UUID 형식만 허용하고 fixed handshake fields로만 shell에 넣는다. 사용자 command는
의도한 shell syntax라서 내부 명령을 quote해 literal로 만들지 않지만, outer Windows
command line에서 argument boundary가 깨지지 않도록 encode한다. cwd/env 값을 raw
script로 이어 붙이거나 outer command line에 secret을 삽입하는 구현은 금지한다.
env key는 `[A-Za-z_][A-Za-z0-9_]*`만 허용하고 예약 key `WSLENV`와
`DEVBOX_RUN_MARKER`는 외부 job env 입력에서 거부한다. wrapper의 고정
marker/handshake와 사용자 command 자체만 shell syntax로 남긴다. `core::shell`은
NUL과 invalid/reserved key를 거부하는 순수 검증 helper이며 secret을 bash prefix로
quote하는 API가 아니다.

Windows에서 콘솔 창이 번쩍이지 않게 하는 처리는 실행 어댑터의 Windows 경계에
격리한다. life-log는 `tokio::process::Command`를 만들고 Windows에서만
`creation_flags(0x0800_0000)`를 적용한다(`apps/life-log/src-tauri/src/core/aggregate.rs:27-39`).
이 플래그는 실행 창을 숨기는 플랫폼 처리이지 cron·정책 로직이 아니므로 `core/`의
순수 함수에 넣지 않는다.

Phase 1에서 취소 가능한 모든 실행은 spawn 시점에 수명 제어를 확립한다. Windows
어댑터는 `tokio::process::Command`/`std::process::Command`의 일반 spawn을 사용하지
않고, Windows 경계에서 직접 `CreateProcessW`를 호출한다. mutable UTF-16 command
line으로 `cmd.exe /D /S /C <command>`를 만들고 `CREATE_SUSPENDED | CREATE_NO_WINDOW`로
시작한 뒤, 아직 resume하지 않은 `PROCESS_INFORMATION.hProcess`를
`CreateJobObjectW`로 만든 새 Job Object에 `AssignProcessToJobObject`하고
`SetInformationJobObject(JobObjectExtendedLimitInformation)`으로
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`를 설정한 뒤 `ResumeThread(hThread)`한다. 성공 후
thread handle은 adapter가 닫고 process/job
handle의 소유권을 async adapter로 넘겨 wait·log flush·종료 시점까지 보존한다.
`AssignProcessToJobObject` 또는 `ResumeThread`가 실패하면 suspended process를
terminate하고 모든 process/thread/job/pipe handle을 닫은 뒤 `starting`을 `failed`로
전환한다. 이 경계가 실패할 때 Job Object 없는 unsupervised `Command` fallback은
절대 사용하지 않는다. command-line contract는 executable/application name과
Windows command-line quoting을 별도로 유지하고, `/C` 뒤의 사용자가 입력한 command
syntax는 literal argv로 quote해 shell 의미를 바꾸지 않는 것이다. Resume 직후
`GetProcessTimes`로 `hProcess`의 creation time을 읽어 `target_pid`와
`target_process_created_at`을 함께 기록하며, startup 검증은 PID와 이 timestamp가
모두 일치할 때만 동일 process로 취급한다.

이 custom 호출의 handle 계약도 구현 명세에 포함한다. `STARTUPINFOEXW`의 inherited
stdout/stderr write handle만 child에 전달하고 parent read handle은 non-inheritable로
만든다. `CreateProcessW`에 넘기는 command-line buffer는 mutable NUL-terminated
UTF-16 storage의 소유자가 호출 중 유지한다. `PROCESS_INFORMATION.hThread`는
`ResumeThread`의 성공 여부를 확인한 직후 닫고, `hProcess`는 async wait task가
소유하며 Job handle은 정상 exit wait·log flush 또는 terminate escalation이 끝날
때까지 닫지 않는다. 어느 단계의 실패라도 suspended child를 먼저 terminate하고
process/thread/job/pipe handles를 각각 한 번만 닫는다. anonymous pipe가 overlapped
가 아니면 dedicated blocking reader task를 사용하고, overlapped로 만들면
completion ownership을 명시한다. 이 중 하나를 구현하지 않은 `Child` wrapper로
대체하거나 process handle만 받아 thread handle을 잃는 구현은 허용하지 않는다.

WSL 어댑터는 환경값을 script/argv에 삽입하지 않는다. Windows 쪽
`Command::env(key, value)`로 child environment를 만들고, 동일한 key를
`WSLENV`에 `<KEY>/w`로 병합해 Win32에서 WSL로 초기 environment를 전달한다. 기존
`WSLENV`에서 managed key와 `DEVBOX_RUN_MARKER`에 해당하는 모든 항목과 중복을
제거한 뒤 정확히 하나의 `<KEY>/w`와 marker 항목을 재생성하고, unrelated key/flag만
보존한다. cwd는
`wsl.exe`의 `--cd <wsl_path>` argv로 전달한다. 실행 argv는
`wsl.exe -d <distro> --cd <wsl_path> -- setsid bash --noprofile --norc -lc
<wrapper+command>`이며, 승인된 `bash -lc` shell boundary는 유지한다. `setsid`를
찾을 수 없거나 session을 만들 수 없으면 plain bash로 우회하지 않고 spawn을
실패시킨다.

wrapper는 예약 marker가 이미 초기 environment에 있는 상태에서 다음 framed
handshake를 stdout에 기록한 뒤 command를 `exec`한다. handshake는 첫 stdout line이라는
전제를 갖지 않는다.

```
__DEVBOX_RUN_HANDSHAKE_V1__
<run_id>
<pid>
<pgid>
<sid>
__DEVBOX_RUN_HANDSHAKE_END__
```

parser는 stdout의 어느 위치에서든 완전한 frame을 찾고, UUID·숫자 형식과
`/proc/<pid>/environ`의 exact `DEVBOX_RUN_MARKER=<run_id>`를 함께 검증한다. frame
뒤에는 `exec <command>`를 수행하며 command/env/cwd의 raw concatenation은 하지
않는다. run row에는 distro, 내부 PID, PGID, SID, marker를 저장한다. 저장 실패나
검증 실패 시 해당 session의 process group을 정리하고 run을 `failed`로 남긴다.

취소·stale recovery는 먼저 marker, PID, PGID, SID가 모두 같은 대상인지 검증한 뒤
`wsl.exe -d <distro> -- kill -TERM -- -<pgid>`처럼 numeric-only argv로
process-group `SIGTERM`을 보내고, 제한 시간 후 살아 있는 같은 group에
`wsl.exe -d <distro> -- kill -KILL -- -<pgid>`를 보낸다. `kill -0 -- -<pgid>`와
`/proc` 검사를 사용해 각 단계에서 group이 실제로 사라졌음을 확인한 뒤에만 run을
terminal 처리한다. 단일 PID `kill <pid>`는 사용하지 않는다. marker가
없는 자식이 group 밖으로 daemonize된 경우까지 재연결하거나 추적하는 것은 범위 밖이며,
session/group 안의 descendants는 모두 종료 대상으로 한다. Phase 2 service는 이
adapter와 동일한 handle을 재사용한다. Windows Job Object와 WSL session/group은
서로 대체하지 않는다.

### 4. SQLite를 사용한다

상태 위치는 `app_local_data_dir()` 아래의 `data.db`다. activity-timeline은 실제로
해당 디렉터리를 만들고 DB를 초기화한다(`apps/activity-timeline/src-tauri/src/lib.rs:21-24`).
run-manager는 잡 정의, 실행 이력, occurrence claim, 로그 디렉터리와 보관 상태를
조회해야 하므로 JSON 한 파일이 아니라 SQLite를 선택한다.

특히 다음 쿼리가 필요하다.

- 잡별 최근 50회 이력 조회
- 기간 필터와 시작 시각 정렬
- 상태·오류·exit code 조회
- 로그 보관 정책에 따른 오래된 실행 정리

activity-timeline의 `core/db.rs`는 `init`이 DB를 열고 `migrate`를 호출하는 구조
(`apps/activity-timeline/src-tauri/src/core/db.rs:4-24`)와 기간 범위·정렬 쿼리
(`apps/activity-timeline/src-tauri/src/core/db.rs:41-83`)를 이미 보여 준다. run-manager는
이 패턴을 잡·실행 이력에 적용한다. stdout/stderr 본문은 DB에 넣지 않고 회전 파일에
기록하며, DB에는 시각·exit code·상태·오류 메시지·앱이 생성한 상대 로그 디렉터리
같은 메타데이터만 저장한다.

### 5. 예약 occurrence를 DB에서 원자적으로 claim한다

스케줄러 tick은 같은 occurrence를 다시 계산할 수 있으므로 메모리의 마지막 실행
시각만으로는 충분하지 않다. `jobs.last_evaluated_at`을 epoch milliseconds로
저장하고, 각 자동 실행의 `runs.scheduled_at`을 cron이 산출한 canonical instant로
기록한다. `scheduled_at`과 `occurrence_wall_key`는 수동 실행에서는 `NULL`이다.
`runs`에는 자동 occurrence에 대해 `UNIQUE(job_id, occurrence_wall_key)`와
canonical `scheduled_at` 인덱스를 둔다. SQLite에서 `NULL`은 unique 충돌을
일으키지 않으므로 수동 실행은 여러 번 저장할 수 있다.

daemon은 기동할 때 한 번 `daemon_started_at`과 system-local wall-clock
`startup_cutoff`을 기록한다. `occurrence <= startup_cutoff`인 occurrence만 앱이
꺼져 있던 startup gap으로 간주한다. 각 job에는 per-job async mutex가 하나 있고, 그 mutex는
cron 평가, queue 소비, child monitor의 terminal 전환, stop/kill-previous를 모두
직렬화한다. SQLite transaction은 DB 작업만 포함하며 mutex/DB transaction을
terminate·wait 같은 `await` 너머로 유지하지 않는다.

각 scheduler tick은 `BEGIN IMMEDIATE`로 다음 DB 작업을 수행한다.

1. `jobs.last_evaluated_at`을 읽고 이전 checkpoint 이후 현재 시각까지의 due
   occurrence를 계산한다. 시스템 시각이 뒤로 이동한 경우 checkpoint보다 과거의
   occurrence는 다시 claim하지 않는다.
2. `occurrence <= startup_cutoff`인 startup-gap due에만 `catch_up`을 적용한다.
   `catch_up = true`이면 그 gap에서 가장 마지막 한 번만 후보로 남기고, `false`이면
   후보를 만들지 않은 채 checkpoint만 전진시킨다. `occurrence > startup_cutoff`인
   steady-state due는 `catch_up` 값과 무관하게 모두 후보로 남긴다. 따라서
   `catch_up = false`인 job도 정상 기동 후 매 tick의 due를 영원히 건너뛰지 않는다.
   한 tick이 지연되어 여러 steady due가 쌓여도 각 occurrence를 순서대로 claim한다.
3. `jobs.next_queue_sequence`를 `BEGIN IMMEDIATE` 안에서 증가시켜 claim row의
   `queue_sequence`를 할당한다. `INSERT INTO runs (..., scheduled_at,
   occurrence_wall_key, queue_sequence, status, ...)
   ... ON CONFLICT(job_id, occurrence_wall_key) DO NOTHING`으로 occurrence를
   claim한다. affected row가 1일 때만 이 daemon이 claim owner이고, 이미 있는 row이면
   process를 만들지 않는다. `last_evaluated_at` 전진과 sequence/claim은 같은
   transaction이다.
4. active run이 없으면 새 row는 `queued`로 남긴다. `skip`이면 occurrence를
   `skipped` terminal row로 claim한다. `queue`이면 FIFO `queued` durable row로
   보존한다. `kill-previous`이면 DB transaction에서 기존 `running` run을
   `stopping`으로 conditional update하고, 같은 transaction에서 이미 할당한
   `queue_sequence`로 새 occurrence를 `queued`로 claim하며
   `blocked_by_run_id = old_run.id`를 함께 기록한 뒤 commit한다. 그 다음 같은 job
   mutex를 유지한 채 **transaction 밖에서**
   기존 adapter handle에 terminate를 요청하고 wait한다. DB lock을 잡은 채 이 작업을
   await하지 않는다. `blocked_by_run_id`가 NULL이 될 때까지 이 row를 dequeue하지
   않는다.
5. adapter가 old process의 실제 종료와 모든 descendant cleanup을 확인한 경우에만
   짧은 새 transaction에서 old `stopping -> cancelled`를 affected-row CAS로
   확정하고, `blocked_by_run_id = old_run.id`인 linked row를
   `blocked_by_run_id = NULL`로 푼 뒤에만 새 row의 `queued -> starting` CAS를
   수행한다. 종료 확인 전에는 old run을 `cancelled`로 기록하거나 linked row를
   dequeue/spawn하지 않는다. terminate/timeout/identity validation이 실패하면 old
   run을 오류와 함께 `failed`로 terminal 처리하고, 같은 transaction에서
   `status = 'queued' AND blocked_by_run_id = old_run.id`인 linked row도
   affected-row CAS로 `failed` 처리하며 새 spawn은 금지한다.
6. 일반 `queued` row를 실행할 때도 transaction에서
   `UPDATE runs SET status = 'starting', owner_instance_id = ?,
   attempt_token = ? WHERE id = ? AND status = 'queued' AND
   blocked_by_run_id IS NULL`를 수행한다. job별 queue worker는
   `ORDER BY queue_sequence`로 가장 앞의 unblocked row만 dequeue한다. affected
   row가 정확히 1인 daemon만 해당 attempt를 진행한다. 이 CAS와 attempt token
   검증은 queue worker 간 중복 spawn을 막는다.
7. DB transaction을 commit한 뒤에만 process를 spawn한다. Windows adapter는
   Job Object assignment/resume까지, WSL adapter는 framed handshake와 PID/PGID/SID
   identity validation까지 같은 `attempt_token` owner가 수행한다. 성공 시
   `starting -> running`을 owner/token conditional update하고, spawn·handshake·로그
   준비 실패는 owner/token을 확인해 `failed`로 갱신한다.

프로세스가 시작된 뒤 daemon이 죽는 crash window는 external process와 SQLite commit을
하나의 원자 연산으로 만들 수 없다. 그러므로 `starting` 또는 `stopping` row를
startup에서 절대 새 process로 respawn하지 않는다. Windows Job Object handle은
의도적으로 anonymous이고 crash 뒤 recover/reopen하지 않는다. daemon이 죽으면
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`에 따라 handle close와 함께 child cleanup이
보장되므로 startup은 Job을 다시 열거나 확인한다고 주장하지 않는다. 대신 저장한
`target_pid`와 `target_process_created_at`으로 PID가 사라졌는지 확인한다. PID가
다른 creation time으로 재사용됐으면 절대 kill하지 않고 stale run을 실패 처리한다.
둘 중 하나가 저장되지 않은 ambiguous starting row도 PID를 추측해 kill하지 않고
실패와 fail-safe report만 남긴다.
동일 creation time의 process가 예상 밖에 남아 있으면 blind kill하지 않고 fail-safe
오류를 보고하며 ambiguous run을 `failed`로 terminal 처리한다. WSL은
marker·PID·PGID·SID를 exact validation한 뒤 session/group을
SIGTERM→timeout→SIGKILL로 정리하고 실제 소멸을 확인한다. `kill-previous`의
`stopping` old run은 cleanup이 확인된 경우에만 affected-row CAS로 `cancelled`
처리하고 linked `blocked_by_run_id` row를 unblocked queued로 푼다. PID identity
불일치·matching process 잔존·group cleanup timeout 같은 실패는 old와 linked queued를
각각 affected-row CAS로 `failed` 처리한다.
오직 unblocked `queued` row만 durable resume 대상이다. 이렇게 at-most-once
occurrence claim과 crash 후 no-respawn 규칙을 분리해 중복 외부 side effect를 피한다.

SQLite transaction은 DB connection 간에도 unique claim을 직렬화한다. 따라서
single-instance plugin이 우발적 이중 실행을 막는 것과 별개로, 두 tick이 동시에
평가해도 같은 automatic occurrence가 두 번 claim되지 않는다. job을 새로 만들거나
cron/catch-up/enabled 값을 바꾸는 transaction은 `last_evaluated_at`과
`startup_cutoff` 기준을 그 시각으로 재설정해 이전 표현식의 missed occurrence를
새 표현식에 적용하지 않는다. 수동 실행은 같은 per-job mutex, `next_queue_sequence`
allocator/`queue_sequence`, starting CAS, adapter, 로그 경로를 사용하지만 checkpoint와
automatic wall key를 갱신하지 않는다. 수동 row도 `queue_sequence`를 할당하되 FIFO
정렬은 queued policy에만 적용한다.

### 6. 환경변수는 Windows DPAPI로 보호한다

`jobs`에는 환경변수의 평문 JSON 컬럼을 저장하지 않는다. Windows target에서 command layer가
받은 환경변수 map을 `CryptProtectData`의 CurrentUser scope로 암호화하고, DB에는
`env_ciphertext` BLOB만 저장한다. 실행 직전에 `CryptUnprotectData`하고 process
adapter에 전달하며, 복호화 실패는 실행하지 않고 `failed` run의 `error_message`로
기록한다. 암호화·복호화는 `platform/windows.rs`의 `#[cfg(target_os = "windows")]`
경계에만 두고 `core/`와 공용 crate에는 Windows API를 넣지 않는다. Linux CI에서
순수 로직을 테스트할 때는 `EnvProtector` trait의 in-memory fake만 주입하며, fake
암호문도 DB에 평문으로 쓰지 않는다.

환경 조회 API는 key, configured 여부, 항상 mask인 값을 반환하고 plaintext를 조회하지
않는다. update DTO는 각 key에 대해 `Keep`(기존 ciphertext 유지),
`Set { value }`(새 plaintext를 받아 재암호화), `Clear`(key 제거) 중 하나만
허용한다. masked sentinel을 `Set`의 secret으로 취급하지 않으며, `Keep` 요청에
값을 함께 받지 않는다. DPAPI가 실패하면 기존 ciphertext를 임의로 평문 fallback하지
않고 update/run을 실패시킨다.

Windows child의 environment에는 값이 필요하므로 WSL에서는 `Command::env`와
`WSLENV` names로만 초기 environment에 전달한다. DPAPI 값은 bash argument,
`bash -lc` script, cwd argv, error/toast, stdout/stderr log에 넣지 않는다.
사용자가 입력한 command 자체는 shell command boundary이므로 command line에 나타날
수 있지만, stored environment secret을 command line에 삽입하는 구현은 금지한다.
실행 시작 시 decrypt한 map의 snapshot을 해당 run 전용 byte redactor에 넘긴다.
job을 편집해 secret이 바뀌어도 이미 시작한 run은 old secret snapshot을 끝까지
redact한다.

redactor는 raw bytes를 왼쪽부터 찾고, 동일 위치에서 겹치는 secret은 가장 긴 값부터
선택해 한 번만 `<redacted>`로 치환한다. 빈 secret은 무시하고, 중복 secret은
deduplicate한다. chunk 경계에서는 최대 secret byte 길이 - 1을 carry하고 EOF에서
flush해 byte를 잃거나 두 번 쓰지 않는다. UTF-8이 아닌 child output도 byte sequence로
처리하되 replacement는 ASCII bytes로 고정한다. plaintext map, key/value byte buffer와
redactor carry는 child 종료·spawn 실패·취소의 모든 경로에서 `zeroize` best-effort로
지우고 ownership을 drop한다. redaction 실패 자체는 secret을 오류 메시지에 복사하지
않고 고정 오류 코드만 남긴다.

## 공통 추출

CONVENTIONS의 공통화 원칙은 "두 번 이상 실제로 필요해진 코드만
`packages/`·`crates/`로 추출한다"는 것이다(`CONVENTIONS.md:17-19`). 그 조건에 따라
`crates/process`는 현재 workspace에 존재하고, port-manager가 첫 소비자로 사용한다.
run-manager는 service 헬스체크·프로세스 조회의 두 번째 소비자로 이 이미 머지된 API를
사용한다.

| 대상 | port-manager에서 확인한 위치 | `crates/process` 판단 | 이유 |
|---|---|---|---|
| `PortInfo` 데이터 구조 | `crates/process/src/models.rs`; port-manager import는 `apps/port-manager/src-tauri/src/commands/ports.rs:4` | 현재 API | 포트·PID·상태를 표현하는 OS 비의존 값이다 |
| `parse_netstat_output`, `extract_port` | `crates/process/src/netstat.rs`; port-manager 호출은 `apps/port-manager/src-tauri/src/commands/ports.rs:31-34` | 현재 API | 문자열 파싱만 하며 crate 내부 테스트로 검증한다 |
| sysinfo 기반 PID 조회·생존 확인의 공통 primitive | `crates/process/src/process.rs:3-64`; port-manager snapshot 조립은 `apps/port-manager/src-tauri/src/commands/ports.rs:36-44` | 현재 API | `ProcessId`, `ProcessSnapshot`, `lookup_process`, `is_process_alive`를 공용으로 제공하고 `ProcessInfo` UI DTO와 kill command 조립은 소비자에 남긴다 |
| Windows `netstat -ano` 실행 | `apps/port-manager/src-tauri/src/commands/ports.rs:51-65` | 남김 | `std::os::windows::process::CommandExt`, `creation_flags`, Windows 명령 호출이다 |
| `PortRow`와 netstat 결과를 UI 행으로 조립 | `apps/port-manager/src-tauri/src/commands/ports.rs:20-26`, `33-51` | 남김 | port-manager 화면의 `process_name` 표현에 특화된 조합이다 |
| Windows Job Object | run-manager Phase 1 adapter | 남김 | `windows` API와 프로세스 트리 수명은 run-manager 플랫폼 계층에 두며 `kill-previous`/shutdown 때문에 service 전용으로 미룰 수 없다 |
| WSL 내부 PID/PGID/SID 수집·session 종료 | run-manager Phase 1 adapter | 남김 | WSL 호출·marker/group 프로토콜은 run-manager 실행 수명에 종속되며 Phase 2 service가 재사용한다 |

이미 머지된 crate는 run-manager 구현 PR과 별개로 유지한다. port-manager의
`run_netstat`는 그대로 남기고, 그 출력만 `crates/process`의 파서에 넘긴다. run-manager도 Windows에서
netstat를 실행할 때는 자기 플랫폼 계층에서 실행하고 동일한 순수 파서를 재사용한다.
이렇게 하면 CONVENTIONS의 "crates 안에 Windows 전용 코드(`windows` crate 등)를
넣지 않는다"는 규칙(`CONVENTIONS.md:137-140`)을 지키면서 포트 열림 판정과 파싱을
공유할 수 있다.

`crates/process`에는 Job Object나 WSL PID/PGID/SID control을 넣지 않는다. Windows 전용 API는
공용 crate로 옮기지 않고 run-manager의 Phase 1 플랫폼 모듈에 격리한다. WSL 서비스
종료도 Windows 쪽 `wsl.exe` 프로세스 종료와 동일하지 않으므로 공용 단일 kill 함수로
뭉치지 않는다. 두 메커니즘은 모두 필요하지만 수명 모델이 다르다.

현재 root `Cargo.toml`에는 앱들과 `crates/filesystem`, `crates/markdown`,
`crates/process`가 workspace member로 들어 있다(`Cargo.toml:5-21`). 저장소의
오래된 AGENTS 상태 문구와 달리 이 세 crate는 실제 workspace 사실이다. process
crate의 현재 package/import 이름은 package `process`, Rust crate import
`devbox_process`이며 run-manager는 이 공개 API를 path dependency로 사용한다.

## 데이터 모델

### `jobs`

잡과 서비스의 공통 정의를 한 테이블에 두고 `kind`로 판별한다. 두 타입이 이름,
명령, cwd, 환경변수, 실행 대상, 이력·로그 연결을 공유하므로 타입별로 테이블을
나누지 않는다.

개념적 컬럼은 다음과 같다. 정확한 SQLite 타입·기본값·인덱스는 구현 PR에서
확정해야 할 항목으로 남긴다.

| 컬럼 | 의미 |
|---|---|
| `id` | 잡 식별자 |
| `name` | UI에 표시할 이름 |
| `kind` | `'job'` 또는 `'service'` 판별자 |
| `command` | 실행할 명령 |
| `cwd` | 작업 디렉터리 |
| `env_ciphertext` | Windows DPAPI CurrentUser로 보호한 환경변수 map의 ciphertext; 평문 JSON은 저장하지 않음 |
| `target_kind` | `windows` 또는 `wsl` |
| `target_distro` | WSL 대상일 때의 `<distro>`; Windows 대상은 비어 있음 |
| `cron_expr` | job의 cron 표현식; service에는 적용하지 않음 |
| `enabled` | 활성/비활성 토글 |
| `overlap_policy` | job의 `skip` / `queue` / `kill-previous` |
| `catch_up` | `occurrence <= startup_cutoff`인 downtime gap의 missed occurrence를 마지막 한 번 따라잡을지 여부(잡별 설정); cutoff 이후 steady due에는 적용하지 않음 |
| `last_evaluated_at` | scheduler가 이 job의 due occurrence를 평가해 checkpoint를 전진시킨 epoch milliseconds; 수동 실행은 갱신하지 않음 |
| `next_queue_sequence` | job별 queue sequence allocator. `BEGIN IMMEDIATE` transaction에서 증가시켜 runs의 FIFO sequence를 할당함 |
| `restart_policy` | service의 `never` / `on-failure` / `always` |
| `auto_start` | Phase 2에서 데몬 기동과 함께 service를 시작할지 여부 |
| `health_tcp_address` | service의 optional local TCP probe 주소; NULL이면 TCP probe를 사용하지 않음. UI 입력 형식은 미결 |
| `health_tcp_port` | service의 optional local TCP probe port; NULL이면 TCP probe를 사용하지 않음 |
| `health_start_grace_ms` | service 시작 직후 TCP failure를 유예할 값; 정책 수치와 UI 표현은 미결이며 process exit는 즉시 failure |
| `created_at`, `updated_at` | 정의 변경 시각 |

`cron_expr`, `overlap_policy`, `catch_up`, `last_evaluated_at`은 Phase 1에서 사용하고,
`restart_policy`, `auto_start`는 Phase 2에서 사용한다. `kind`, `target_kind`와
job/service별 필수 컬럼은 command layer와 migration의 CHECK 조건으로 검증한다.
환경변수 암호문은 Windows에서만 생성·복호화하며, DPAPI 오류가 나면 평문 fallback을
하지 않는다. service health interval/timeout/failure threshold와 restart backoff
단계는 각각 10초/3초/3회 및 1/2/4/8/16/30초로 고정하며, 포트 설정 UI 형식만
`미결`에 둔다.

### `runs`

한 번의 실행을 한 행으로 기록한다. job과 service가 같은 이력 화면과 로그 경로
규칙을 공유하도록 `job_id`만 참조하고, 서비스가 추가되어도 실행 이력 테이블을
복제하지 않는다.

| 컬럼 | 의미 |
|---|---|
| `id` | 실행 식별자 |
| `job_id` | `jobs.id` |
| `scheduled_at` | 자동 job의 canonical cron occurrence epoch milliseconds; 수동 실행은 `NULL` |
| `occurrence_wall_key` | 자동 occurrence의 local wall fields canonical key(예: 날짜·시·분·초); ambiguous DST를 earliest offset 한 번으로 dedupe하며 수동 실행은 `NULL` |
| `queue_sequence` | 모든 run claim transaction에서 job별 `next_queue_sequence`로부터 monotonic하게 할당하는 `INTEGER NOT NULL`; queued FIFO의 유일한 정렬 기준(간격은 허용) |
| `blocked_by_run_id` | `kill-previous`가 만든 queued row가 old stopping run의 cleanup을 기다릴 때 참조하는 `runs.id`; cleanup 확인 후 NULL, 그 전에는 dequeue 금지 |
| `started_at` | 실제 process 시작 시각; `queued`/`skipped`에서는 `NULL`일 수 있음 |
| `ended_at` | terminal 상태가 된 시각; 실행 중에는 `NULL` |
| `exit_code` | process exit code; spawn 실패·skip·queue에는 `NULL` |
| `status` | `queued`, `starting`, `running`, `stopping`, `succeeded`, `failed`, `cancelled`, `skipped` 중 하나 |
| `owner_instance_id` | 이 daemon instance가 `queued -> starting` CAS를 성공시킨 instance UUID; queued/terminal에서는 NULL일 수 있음 |
| `attempt_token` | 한 spawn attempt의 random token. `starting -> running/failed`와 adapter handshake/cleanup를 owner와 함께 CAS 검증하는 값 |
| `error_message` | spawn 실패, DPAPI/경로 오류, 취소 원인 등 사용자에게 보여 줄 비밀값 없는 오류; 없으면 `NULL` |
| `target_pid` | WSL 내부 PID 또는 확인 가능한 Windows process ID; 없으면 `NULL` |
| `target_process_created_at` | Windows `target_pid`의 process creation time을 UTC epoch milliseconds로 저장; WSL/해당 없는 run은 `NULL`이며 stale recovery의 PID 재사용 검증에만 사용 |
| `target_pgid` | WSL process group ID; Windows/해당 없는 run은 `NULL` |
| `target_sid` | WSL session ID; Windows/해당 없는 run은 `NULL` |
| `process_marker` | WSL stale-run 검증용 app-generated marker; Windows 또는 해당 없는 run은 `NULL` |
| `log_dir` | 앱이 run ID로 생성한 로그 디렉터리의 app-local-data 기준 상대 경로. 사용자 입력·절대 경로는 저장하지 않으며 로그 reference가 제거되면 `NULL` |
| `logs_deleted_at` | terminal run의 log files/reference를 제거한 시각; metadata row가 남아 있으면 non-NULL |

`UNIQUE(job_id, occurrence_wall_key)`와 canonical `scheduled_at` 인덱스로 automatic
occurrence를 claim한다. SQLite의 unique 제약은 `NULL`을 서로 충돌시키지 않으므로
수동 실행에는 적용되지 않는다. `skipped`는
실행되지 않은 occurrence를 이력에 남기며 `log_dir`가 없다. `queued`는 queue
정책의 durable 대기 항목이고 `queue_sequence` 오름차순으로 소비한다. process를
만들기 직전에 로그 디렉터리를 생성한다. `blocked_by_run_id`가 있는 queued row는
old run cleanup이 확인될 때까지 소비하지 않는다.
`starting`은 owner/token CAS를 거쳐 외부 spawn attempt가 진행 중인 상태이며,
`stopping`은 `kill-previous`가 old process의 실제 종료를 기다리는 상태다. 이 두
상태는 startup에서 respawn하지 않는다. `cancelled`는 사용자 취소 또는
`kill-previous`/orderly shutdown 후 실제 종료가 확인된 run이며,
`failed`는 non-zero exit code 또는 process spawn/로그/복호화 실패다. spawn 실패처럼
exit code가 없는 terminal run도 `error_message`로 원인을 보존한다.
`target_pid`/`target_pgid`/`target_sid`/`process_marker`는 WSL handshake
validation 후 owner/token과 함께 저장하며, Windows run은 `target_pid`와
`target_process_created_at`을 CreateProcessW 직후 저장한다. identity가 없는 queued,
skipped run에는 해당 컬럼을 NULL로 저장한다. `owner_instance_id`와 `attempt_token`은 terminal 전환
시에도 CAS 조건으로 검증한 뒤 지우거나 보존한다.
spawn·wait·stale-recovery가 exit code를 얻지 못한 오류는 항상 `failed`와
`exit_code = NULL`로 남기고, 의도적인 취소만 `cancelled`로 남긴다.

새 enabled job은 생성 transaction의 현재 epoch milliseconds를
`last_evaluated_at`으로 초기화한다. disabled job을 enable하거나 cron/catch-up을
수정할 때도 같은 transaction 시각으로 checkpoint를 재설정해 이전 표현식의
occurrence를 새 설정으로 소급하지 않는다.

UI는 `started_at`, `ended_at`, `exit_code`, `status`, `error_message`, `log_dir`를
사용해 실행 이력을 표시하고, 실제 로그는 `run_id`와 `stdout`/`stderr` stream을
전달해 tail한다. 로그 본문은 `runs`에 저장하지 않는다. 기간 조회는 시작 시각을
기준으로 필터하고 정렬한다.

### `service_instances`

Phase 2 service는 `runs.scheduled_at = NULL`인 수동 run을 여러 개 만들 수 있다는
사실과 별개로, job마다 실제 active service instance는 하나만 허용한다. 다음
durable table을 `job_id PRIMARY KEY`로 둔다.

| 컬럼 | 의미 |
|---|---|
| `job_id` | `jobs.id`와 1:1인 service identity |
| `generation` | start/stop/restart마다 증가하는 monotonic generation |
| `active_run_id` | 현재 generation의 run; 없으면 `NULL` |
| `state` | `stopped`, `starting`, `running`, `stopping`, `retry_waiting` |
| `owner_instance_id` | 현재 daemon instance UUID |
| `attempt_token` | 현재 spawn/health attempt token |
| `next_retry_at` | backoff 후 재시작 시각; 없으면 `NULL` |
| `consecutive_failures` | 현재 generation의 연속 health failure 수 |
| `updated_at` | 마지막 상태 변경 시각 |

start/auto-start는 transaction 안에서 `state = stopped` 또는 해당 generation의
terminal 조건을 `WHERE`에 넣은 conditional update로 claim한다. stop/restart는
먼저 generation을 증가시키고 이전 generation의 retry/health event를 무효화한다.
모든 health 결과, retry timer, process exit callback은 `job_id + generation +
attempt_token`을 CAS 조건으로 사용한다. 따라서 stop 직후의 늦은 health failure가
새 start를 resurrect하지 않고, auto-start와 사용자의 start가 동시에 와도 한
generation만 spawn한다. startup에서 nonterminal instance를 발견해도 queued
occurrence처럼 새 service를 spawn하지 않고 adapter cleanup 후 stale generation을
terminal/failed 처리한다.

### `notification_outbox`

Rust-side toast adapter가 runtime permission/API 오류를 받거나 webview가 아직
load되지 않은 경우를 위해 optional durable queue를 둔다. 컬럼은 `id`,
`kind = 'run-failed'`, `job_id`, `run_id`, `error_code`, `created_at`,
`delivered_at`이며 job name과 고정 error code 외 secret/error text는 저장하지 않는다.
startup/foreground와 무관하게 adapter가 drain을 시도하고, permission 거부·toast
실패는 run/scheduler 상태를 rollback하지 않는다. delivered row의 retention은 구현
PR에서 metadata cleanup과 함께 처리한다.

### `meta`

처음부터 `meta(key, value)`를 둔다. `key = 'schema_version'` 행을 migration 때
읽고, 버전이 올라가면 명시적인 마이그레이션을 수행한다. everything-plus는 초기
스키마에 버전 개념이 없었다가 `meta`를 뒤늦게 추가한 사례다. 현재 구현은
`SCHEMA_VERSION`을 선언하고(`apps/everything-plus/src-tauri/src/core/db.rs:4-7`),
`meta` 테이블을 생성한다(`apps/everything-plus/src-tauri/src/core/db.rs:95-99`),
낮은 버전이면 파생 데이터를 지운 뒤 값을 갱신한다
(`apps/everything-plus/src-tauri/src/core/db.rs:108-128`). run-manager는 이
교훈을 처음부터 반영한다.

## 모듈 구조

CONVENTIONS의 Tauri 구조는 `lib.rs`를 진입점·command 등록·상태 초기화로 두고,
commands를 얇게, `core/`를 OS 비의존 순수 로직으로 둔다
(`CONVENTIONS.md:126-141`). 이를 run-manager에 적용하면 다음과 같다.

```
apps/run-manager/
├─ src/
│  ├─ App.tsx
│  ├─ api.ts
│  ├─ types.ts
│  ├─ pages/
│  │  ├─ JobsPage.tsx
│  │  ├─ JobEditorPage.tsx
│  │  └─ HistoryPage.tsx
│  ├─ components/
│  │  ├─ CronBuilder.tsx
│  │  ├─ NextRunsPreview.tsx
│  │  ├─ JobTable.tsx
│  │  ├─ RunHistory.tsx
│  │  └─ LogTail.tsx
│  └─ lib/
│     └─ historyFormat.ts
└─ src-tauri/
   └─ src/
      ├─ lib.rs                 # run(), 상태·DB·데몬·트레이 초기화
      ├─ main.rs
      ├─ commands/
      │  ├─ mod.rs
      │  ├─ jobs.rs             # 잡 CRUD, 활성화, 지금 실행
      │  ├─ history.rs          # 이력 조회·기간 필터
      │  ├─ logs.rs             # 로그 tail
      │  └─ services.rs         # Phase 2 start/stop/restart/health
      ├─ core/
      │  ├─ models.rs
      │  ├─ cron.rs             # 표현식 검증·다음 실행 시각
      │  ├─ policies.rs         # catch-up·중복·재시작 판정
      │  ├─ shell.rs            # command boundary·key/path validation 순수 함수
      │  └─ retention.rs        # 최근 50회·200MB 정리 계산
      ├─ db.rs                  # SQLite init/migrate/queries
      └─ platform/
         ├─ windows.rs          # cfg(windows) 실행·Job Object·DPAPI·toast/startup 경계
         └─ wsl.rs              # wsl.exe 실행·PID/PGID/SID session/group 종료 경계
```

`core/`에는 `std::process::Command`, `wsl.exe`, Windows API, 파일 tail 같은 IO를
넣지 않는다. `cron.rs`, `policies.rs`, `shell.rs`, `retention.rs`는 입력값을 받아
결과를 반환하는 순수 함수여야 WSL에서 `cargo test`로 검증할 수 있다. DPAPI,
Job Object, toast, startup shortcut은 `#[cfg(target_os = "windows")]` 플랫폼 경계에
격리하고 Linux compile-check에는 명시적인 unsupported stub만 제공한다. `commands/`는
serde 구조체를 받고 상태·DB·플랫폼 어댑터를 조합하는 얇은 Tauri command 계층으로
둔다. 이는 command 파라미터를 serde 구조체로 묶고 결과를 `Result<T, String>`으로
돌려주라는 규칙(`CONVENTIONS.md:119-124`)에도 맞는다.

트레이 초기화와 창 수명은 `lib.rs`가 담당한다. activity-timeline의 `setup`에서
DB와 상태를 만들고 `spawn_poller`·`setup_tray`를 호출하는 순서
(`apps/activity-timeline/src-tauri/src/lib.rs:21-33`)를 따르며, 창 닫기는 숨긴다
(`apps/activity-timeline/src-tauri/src/lib.rs:35-40`). Quit 메뉴는 기존 앱처럼
`app.exit(0)`를 바로 호출하지 않고 shutdown future를 시작한다. Tauri
`RunEvent::ExitRequested`와 Windows logoff/shutdown session-end hook도 같은
idempotent shutdown coordinator에 연결해 scheduler 중지, retry 취소, handle
terminate/wait/escalation, 로그 flush, terminal 상태 반영이 끝난 뒤에만 exit를
허용한다. 실패 알림은 Rust-side notification adapter로 별도 처리하며
`app.exit`를 알림 근거로 사용하지 않는다. `tauri-plugin-single-instance`는 두
번째 실행의 인자/cwd를 기존 창으로 전달하고 새 scheduler를 만들지 않도록
`tauri::Builder`의 다른 plugin/setup보다 먼저 등록한다. `src-tauri/capabilities/default.json`에는
`"notification:default"` permission을 `core:default`, `opener:default`와 함께
선언하고, Rust builder에는 notification plugin을 초기화한다. Rust adapter는
sanitized job name/run ID/fixed error code만으로 Windows toast를 직접 시도하고,
hidden/unloaded webview에 의존하지 않는다. permission/API 오류는
`notification_outbox`에 sanitized event를 보관하거나 nonfatal error로 기록하며
scheduler/run state를 중단시키지 않는다.

## 데이터 흐름

### 시작과 데몬

1. Tauri `setup`에서 `app_local_data_dir()`를 얻고 `data.db`의 부모 디렉터리와
   `logs/runs/`를 만든다. activity-timeline의 실제 순서가 `create_dir_all` 후
   `db::init`이다(`apps/activity-timeline/src-tauri/src/lib.rs:21-24`).
2. `db::init`이 연결을 열고 `migrate`로 `jobs`, `runs`,
   `service_instances`, `notification_outbox`, `meta`를 보장한다. 기존
   앱의 `init`→`migrate` 패턴은 `apps/activity-timeline/src-tauri/src/core/db.rs:4-24`에
   있다. migration은 `occurrence_wall_key` unique claim, nullable `error_message`,
   nullable `log_dir`/`logs_deleted_at`, `starting`/owner/token/PID-group
   columns, `queue_sequence`/`blocked_by_run_id`, Windows process creation time,
   `jobs.last_evaluated_at`/`next_queue_sequence`, DPAPI `env_ciphertext`를 만든다.
3. second-instance plugin이 다른 daemon 실행을 기존 인스턴스로 전달했는지 확인한
   뒤, 이 daemon의 `owner_instance_id` UUID, DB 연결, per-job mutex map, 실행 중인
   job/service handle, shutdown sender, log root, 환경변수 protector를 앱 상태에
   등록한다. 이 시점의 `daemon_started_at`/system-local `startup_cutoff`은
   scheduler 전체에서 고정한다.
4. 이전 daemon에서 남은 `starting`, `stopping`, `running` run은 **절대
   respawn하지 않는다**. Windows Job Object handle은 anonymous라 startup에서
   recover/reopen하지 않으며, daemon crash 시 `KILL_ON_JOB_CLOSE`가 child cleanup을
   보장한다. startup은 저장된 `target_pid`/`target_process_created_at`으로 PID가
   사라졌는지 확인하고, PID가 다른 creation time으로 재사용됐으면 kill하지 않는다.
   동일 identity가 남은 예외 상황도 blind kill하지 않고 fail-safe 오류로 보고한다.
   WSL은 저장된 distro/PID/PGID/SID/marker를 exact validation하고
   session/group을 SIGTERM→timeout→SIGKILL로 정리하며, `/proc/<pid>/environ`
   marker가 없거나 PID가 재사용됐으면 무작정 kill하지 않고 실패로 기록한다.
   handshake 전에 PID가 NULL인 run도 distro의 `/proc/*/environ`에서 marker를 찾고
   group을 검증·정리한 뒤 `failed`로 끝낸다. `stopping` old run의 cleanup이
   성공하면 linked `blocked_by_run_id`를 NULL로 풀어 queued resume를 허용하고,
   실패하면 linked queued row를 `failed`로 CAS한다. unblocked `queued` run만 durable
   queue로 재개하며, 이 recovery는 같은 occurrence를 새로 insert하지 않는다.
   `service_instances`의 nonterminal generation도 같은 cleanup/no-respawn 규칙을
   따른다.
5. tray와 notification plugin/Rust notification adapter를 설치하고 백그라운드
   scheduler loop를 한 번 시작한다.
   폴러가 `tokio::time::interval`로 반복되는 기존 패턴
   (`apps/activity-timeline/src-tauri/src/commands/tracking.rs:47-51`)을 사용하되,
   매 tick에 cron을 외부 스케줄러로 전달하지 않고 DB transaction에서 active job의
   due occurrence를 계산·claim한다.

### job 정의와 미리보기

- 프론트의 `CronBuilder`는 프리셋 또는 직접 입력을 `cron_expr`로 만든다.
- Tauri command가 표현식을 검증하고 저장한다. 잘못된 표현식은 저장하지 않고
  사용자에게 입력 오류를 반환한다.
- `core::cron`은 기준 시각을 받아 다음 실행 시각 N개를 계산한다. 같은 함수가
  데몬의 due 판정과 UI 미리보기에 사용되어 화면과 실제 실행의 의미가 갈라지지
  않는다.
- 실행 대상, cwd, env를 함께 저장한다. WSL 대상은 distro를 별도 값으로 둔다.
  환경변수 값은 UI/API 조회에서 mask하고 DPAPI ciphertext로만 저장한다. Windows
  target은 `cmd.exe /D /S /C`로 실행하고, WSL target은 `wsl.exe -d <distro> --cd
  <wsl_path> -- setsid bash --noprofile --norc -lc`로 실행한다. WSL env는
  `Command::env`와 `WSLENV` names로 전달하며 script에 secret을 넣지 않는다.

### 예약 실행

1. scheduler loop가 `enabled = true`인 `kind = 'job'`을 조회한다.
2. scheduler loop는 1초 tick으로 동작한다. 각 job의 system local timezone 기준
   cron 표현식과 `last_evaluated_at` 이후 현재 시각까지의 due occurrence를 계산한다.
   시스템 시각이 뒤로 이동하면 checkpoint보다 과거의 occurrence를 다시 claim하지
   않고, 앞으로 이동하면 한 transaction에서 현재 시각까지 checkpoint를 전진시킨다.
3. `core::cron`이 먼저 system-local **naive wall tuple**을 cron field에 맞춰
   오름차순으로 명시적으로 열거한다. Croner는 표현식 syntax/field matching aid로만
   사용하며 timezone-aware next instant 또는 DST gap-shift candidate를 실행의
   source로 사용하지 않는다. 각 `NaiveDateTime`을
   `Local.from_local_datetime(&wall)`로 resolve해 `LocalResult::None`인
   nonexistent wall-clock 시각은 건너뛰고, `Single`은 사용하며, `Ambiguous`는
   earliest offset/earliest instant 하나로 canonicalize한다. `occurrence_wall_key`는
   원래 naive local date/time tuple에서 만들고 그 key로 dedupe한다. 따라서 fallback의
   같은 wall-clock 시각을 두 번 claim하지 않는다. 다음 실행 preview와 daemon이 같은
   explicit wall-tuple generator를 사용한다.
4. 앱이 꺼져 있던 동안 여러 시각이 지났다면 `occurrence <= startup_cutoff`인 gap에만
   `catch_up`을 적용한다. 켜진 job에서 **가장 마지막으로 놓친 한 번만** 실행
   대상으로 만들며 다섯 번을 연달아 실행하지 않는다. `catch_up`이 꺼져 있으면
   gap 후보는 실행 없이 checkpoint만 전진시킨다. cutoff 이후 steady due에는 이
   옵션을 적용하지 않는다.
5. 한 transaction에서 `(job_id, occurrence_wall_key)` unique claim을 시도한다.
   이미 claim된 occurrence면 이 tick은 아무 process도 만들지 않는다. 같은
   transaction이 `last_evaluated_at`을 전진시키므로 두 tick/daemon이 경쟁해도
   at-most-once claim이 유지된다.
6. active run이 없으면 `queued` row를 만든 뒤, process 시작 직전에
   `queued -> starting` affected-row CAS와 owner/attempt token을 얻는다. WSL은
   framed handshake로 PID/PGID/SID/marker를 검증한 뒤에만 `running`으로 전환한다.
   `skip`은 해당 occurrence를 `skipped` terminal row로 claim하고, `queue`는
   `queued` row를 DB에 남겨 process가 끝난 뒤 FIFO로 시작한다. queue row는 앱
   재시작 뒤에도 보존한다. `kill-previous`는 per-job async mutex 아래에서 DB
   transaction 밖 terminate·wait를 수행하고, 실제 종료 확인 후에만 old
   `stopping -> cancelled`와 새 `starting`을 CAS한다. 실패하면 new spawn 없이
   affected rows를 `failed`로 terminal 처리한다.
7. run ID로 app-local `logs/runs/<run_id>/` 디렉터리를 만들고 stdout/stderr의
   첫 segment와 manifest를 먼저 연다. 파일 생성 또는 DPAPI 복호화가 실패하면 process를
   만들지 않고 `status = failed`, `exit_code = NULL`, `error_message`를 기록한다.
   파일이 준비된 뒤 Windows custom CreateProcessW adapter는 `cmd.exe /D /S /C
   <command>`를 suspended Job Object에 넣고 resume한다. WSL adapter는
   `wsl.exe -d <distro> --cd <wsl_path> -- setsid bash --noprofile --norc -lc
   <wrapper+command>`를 spawn하며 env 값은 `Command::env`/`WSLENV`로만 전달한다.
   stdout의 어느 위치에서든 framed handshake를 파싱해 PID/PGID/SID/marker를 exact
   validation한 뒤 `target_pid`, `target_pgid`, `target_sid`, `process_marker`를
   owner/attempt token과 함께 저장한다. 저장·검증에 실패하면 해당 Job Object 또는
   WSL process group을 terminate→wait/timeout→kill하고 실제 종료를 확인한 뒤
   run을 `failed`로 남긴다.
8. process 종료 시 `ended_at`, exit code, `succeeded`/`failed`/`cancelled` 상태와
   비밀값 없는 오류를 갱신한다. non-zero exit code와 로그 write failure는 `failed`,
   kill-previous와 orderly shutdown은 실제 process/group 종료가 확인된 경우에만
   `cancelled`다. terminate timeout은 `failed`와 고정 오류 코드로 남긴다. 실패하면
   각 `failed` 상태 전환에 sanitized `run-failed` event를 만들고 Rust-side
   notification adapter가 Windows toast를 직접 시도하거나 sanitized outbox에
   적재한다. notification permission/API 오류는 scheduler를 중단시키지 않는다.
9. `tail_log(run_id, stream, cursor, max_bytes)` command는 `stream`을 `stdout` 또는
   `stderr`로 제한하고 `runs.log_dir`를 log root 아래에서 canonicalize해 해당
   stream의 고정 파일만 읽는다. 사용자 입력 절대 경로, `..`, 다른 run 경로는
   거부한다. cursor는 JS number가 아닌 lossless decimal string인 단조 증가 logical
   byte offset이다. 응답은 `data`, `retainedStartOffset`, `nextCursor`,
   `truncated`를 포함한다. 회전 전후의 모든 파일을 oldest-to-newest logical order로
   읽고, cursor가 retained start보다 작으면 그 start에서 재개하며
   `truncated = true`로 표시한다. 한 번 읽기 상한은 256 KiB로 clamp하고,
   stream별 async lock과 snapshot end offset 아래에서 읽어 rotation 중 skip/duplicate를
   만들지 않는다. 한 stream은 10 MB 파일을 5개까지 순환한다.
10. run 종료 뒤 job별 최근 50개 **terminal run metadata row**는 보존한다.
   `queued`/`starting`/`running`/`stopping` active row는 retention 대상에서
   제외한다. 모든 job의 terminal log 총량이 200 MB를 넘으면 오래된 terminal run의
   log files와 log reference(`log_dir`)만 먼저 제거하고 row는 유지한다. job별
   최근 50개를 초과한 terminal row는 log 삭제가 성공하거나 이미 없는 것으로
   확인된 뒤에만 row를 삭제한다. 파일 삭제 실패 시 row를 보존하고 다음
   startup/cleanup tick에서 재시도한다. DB 삭제 후 파일만 남는 crash에 대비해
   startup에서 app-generated log directory와 `runs`를 대조해 orphan directory를
   회수하고, reference가 없는 row는 `logs_deleted_at`으로 idempotently 표시한다.

### 수동 실행과 이력

"지금 실행" command는 cron 계산과 scheduled claim을 거치지 않고
`scheduled_at = NULL`인 run을 같은 실행 어댑터와 overlap 정책으로 만든다. 자동
실행과 수동 실행이 다른 process 생성 경로를 가지면 로그·종료·중복 정책이 달라지므로
경로를 공유한다. 이력 화면은 `runs`에서 잡별 최근 50회 또는 기간 범위를 조회하고
시작 시각으로 정렬한다. activity-timeline의 범위 조건과 `ORDER BY start_ts` 쿼리
(`apps/activity-timeline/src-tauri/src/core/db.rs:41-63`)를 동일한 조회 원칙으로
적용한다.

### Phase 2 서비스

서비스는 cron tick마다 새 프로세스를 만드는 대신 명시적인 start/stop/restart
명령과 데몬 기동 시 `auto_start`를 처리한다. 실제 active instance claim은
`service_instances(job_id PRIMARY KEY, generation, state, active_run_id, owner_instance_id,
attempt_token, next_retry_at, consecutive_failures)`에서 transaction으로 수행한다.
Phase 1 adapter가 확립한 Job Object 또는 WSL session/group handle을 그대로 사용한다.

- Windows 서비스 프로세스는 Phase 1과 같은 per-instance Job Object에 넣고
  `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`를 설정한다. stop/restart는 Job을 종료하고
  process handle wait로 descendants가 실제 사라졌음을 확인한 뒤에만 old run을
  `cancelled`로 만든다. `npm start`만 종료하면 그 아래 node가 남고 `docker run`도
  자식이 남을 수 있으므로 단일 PID kill로는 충분하지 않다.
- WSL 서비스는 Windows의 `wsl.exe` PID를 kill하는 방식으로 처리하지 않는다.
  실행 시 `setsid bash --noprofile --norc -lc` wrapper가 초기 environment marker와
  framed PID/PGID/SID handshake를 내고 `exec`한다. stop/restart는 marker·PID·PGID·SID
  exact validation 후 `wsl.exe -d <distro> -- kill -TERM -- -<pgid>`,
  timeout, `wsl.exe -d <distro> -- kill -KILL -- -<pgid>`를 순서대로 보내고
  `kill -0 -- -<pgid>`와 `/proc` 검사로 실제 group 소멸을 확인한다. Windows Job
  Object와 WSL session/group은 서로
  대체할 수 없는 별도 메커니즘이다.
- 헬스체크는 **프로세스 생존 AND** optional TCP probe다. process exit는 start grace
  중에도 즉시 failure로 처리한다. TCP 설정이 NULL이면 liveness만 요구하고,
  설정되어 있으면 두 조건이 모두 성공해야 healthy다. Windows target은 Windows host
  adapter에서 local address/port를 probe하고, WSL target은 Windows host
  `netstat` 결과를 대신 쓰지 않고 해당 distro 내부에서 local TCP probe를 실행한다.
  포트 판정에 필요한 순수 parser·sysinfo primitive는 `crates/process`를 사용한다.
  헬스체크는 10초 간격, 각 TCP probe 3초 timeout, 3회 연속 실패를 unhealthy로
  판정한다. 성공 probe는 consecutive failure와 backoff 단계를 reset한다.
- start grace의 정확한 수치와 포트 설정 UI 형식은 `미결`에 둔다. grace가 끝나기
  전에 process가 exit하면 즉시 failure이며, grace 중 TCP failure만 유예한다.
  health 결과와 retry timer는 `job_id + generation + attempt_token` CAS를 사용하므로
  stop/restart 뒤 늦은 failure가 새 instance를 resurrect하지 않는다.
- 실패 시 `never`, `on-failure`, `always`와 고정된 exponential backoff
  `1/2/4/8/16/30초`를 사용하며 30초를 상한으로 한다. 정상 기동 또는 정상
  health probe 뒤 backoff 단계는 1초로 reset한다. 재시도 횟수는
  `on-failure`/`always` 정책의 의미에 따라 제한하지 않으며, stop/daemon shutdown은
  pending retry를 transaction으로 취소한다.

## 상태 저장

`app_local_data_dir()`는 번들 identifier 기준 폴더를 반환하며, 저장 위치 규약은
`%LOCALAPPDATA%\\{identifier}\\data.db`다(`CONVENTIONS.md:107-115`). run-manager는
identifier를 기존 kebab-case 앱의 기계적 변환인 `com.workbench.runmanager`로
사용하고, 그 폴더에 `data.db`와 로그 디렉터리를 둔다. 정확한 로그 하위 디렉터리
이름은 `logs/runs/<run_id>/`로 고정한다. 각 run directory는 앱이 생성한 UUID run ID
로만 만들고, `runs.log_dir`에는 이 root 기준 상대 경로만 저장한다.

code-pad와 달리 JSON 단일 파일을 쓰지 않는 이유는 run-manager 데이터가 평평한
설정이 아니라 쿼리 대상이기 때문이다. code-pad는 세션 상태를 `session.json`에
저장한다(`docs/superpowers/specs/2026-08-12-code-pad-design.md:255-256`). 그 선택은
조건 검색이 없는 세션·설정 상태에 맞지만, run-manager는 최근 50회·기간 필터·정렬·
보관 정책을 항상 수행한다. activity-timeline은 SQLite `sessions` 테이블과 시작
시각 인덱스를 생성한다(`apps/activity-timeline/src-tauri/src/core/db.rs:11-24`)는
동일한 선택의 선례다.

### 로그 저장

- 실행마다 app-local `logs/runs/<run_id>/` 아래 stdout/stderr segment files를
  각각 stream별 10 MB 단위로 최대 5개까지 회전한다. DB에는 본문을 넣지 않고
  `log_dir` 상대 identifier만 기록한다. 각 stream의 lossless cursor manifest는
  정확히 `logs/runs/<run_id>/<stream>.manifest.json`에 둔다. manifest schema에는
  `schema_version`, `run_id`, `stream`, `retained_start_offset`, `next_offset`,
  `rotation_generation`, 그리고 logical start/end offset·byte length·checksum을
  가진 retained segment filename 목록이 포함된다. segment filename 자체도
  `stdout.g<generation>.o<start>-<end>.log`/stderr 대응 형식으로 generation과
  logical range를 encode해 manifest 없이도 재구성할 수 있어야 한다. 같은 디렉터리의
  `<stream>.manifest.json.tmp`에 schema/checksum을 완성해 flush한 뒤 atomic
  rename하고, reader는 checksum/schema/run/stream을 검증한다. crash로 manifest가
  없거나 손상되면 파일명·segment metadata와 길이를 검사해 logical offsets를
  재구성하고 새 manifest를 atomic하게 기록한다. DB migration은 manifest column을
  만들지 않는다.
- UI tail command는 `run_id`, `stream ∈ {stdout, stderr}`, optional decimal
  `cursor`와 `max_bytes`를 받는다. cursor는 파일 offset이 아니라 해당 stream이
  시작한 뒤 모든 byte에 부여한 logical offset의 decimal string이며, JS number로
  변환하지 않는다. 응답은 data, `retainedStartOffset`, `nextCursor`,
  `truncated`를 포함한다. cursor가 보존 시작보다 앞서면 보존 시작부터 반환하고
  truncated를 true로 한다. cursor가 NULL이면 최신 retained 범위에서 시작한다.
  `nextCursor`는 반환한 byte 바로 다음 offset이므로 client가 그대로 재요청해도
  중복/누락이 없다. rotation 전후 파일을 logical oldest-to-newest 순으로 읽고,
  stream lock 아래에서 snapshot end offset을 고정한 뒤 읽는다. max_bytes는
  256 KiB로 clamp한다.
  `log_dir`를 canonicalize한 뒤 log root 하위인지 확인하고, 경로 탈출·절대 경로·
  다른 run ID는 거부한다.
- DB에 출력 본문을 넣지 않으므로 매분 실행되는 job이 큰 출력으로 이력 조회를
  느리게 만들지 않는다.
- 보관은 job별 최근 50개 terminal metadata row와 전체 200 MB log cap을 함께
  적용한다. 200 MB cap은 active row를 건드리지 않고 terminal run의 stream files와
  `log_dir` reference만 오래된 순서로 제거하며 row는 유지한다. 최근 50개를 초과한
  terminal row는 log files/reference 삭제가 성공하거나 `NotFound`로 확인된 뒤에만
  row를 삭제한다. log 삭제 실패는 row를 남겨 startup/cleanup tick에서 재시도한다.
  삭제·manifest 갱신은 idempotent해야 하며, DB row만 남은 경우
  `logs_deleted_at`을 채우고, 파일만 남은 경우 app-generated run ID와 DB를 대조해
  orphan을 회수한다.

### 로그 검색 (#311, v0.5.0 P2-13 구현 계약)

검색은 선택한 하나의 run에 대해 기존 `tail_log`의 app-owned 회전 파일을 bounded
snapshot으로 읽은 뒤, 파일·DB·process writer와 분리된 순수 core에서 판정한다. 한 번에
전체 파일을 열어 writer를 오래 기다리지 않고 256 KiB cursor chunk마다 async yield한다.
rotation으로 cursor가 stale해진 경우 현재 retained boundary에서 최대 한 번 재시작하며,
그 이후의 결과는 `truncated`로 표시한다. 로그 본문은 SQLite·snapshot·telemetry·remote
archive로 복제하지 않는다.

요청 DTO는 `runId`, `query`, `mode`, optional `source`, `level`, `startAt`, `endAt`이다.
`mode=literal`이 기본 UI 선택이며 query를 정규식으로 해석하지 않는다. `mode=regex`를
명시한 경우에만 Rust `regex` 엔진을 사용하고 compile/automata budget을 적용한다. source는
임의 path나 remote source가 아니라 이 run의 `stdout`/`stderr` stream adapter이며, level은
줄 시작의 trace/debug/info/warn/error 토큰을 best-effort로 판정한다. RFC3339 timestamp가
줄 앞에 있으면 이를 사용하고 없으면 run 시작 시각을 사용한다. 시간 필터는 epoch
milliseconds 반열린 구간 `[startAt, endAt)`이며 JSON/WebView 왕복에서 정밀도를 잃지 않는
JavaScript safe integer 범위만 허용한다. request DTO는 unknown field를 거부한다.

응답은 검색 원문을 포함하지 않고 `sourceId`, `stream`, 보존 snapshot 기준 1-based
`lineNumber`, 판정된 `level`/`timestampMillis`와 `scannedLines`, `scannedBytes`,
`truncated`, `sources`만 반환한다. 따라서 frontend는 현재 log viewer의 줄을 stream·line
기준으로 선택하고 이전/다음 결과로 이동한다. 보존 rotation 또는 화면 1 MiB cap 때문에
해당 줄이 현재 DOM에 없으면 메타데이터만 유지하고 안전한 안내를 표시한다.

검증 상한은 query/regex UTF-8 512 bytes, source당 scan 4 MiB, 전체 scan 8 MiB, record
16 KiB, 전체 50,000 records, 결과 500개이다. invalid query/time/source와 regex compile
실패는 raw query, line, path, credential을 반향하지 않는 고정 command 오류 코드로
변환한다. `log-source/v1` reference는 `run-manager:<opaque-run-id>:<stream>` identity와
kind를 local boundary에서 exact 검증하며 absolute path·command·environment·secret은
payload에 없고 unknown field도 거부한다. retained segment metadata 복원과 bounded core
scan은 blocking worker로 옮겨 async command executor를 점유하지 않는다. Log Lens
receiver/producer handoff와 remote/permanent log archive는 이 PR에 포함하지 않고 Log Lens
bootstrap 뒤 별도 integration PR로 남긴다.

### 스키마 버전

초기 migration에서 `meta(schema_version)`를 생성한다. everything-plus의
`migrate()`가 기존 `meta` 값을 읽어 낮은 버전이면 `clear_all` 후 버전을 갱신하는
방식(`apps/everything-plus/src-tauri/src/core/db.rs:108-128`)을 참고하되, run-manager의
job 정의와 실행 이력은 파생 인덱스인지 여부가 다르므로 실제 버전 상승 시
데이터 보존·변환 규칙은 해당 migration PR에서 작성한다.

## 에러 처리

확정된 사용자-visible 동작과 구현에서 아직 수치가 정해지지 않은 항목을 분리한다.

| 상황 | 처리 |
|---|---|
| DB 디렉터리 생성·열기·migration 실패 | 시작을 완료하지 않고 사용자에게 오류를 반환한다. activity-timeline의 `?` 전파 초기화(`apps/activity-timeline/src-tauri/src/lib.rs:21-24`)와 같은 실패 경계다 |
| cron 표현식이 잘못됨 | 저장을 거부하고 builder에 입력 오류를 표시한다. 실행 가능한 표현식만 daemon에 들어간다 |
| 비활성 job | scheduler 대상에서 제외한다. 수동 실행은 사용자가 명시적으로 요청한 실행이므로 같은 command 경로에서 별도 처리한다 |
| 대상 실행 실패 | `failed` run에 비밀값 없는 `error_message`를 저장하고 UI에 전달한다. process가 시작되지 않아 exit code가 없는 경우에도 `exit_code = NULL`로 row를 남긴다 |
| 프로세스가 0이 아닌 exit code로 종료 | `failed`로 표시하고 exit code와 `log_dir`를 이력에 남긴다 |
| 이미 실행 중인 job | `skip` / `queue` / `kill-previous` 중 잡 설정을 적용한다 |
| WSL distro가 없거나 `wsl.exe` 호출 실패 | 해당 실행을 `failed`로 처리하고 redacted stderr와 고정된 오류 코드를 저장한다. distro는 저장 시 목록 검증하고 실행 직전 다시 확인한다 |
| stdout/stderr 로그 파일 쓰기 실패 | process를 시작하지 않고 `failed`로 기록한다. 이미 실행 중 write failure는 adapter handle을 terminate하고 child wait·log flush 뒤 run을 `failed`로 종료하며 cleanup에서 재시도한다 |
| 실패한 job | 비밀값 없는 `run-failed` event를 Rust-side notification adapter가 처리하고 Windows toast를 시도한다. hidden webview에 의존하지 않으며 notification permission/API 실패가 scheduler 전체를 중단시키지 않도록 outbox/nonfatal 경계로 분리한다 |
| scheduler 한 job의 계산·실행 오류 | 해당 job의 `failed` run과 checkpoint를 기록하고 다음 job의 1초 tick은 계속 처리한다 |
| Phase 1 Windows 실행 취소/종료 | Job Object에 terminate를 요청하고 process handle wait로 tree가 실제 사라졌음을 확인한다. 확인 뒤에만 run을 `cancelled`로 기록하며 timeout이면 `failed/termination_timeout`이다 |
| Phase 1 WSL 실행 취소/종료 | 저장한 distro/PID/PGID/SID/marker를 exact 검증한 뒤 process group에 SIGTERM→timeout→SIGKILL을 보내고 실제 group 소멸을 확인한다. 확인 뒤에만 `cancelled`로 기록하며 PID 재사용/검증 실패는 kill하지 않는다 |
| 데몬 종료·Tauri ExitRequested·Windows logoff/shutdown | scheduler와 pending retry를 먼저 멈추고 queue는 보존한다. active handle을 orderly terminate, 제한 시간 후 강제 escalation, wait, 로그 flush, terminal CAS 순으로 처리한 뒤에만 exit를 허용한다 |
| stale `starting`/`stopping`/`running` run | startup에서 절대 respawn하지 않는다. Windows Job cleanup 또는 WSL marker/group cleanup과 실제 종료를 확인한 뒤 ambiguous run을 `failed`로 terminal 처리한다. 오직 `queued`만 durable resume한다 |
| 환경변수 DPAPI 오류 | 평문 fallback 없이 `failed` run을 기록하고 해당 process를 시작하지 않는다 |
| 로그 검색 query/source/time/regex 오류 | 고정 `log-search-*` 오류 코드만 반환하고 query·line·경로·credential을 반향하지 않는다. scan은 source/전체 byte·record·result 상한 안에서만 수행하며, stale cursor와 상한 도달은 `truncated` 결과로 표시한다 |

`CREATE_NO_WINDOW`는 오류를 숨기는 수단이 아니라 콘솔 창 생성을 막는 실행 옵션이다.
life-log가 Windows 조건부로만 이 플래그를 호출한다(`apps/life-log/src-tauri/src/core/aggregate.rs:37-39`)는
선례처럼 플랫폼 계층에서만 적용한다.

## 테스트

### Rust 순수 로직

`core/`의 OS 비의존 함수에 다음 테스트를 먼저 둔다.

- cron 표현식의 유효성, 기준 시각 이후 다음 실행 시각 계산, 다음 실행 시각 N개
  생성. 기본 optional seconds(5-field 입력은 초 `0`으로 정규화), POSIX
  DOM-OR-DOW(예: `0 0 13 * 5`는 13일 또는 금요일), 하루 여러 시각을 포함한다.
  `@reboot`은 croner 3.0.1에서 지원하지 않으므로 command validation에서 명시적으로
  거부한다.
- `occurrence <= startup_cutoff`인 gap에서만 마지막 한 번을 반환하는 catch-up
  판정과, `occurrence > startup_cutoff`인 steady due는 catch_up false여도 모두
  반환하는 판정. cutoff와 occurrence가 정확히 같은 equality 경계도 gap으로
  처리하는지, 1초 tick이 지연되어 여러 steady occurrence가 쌓이는 경우도 검증한다.
- 중복 실행 정책 `skip`, `queue`, `kill-previous`의 판정.
- `kill-previous` old cleanup 성공 전 linked queued row가 dequeue되지 않고,
  실패 시 old와 linked row가 모두 failed가 되는지, queue sequence FIFO를 검증한다.
- 로그 보관 계산: active row를 제외한 job별 최근 50개 terminal metadata를 보존하고,
  전체 200MB 초과 시 terminal log files/reference만 제거하며 row를 유지한다. 50개
  초과 terminal row는 log 삭제 성공 뒤에만 삭제한다. 두 상한이 각각 다른 상황
  (매분 job/하루 한 번 job)을 모두 테스트한다.
- occurrence claim의 unique constraint와 transaction을 두 scheduler tick/두 app
  instance가 경쟁해도 한 row와 한 process만 만드는지 테스트한다.
  `queued -> starting` affected-row CAS, owner_instance_id/attempt_token mismatch,
  crash 중 starting/stopping no-respawn와 queued-only resume를 함께 검증한다.
  수동 run의 `scheduled_at = NULL`/wall key NULL은 여러 번 저장되는지 검증한다.
- explicit naive wall-tuple generator가 cron field에 맞는 gap/overlap fixture를 만들고,
  `Local.from_local_datetime`의 `None` discard와 `Ambiguous` earliest-offset
  canonicalization, wall-key dedupe를 `chrono-tz` deterministic timezone으로
  테스트한다. croner가 반환하는 gap-shifted instant는 실행 source로 사용하지 않는다.
- `core::shell`의 NUL 거부와 Windows `cmd.exe /D /S /C` command-line contract,
  WSL `Command::env`/WSLENV `<KEY>/w` names 및 `setsid bash --noprofile --norc -lc` argv가
  env secret을 bash args에 넣지 않는지 검증한다. 기존 WSLENV의 managed key에
  conflicting direction flag가 있거나 duplicate key가 있어도 제거 후 정확한
  `<KEY>/w`로 재생성되고, reserved `WSLENV`/marker 입력이 거부되며 unrelated
  flag만 보존되는지도 검증한다. WSL framed handshake가 stdout
  noise 어느 위치에서도 파싱되고 `/proc/<pid>/environ` marker와 PGID/SID를 exact
  check하는지, group TERM→timeout→KILL이 실제 descendants 종료를 확인하는지
  adapter integration fixture로 테스트한다. Windows integration은 custom
  CreateProcessW suspended→AssignProcessToJobObject→ResumeThread 순서, 모든 error
  cleanup, no-fallback 계약을 검증한다.
- tail cursor의 decimal logical offset, retainedStartOffset/nextCursor/truncated,
  rotation 중 snapshot과 256 KiB cap을 테스트한다. DPAPI는 Windows integration test와
  Linux fake protector test로 Keep/Set/Clear, plaintext fallback 없음, old-secret
  per-run snapshot, empty/overlap/chunk-boundary secret redaction과 zeroize
  best-effort를 검증하고, error/toast/commandline에 plaintext가 없음을 확인한다.
- 로그 검색은 literal 우선과 명시적 regex mode, invalid pattern 고정 오류, level/source/time
  filter, stream·보존 line navigation metadata, malformed/oversized record, scan/result
  cap, deterministic source order, nested-quantifier regex, stale cursor와 running-writer
  chunk fixture를 검증한다. frontend는 Enter/IME composition, busy 중복 제출, stale
  async response, unmount, clear, keyboard/a11y navigation을 검증한다.
- `service_instances` generation/state claim, process liveness AND optional TCP
  semantics, immediate exit, 10s/3s/3 failures, fixed backoff, stale health/retry
  cancellation을 테스트한다.

실행·WSL·Windows API를 순수 core 테스트에 넣지 않는다. activity-timeline도
폴러 loop에서 OS 호출(`apps/activity-timeline/src-tauri/src/commands/tracking.rs:56-66`)을
수행하지만 DB 조회·저장과 세션 병합을 분리한다. run-manager도 정책 계산과
플랫폼 실행을 분리하고 adapter/platform은 별도 Windows/WSL integration test로
검증한다. 검증 명령은 `cargo test`다.

### 프론트

- vitest로 cron builder의 프리셋·직접 입력이 올바른 표현식을 만드는지 테스트한다.
- vitest로 실행 이력의 시작/종료 시각, 성공·실패, exit code, 로그 tail 링크의
  표시 포맷을 테스트한다. `error_message`와 `scheduled_at = NULL` 수동 run,
  stdout/stderr stream 선택 및 256 KB tail clamp도 테스트한다.

저장소 루트는 이미 `vitest`, Testing Library, jsdom을 devDependency로 두고 있다
(`package.json:11-15`), 프론트 전체 테스트 script도 `pnpm -r test`다
(`package.json:6-10`). CI의 `frontend` job은 `pnpm build`와 `pnpm test`를 모두
실행한다(`.github/workflows/ci.yml:13-40`). 따라서 run-manager의 builder·이력
테스트에는 별도 CI job을 만들지 않는다.

### Windows CI와 수동 검증

CI는 세 잡을 모두 통과해야 한다.

- `frontend`: `pnpm install --frozen-lockfile`, build, test, 타입 검사
  (`.github/workflows/ci.yml:14-40`)
- `rust`: Linux에서 workspace check, clippy, fmt, test
  (`.github/workflows/ci.yml:41-74`)
- `rust-windows`: Windows에서 workspace check, clippy, test
  (`.github/workflows/ci.yml:76-106`)

특히 `rust-windows`가 필요한 이유는 CI 주석이 `#[cfg(target_os = "windows")]`
코드가 Windows에서만 실제 컴파일된다고 명시하기 때문이다
(`.github/workflows/ci.yml:82-83`). Job Object, `CREATE_NO_WINDOW`, Windows 실행
어댑터가 저장소에서 가장 많이 들어가는 앱이므로 이 잡의 가치가 가장 크다.

Windows 수동 검증은 다음 순서로 한다.

1. `npm start`를 service로 띄운다. stop 뒤 node와 그 자식 프로세스가 남지 않는지
   확인한다.
2. WSL service를 띄운다. stop 뒤 저장한 PID가 아니라 exact marker/PGID/SID
   session의 Linux descendants가 TERM→timeout→KILL 뒤 모두 죽는지 확인한다.
3. 앱을 종료하고 startup cutoff 전후의 cron 시각을 지나 다시 켠다.
   `occurrence <= startup_cutoff`인 equality 포함 gap에서 catch-up이 켜진 job은
   한 번만, 끈 job은 실행하지 않으며, `occurrence > startup_cutoff`인 steady due는
   두 설정 모두 실행되는지 확인한다.
4. 매분 job과 하루 한 번 job을 각각 실행해 stream별 10 MB x5 회전, decimal
   logical cursor/256 KiB tail, 최근 50회 metadata·전체 200MB log reference 보관이
   함께 적용되고 UI tail이 회전 파일을 lossless하게 읽는지 확인한다.

완료 정의는 저장소 규칙과 동일하게 `cargo test` + `cargo check` + `pnpm build`다
(`AGENTS.md:16-29`). 이 설계 커밋에서는 앱을 구현하지 않으므로 위 검증은 구현
PR의 독립 단계로 실행한다.

## 범위 밖

- **의존성 그래프**: A가 끝난 뒤 B를 실행하는 조건부 DAG는 cron job의 중복·실패
  정책과 다른 기능이다.
- **원격 호스트**: Windows와 로컬 WSL 배포판만 실행 대상으로 한다.
- **컨테이너 오케스트레이션**: `docker run`을 하나의 로컬 service 명령으로
  관리할 수는 있지만 여러 컨테이너의 배포·네트워크·스케일링은 다루지 않는다.
- **외부 알림 채널**: 실패 시 `tauri-plugin-notification` Windows toast까지만
  제공하고 Slack 등은 넣지 않는다.
- **결과 기반 트리거**: 한 job의 exit code나 출력이 다른 job을 시작하는 조건은
  넣지 않는다.
- **외부 스케줄러 동기화**: Task Scheduler 태스크와 WSL crontab을 읽거나 쓰거나
  양방향 동기화하지 않는다.
- **Phase 1 service**: Phase 1에서는 service 행을 저장하거나 실행하지 않고,
  service 제어는 Phase 2로 미룬다.
- **DB 내 로그 본문**: 로그는 파일과 tail로만 다루고 SQLite에는 메타데이터만 둔다.
- **고아 서비스 재연결**: 데몬을 다시 켰을 때 이전 프로세스를 찾아 붙이는 기능은
  넣지 않는다. 데몬 종료 시 Phase 2 서비스도 Job Object 또는 WSL
  marker/session/group으로 정리하며, crash 뒤 nonterminal run/service를 respawn하지
  않는다.

## 미결

브리핑에서 동작 원칙과 안전 경계는 확정했지만 다음 UX·저위험 구현 값은 정해지지
않았다. 이 문서에서 임의로 채우지 않고 구현 PR의 결정 사항으로 남긴다.

- 다음 실행 시각 미리보기의 기본 N과 builder가 제공할 정확한 프리셋 목록
- 동시에 실행할 수 있는 job 수, SQLite busy timeout과 취소 요청의 UI 동작
- service 포트 헬스체크의 설정 UI 형식(생존 확인·10초/3초/3회 정책 자체는 확정)
- service process 시작 grace의 정확한 수치와 UI 표현(즉시 process exit는 grace와
  무관하게 failure)
- startup shortcut의 파일명·아이콘·인자
- Windows toast의 문구·중복 억제·보관 방식
- SQLite의 비어 있는 선택 컬럼 기본값과 이후 migration에서의 데이터 변환 세부

## 확정된 세부 결정

### 1. 놓친 실행은 마지막 한 번만 따라잡는다

앱이 꺼져 있는 동안 같은 job의 cron 시각이 다섯 번 지났어도
`occurrence <= startup_cutoff`인 gap에서는 다시 켤 때 한 번만 실행한다.
백업·동기화처럼 멱등인 작업에 맞는 동작이며,
며칠 뒤 수십 실행이 한꺼번에 몰리는 전부 따라잡기 방식은 채택하지 않는다. 이
gap 정책은 job별 `catch_up` 옵션으로 끌 수 있다. cutoff 이후 steady-state due는
`catch_up` 값과 무관하게 실행한다.

### 2. 자동 시작은 시작프로그램 폴더 바로가기로 한다

경로는 `%APPDATA%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup`이다. 관리자
권한이 필요 없고 사용자가 파일 탐색기에서 직접 확인·삭제할 수 있다. Task Scheduler의
ONLOGON 태스크도 가능하지만, 외부 스케줄러를 사용하지 않기로 한 앱이 자기 자동
등록에만 Task Scheduler를 쓰면 설계 원칙과 맞지 않으므로 사용하지 않는다.

### 3. 데몬 종료 시 모든 run과 service를 종료한다

Phase 1 job과 Phase 2 service 모두 데몬과 독립적으로 남아 있지 않는다. Windows에서는
각 cancellable run이 자기 Job Object에 프로세스 트리를 넣고
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`를 적용한다. orderly Quit, Tauri
`ExitRequested`, Windows logoff/shutdown은 single-instance가 소유한 scheduler와
retry를 먼저 멈추고 pending queue를 보존한 뒤 각 handle에 terminate/wait를
요청한다. 제한 시간에는 Job terminate/close 또는 WSL group SIGKILL로 escalate하고,
실제 종료·로그 flush·terminal 상태 갱신을 기다려 `app.exit(0)`를 허용한다.
다음 기동 때 고아 프로세스를 찾아 재연결하거나 ambiguous run을 respawn하지 않는다.

WSL은 별개다. Windows 쪽 `wsl.exe`를 kill해도 Linux 내부 process tree가 안전하게
종료된다고 볼 수 없으므로, exact marker로 검증한 PID/PGID/SID를 받아 session/group에
SIGTERM→timeout→SIGKILL을 보내고 소멸을 확인한다. Job Object와 WSL group cleanup을
하나의 추상화로 합치지 않지만, Phase 2는 Phase 1 adapter의 두 handle을 그대로
재사용한다.

### 4. 로그 보관은 두 상한을 동시에 적용한다

각 run의 stdout/stderr는 stream별 10 MB 파일을 최대 5개까지 회전하고, tail API의
한 번 읽기 상한은 256 KiB다. tail은 lossless decimal logical cursor와
retainedStartOffset/nextCursor/truncated를 사용해 rotation across files를
지원한다. 잡마다 최근 50개 terminal metadata row를 유지하고, 모든 잡의 로그
합계가 200MB를 넘으면 terminal log files/reference만 오래된 실행부터 삭제하며
row는 유지한다. 최근 50개를 초과한 terminal row는 log 삭제 성공 뒤에만 삭제하고,
active row는 제외한다. DB에는 로그 본문이 아닌 앱이 생성한 상대 log identifier와
metadata만 남긴다.

### 5. 시간대와 occurrence 보장

스케줄러 tick은 1초이고 system local timezone을 사용한다. explicit naive wall-tuple
generator가 만든 값을 `Local.from_local_datetime`으로 resolve해 DST nonexistent
wall-clock 시각은 건너뛰고, ambiguous 시각은 earliest offset과 wall-key 하나로
canonicalize해 한 occurrence로만 claim한다. `jobs.last_evaluated_at` checkpoint 전진과
`UNIQUE(job_id, occurrence_wall_key)` insert는 한 SQLite transaction에서 수행하며,
수동 run의 `scheduled_at`/wall key는 NULL이다. `@reboot`은 croner 3.0.1이 지원하지
않으므로 저장을 거부한다.

### 6. 실행·비밀값 경계

Windows는 `cmd.exe /D /S /C`를 custom CreateProcessW suspended→Job
assign→ResumeThread 경계로 사용하고, WSL은 `wsl.exe -d <distro> --cd <wsl_path> --
setsid bash --noprofile --norc -lc`를 사용한다. WSL env values는
`Command::env`/WSLENV names로만 전달하고 bash args/script에 넣지 않는다.
환경변수는 Windows DPAPI CurrentUser ciphertext로만 저장하고 UI/API/log/toast에는
값을 노출하지 않는다. 실패 알림은 Rust-side `tauri-plugin-notification` Windows
toast를 직접 시도하거나 sanitized durable outbox에 보관하며 capability의
`notification:default` permission을 사용한다.

## 구현 순서

각 단계는 독립적으로 검증할 수 있어야 한다. 기능 단위 1개를 PR 1개로 분리하라는
저장소 규칙(`AGENTS.md:24-29`)을 따른다.

### 구현 전제 — 이미 충족된 공통 crate

`crates/process` 추출은 `origin/main`의 `75b8af2`에서 완료되었다. run-manager 구현
PR은 package `process`를 `devbox_process`로 import하고, `PortInfo`,
`parse_netstat_output`, `extract_port`, `ProcessId`, `ProcessSnapshot`,
`lookup_process`, `is_process_alive`를 현재 API 그대로 사용한다. Windows `netstat`
실행, Job Object, WSL PID/PGID/SID session/group 호출은 여전히 소비자의 플랫폼
계층에 남긴다. 이 공통 crate에 새 Windows 수명 API를 추가하는 별도 선행 작업은
필요하지 않다.

### Phase 1 — job

1. **앱 스캐폴드·생명주기**: CONVENTIONS의 Tauri 앱 생성 절차
   (`CONVENTIONS.md:175-193`)로 `apps/run-manager`를 만들고, 이름·identifier·root
   Cargo workspace 멤버를 맞춘다. single-instance plugin을 가장 먼저 등록하고,
   notification plugin과 `notification:default` capability, DB 초기화, 상태 등록,
   tray, 창 숨김, 1초 scheduler 종료 신호를 연결한다. `cargo check`와 `pnpm build`로
   빈 생명주기를 검증한다.
2. **SQLite 기반**: `jobs`, `runs`, `service_instances`,
   `notification_outbox`, `meta(schema_version)` migration과 serde 모델을 작성한다.
   `runs.scheduled_at`/`occurrence_wall_key` nullable column,
   `UNIQUE(job_id, occurrence_wall_key)`, `jobs.last_evaluated_at`,
   `jobs.next_queue_sequence`, `queue_sequence`/`blocked_by_run_id`,
   nullable `error_message`/`log_dir`, `starting`/`stopping` states,
   owner_instance_id/attempt_token, `target_pid`/`target_process_created_at`/
   `target_pgid`/`target_sid`/
   `process_marker`, `logs_deleted_at`, `env_ciphertext`를 포함한다.
   잡 CRUD와 최근/기간 이력 쿼리를 붙이고 in-memory SQLite 테스트를 통과시킨다.
   로그 본문은 이 단계에도 저장하지 않는다.
3. **cron core**: `core/cron.rs`에 `croner = "3.0.1"`과 직접 선언한 `chrono`를 감싸는
   표현식 검증, 다음 시각 N개, 기준 시각 판정을 둔다. 기본 optional seconds,
   DOM-OR-DOW, 하루 여러 시각, 잘못된 표현식과 unsupported `@reboot` 거부 테스트를
   `cargo test`로 독립 실행한다. explicit naive wall-tuple generator와
   `Local.from_local_datetime`의 nonexistent-skip/ambiguous-earliest wall key 규칙을
   `chrono-tz` deterministic fixture와 함께 검증한다. croner의 gap-shifted instant는
   discard할 candidate source가 아니라 사용하지 않는 API 결과로 테스트한다. daemon과
   preview는 system local timezone과 nonexistent-skip/ambiguous-once 규칙을 동일하게
   사용한다.
4. **정책 core**: `core/policies.rs`에 `occurrence <= startup_cutoff` catch-up
   (마지막 한 번), `occurrence > startup_cutoff` steady due(옵션 무관), enabled, 중복
   실행 정책 판정을 둔다. 실행 중인 process 없이 timestamp와 상태만 넣는 테스트로
   검증한다.
5. **잡 편집 UI·preview**: 이름·명령·cwd·환경변수·Windows/WSL 대상, cron
   builder, preset/direct input, 다음 실행 시각 N개, 활성 토글을 연결한다. vitest로
   표현식 생성과 preview 포맷을 검증한다.
6. **실행 어댑터·수동 실행**: Windows custom `CreateProcessW`
   `CREATE_SUSPENDED`→Job Object assign→`ResumeThread`와 `CREATE_NO_WINDOW`,
   WSL `wsl.exe -d ... --cd ... -- setsid bash --noprofile --norc -lc`,
   `Command::env`/`WSLENV` env 전달, framed PID/PGID/SID marker handshake,
   process-group cleanup, stdout/stderr 10 MB x5 회전 파일, DPAPI ciphertext,
   `runs` owner/attempt 메타데이터 기록을 붙인다. "지금 실행"이 자동 실행과 같은
   per-job mutex/overlap 정책을 타는지 확인한다.
7. **scheduler loop**: activity-timeline의 `spawn_poller` 패턴
   (`apps/activity-timeline/src-tauri/src/commands/tracking.rs:45-68`)으로 활성
   job을 1초 tick으로 평가하고 `core` 판정을 호출한다. per-job async mutex, DB
   transaction 밖 terminate/wait, queued→starting CAS, owner/token, single-instance
   등록, orderly shutdown, stale no-respawn recovery를 붙인다. startup gap catch-up
   한 번과 steady due, 중복 `skip/queue/kill-previous`를 Windows에서 수동 검증한다.
8. **이력·tail·보관·알림**: 기간/최근 50개 terminal metadata 이력 UI,
   stdout/stderr stream별 256 KiB lossless cursor tail, Rust-side 실패 Windows toast
   또는 sanitized outbox, `core/retention.rs`와 실제 파일 정리를 연결한다.
   stream별 10 MB x5, job별 50개 row, 전체 200MB terminal log/reference cap을 함께
   검증한다.
9. **Phase 1 완료 검증**: `cargo test`, `cargo check`, `pnpm build`, CI의
   frontend/rust/rust-windows를 통과시키고 지정한 Windows 수동 시나리오를 기록한다.

### Phase 2 — service

10. **service 모델·수명**: `kind = 'service'` CRUD, `service_instances` durable
    job_id PK/generation/state claim, start/stop/restart, 데몬과 함께 시작 옵션,
    라이브 로그를 붙인다. Phase 1 adapter의 Job Object/WSL session/group handle과
    process/log lifecycle을 그대로 재사용한다.
11. **Windows 트리 정리 재사용**: Phase 1에서 이미 만든 per-run Job Object와
    `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` handle을 service start/stop/restart에
    연결한다. `windows` crate의 Windows 전용 코드는 이 앱의 플랫폼 계층에만 둔다.
    `rust-windows`에서 실제 compile check를 통과시키고 `npm start` 고아 node
    검증을 한다.
12. **WSL 종료 재사용**: Phase 1에서 저장한 exact marker/PID/PGID/SID를 검증하고
    stop/restart에서 process-group SIGTERM→timeout→SIGKILL과 실제 group 소멸 확인을
    호출한다. WSL service 수동 검증을 Windows에서 수행한다.
13. **헬스체크·재시작**: `crates/process`의 포트 판정 primitive와 process
    liveness AND optional TCP probe(Windows host/WSL distro 내부)를 연결한다.
    immediate exit, start grace(수치는 미결), 10초 간격·3초 timeout·3회 연속 실패,
    generation CAS cancellation, `never/on-failure/always`와 `1/2/4/8/16/30초`
    fixed backoff/30초 cap을 검증한다.

## 의존성

- **Rust `croner = "3.0.1"`**: 사용자 입력은 표준 cron 표현식이고 필요한 핵심은
  표현식 검증과 naive wall-tuple field matching aid다. 3.0.1의 기본 parser는
  optional seconds를 사용하고 5-field 입력을 초 `0`으로 정규화하며, POSIX
  DOM-OR-DOW를 제공한다. `@reboot`은 지원하지 않으므로 validation에서 거부한다.
  timezone-aware next instant나 DST semantics는 사용하지 않고, `core/cron.rs`의
  explicit naive wall-tuple generator와 `Local.from_local_datetime` resolver가
  nonexistent를 discard하고 ambiguous earliest-offset wall key를 dedupe한다.
  라이브러리를 syntax/field matching aid로 감싸되 DST 의미를 croner 기본값에 맡기지
  않는다.
  `cron = "0.17"` 대신 이 선택을 고정하고, 실제 API 연결은 Phase 1의 순수 테스트로
  검증한다.
- **Rust `chrono = "0.4"` (직접 dependency, `clock` feature)**: croner의 간접
  dependency에 기대지 않고 system local timezone의 `DateTime`과 DST
  `LocalResult::{None, Single, Ambiguous}`를 정책 core에서 명시적으로 처리한다.
  **`chrono-tz` (dev-dependency)**는 system local timezone을 바꿀 수 없는 CI에서도
  gap/overlap fixture를 결정적으로 검증하는 데만 사용한다.
- **`rusqlite` (bundled)**: 이력 조회·기간 필터·정렬·보관 메타데이터가 필요하며,
  activity-timeline이 이미 `rusqlite = { version = "0.32", features = ["bundled"] }`
  를 사용한다(`apps/activity-timeline/src-tauri/Cargo.toml:23-27`). run-manager도
  저장소의 SQLite 배포 방식을 따른다.
- **`tokio` (`process`, `time`, `sync`, `io-util`)**: `tokio::time::interval`,
  per-job async mutex/oneshot shutdown, stream reader의 `AsyncReadExt`와 bounded
  channel을 사용한다. 일반 `tokio::process::Command`는 Windows Job Object spawn
  fallback으로 사용하지 않고, WSL argv/env 준비나 async reader가 필요한 경계에서만
  사용한다. Windows custom `CreateProcessW` adapter의 process/thread/job/pipe
  ownership과 wait 계약은 별도로 지킨다. life-log 선례
  (`apps/life-log/src-tauri/src/core/aggregate.rs:1-2`, `27-39`)와
  activity-timeline loop 선례(`apps/activity-timeline/src-tauri/src/commands/tracking.rs:49-51`)를
  따른다.
- **`windows` (Phase 1 platform dependency)**: Phase 1부터 Windows Job Object,
  `CREATE_SUSPENDED`/`CREATE_NO_WINDOW`, DPAPI
  `CryptProtectData`/`CryptUnprotectData`, CreateProcessW pipe/wait/shutdown boundary에
  사용한다. activity-timeline은 이미 Windows target dependency로 `windows`와
  `Win32_System_Threading`을 선언한다(`apps/activity-timeline/src-tauri/Cargo.toml:29-35`).
  run-manager는 최소한 `Win32_Foundation`,
  `Win32_Security_Cryptography`, `Win32_Storage_FileSystem`,
  `Win32_System_IO`, `Win32_System_JobObjects`, `Win32_System_Pipes`,
  `Win32_System_Threading` features를 target dependency에 선언한다.
  Windows session-end hook/Startup Shell Link를 실제 선택할 때 필요한 COM/UI Shell
  features도 그 adapter implementation과 함께 추가하며, Job Object 없는 fallback은
  허용하지 않는다.
- **`tauri-plugin-notification = "2"` + `@tauri-apps/plugin-notification` v2**:
  Rust plugin을 builder에 등록하고 `src-tauri/capabilities/default.json`에
  `notification:default`를 추가한다. scheduler가 sanitized `run-failed` event를
  Rust-side adapter에 넘겨 Windows toast를 직접 시도한다. hidden/unloaded webview의
  guest binding에만 의존하지 않으며, permission/API 실패 시
  `notification_outbox`에 job ID/run ID/fixed error code만 저장하거나 nonfatal error로
  기록한다. 사용자가 승인한 Windows toast 자체는 유지한다.
- **`tauri-plugin-single-instance = "2"`**: 단일 daemon instance와 second-instance
  전달을 보장하며 scheduler 생성 전에 등록한다. capability permission으로
  대체하지 않는다.
- **`uuid` (v4)**: daemon `owner_instance_id`, run ID, per-spawn `attempt_token`을
  cryptographically random UUID로 만든다. frontend number로 변환하지 않는다.
- **`zeroize`**: DPAPI plaintext map, old-secret per-run snapshot, redactor carry를
  모든 정상·오류·취소 경로에서 best-effort zeroize한다. OS/process memory에서
  완전한 forensic 삭제를 보장하는 것은 아니다.
- **`serde`, `serde_json`**: Tauri command DTO와 `env_ciphertext`/masked DTO
  직렬화에 사용한다.
  저장소의 기존 Tauri crate들도 derive 기능의 `serde`와 `serde_json`을 의존성으로
  선언한다(`apps/activity-timeline/src-tauri/Cargo.toml:23-24`).
- **`crates/process` (path dependency)**: `origin/main` `75b8af2`에서 이미 머지된
  package `process`를 Rust import `devbox_process`로 사용한다. 순수 포트 파서와
  sysinfo 기반 `ProcessSnapshot`/liveness primitive를 port-manager와 run-manager가
  함께 사용하며, Windows 명령 실행은 crate에 넣지 않는다.
- **startup shortcut API placeholder**: `platform/windows.rs`에는
  `StartupShortcut { create_or_update, remove }` 추상 경계만 먼저 둔다. 파일명·아이콘·
  인자는 `미결`이며 사용자가 선택한 값이 확정된 뒤 Windows Shell Link COM
  (IShellLink/IPersistFile) features 또는 검증된 wrapper를 implementation PR에
  추가한다. 미결 UX 값을 임의로 고정하거나 Linux core에 shortcut API를 넣지 않는다.
- **프론트 vitest**: root `package.json`의 기존 vitest 및 `pnpm test` 배선을
  사용한다(`package.json:6-15`). 별도의 테스트 프레임워크를 추가하지 않는다.

## 구현 부록 — v0.5.0 #357/#358

이 설계의 초기 Phase 2 계획을 유지하면서, v0.5.0의 실행 이력·import 경계가 실제로
구현된 계약을 아래에 고정한다. 이 부록은 원래 설계의 “구현은 하지 않는다”는 문서
작성 시점 설명을 대체하지 않고, 구현 PR에서 확정된 보안·상한·상태 전이를 기록한다.

### 실행 이력 필터

- `RunHistoryFilter`는 `job_id`, `kind(job/service)`, `status`, half-open epoch-ms
  `start_at`/`end_at`, 실행 시간 `min_duration_ms`/`max_duration_ms`, `limit`을
  선택적으로 받는다. ID는 128바이트, 실행 시간은 0~30일, limit은 1~500으로
  native 경계에서 검증하며 날짜 범위의 끝은 시작보다 엄격히 뒤여야 한다.
- 저장소는 `runs JOIN jobs` 한 번의 parameterized SQLite query로 모든 조건을
  조합한다. 종료하지 않은 run의 duration은 조회 시각을 사용하고, 시작하지 않은
  queued/skipped row는 duration 조건에 포함하지 않는다. SQL, path, command, log
  본문, environment ciphertext는 이 API의 입력·결과에 들어오지 않는다.
- UI는 작업과 서비스를 함께 조회하거나 종류별로 좁힐 수 있고, service history
  row에서는 job 전용 지금 실행/중지/재실행 동작을 노출하지 않는다. 기존 `RunView`
  redaction과 반열린 시간 의미를 그대로 사용한다.

### 정의 JSON 및 native task import

- 기존 definition JSON은 schema version 1, 최대 512KiB/총 128개 정의로 제한한다.
  UUID와 필드 shape를 검증하고, preview의 고정 SHA-256 revision을 apply에서 다시
  비교한다. 선택된 job/service는 기존 ID와 충돌하면 건너뛰며, 모든 검증·삽입·service
  instance 생성은 하나의 `BEGIN IMMEDIATE` transaction으로 처리한다.
- import는 원본의 enabled/auto-start와 environment 값을 실행 경계로 전달하지
  않는다. 생성된 모든 정의는 `enabled=false`, service `auto_start=false`,
  environment `Keep`/ciphertext 없음인 disabled draft다. 사용자는 cwd·환경 상태를
  확인한 뒤 별도로 활성화한다. definition 저장은 bounded non-cancellable
  operation이므로 저장 중 Escape가 이미 커밋될 수 있는 작업을 취소한다고 가장하지
  않는다.
- 사용자가 고른 프로젝트 루트의 바로 아래 `package.json`/`Cargo.toml` 내용만 native
  parser로 읽는다. npm, Cargo metadata, shell, network, dotenv, imported command를
  실행하지 않는다. 파일 내용은 각각 512KiB, 전체 결과는 128개로 제한한다. script는
  body가 아닌 `npm run -- <safe-name>`, Cargo target은 제한된 name을 사용한
  `cargo run/test/bench --...` command로 변환한다. Windows `%KEY%`, POSIX `$KEY`/
  `${KEY}`는 이름만 최대 64개 preview에 보여주고 값은 읽거나 저장하지 않는다.
  Cargo 자동 target 판정에는 source 내용·Cargo metadata 실행 없이 `src/lib.rs`,
  `src/main.rs`, `src/bin`, `examples`, `tests`, `benches`의 표준 layout을 fixed-depth
  bounded metadata로만 확인한다. workspace member의 다른 `Cargo.toml`은 읽지 않으며,
  virtual workspace는 직접 target을 제공하지 않는 것으로 처리한다.
  `autolib`/`autobins`/`autoexamples`/`autotests`/`autobenches`와 edition별 자동
  discovery 기본값을 적용한다. 명시 target과 자동 target은 `(kind, name, path)`로
  dedupe하고, 명시 target의 파일이 없거나 root 밖·symlink/reparse point이면
  fail-closed 한다. `autobins=false`에서는 자동 binary를 만들지 않으며, 자동·명시
  binary 모두 `cargo run --bin <name>`을 사용해 bare `cargo run`을 만들지 않는다.
  non-bin example과 `required-features` target은 실행 task로 만들지 않는다. 명시적인
  `[[bin]]`의 `name`이 생략된 경우에만 안전한 상대 `path`의 파일명에서 target 이름을
  추론한다. preview에 없는 selection ID는 apply에서 거부한다. VS Code `tasks.json`
  parsing은 이 #358 후보에 포함하지 않고, §13.2 정의 import 후속 범위로 보류한다.
- 선택 루트는 absolute/non-symlink/no-follow filesystem identity로 canonicalize하고
  source file의 metadata·canonical parent/name을 확인한다. source path와 실제 열린 file
  handle fingerprint를 read 전후 비교하고 현재 path identity/fingerprint도 다시 확인한다.
  Cargo layout discovery는 source 파일을 열거나 읽지 않고 fixed-depth directory entry,
  regular-file metadata, no-follow identity만 확인한다. 표준 target directory와 target
  파일의 symlink/reparse point는 따라가지 않고 거부하며, directory entry·target 수와
  operation budget을 bounded하게 유지한다. preview revision은 root identity와 정확한
  두 source byte snapshot, 정렬된 Cargo layout metadata snapshot을 포함한 opaque
  SHA-256(64 hex)이며 절대 경로를 digest에 넣지 않는다. apply는 root/source/layout을
  다시 읽어 revision과 표시 root를 비교하고, 변경되면 stale 오류로 중단한다.
- project apply의 `(kind, name, normalized cwd)` 충돌은 preview와 `BEGIN IMMEDIATE`
  transaction에서 같은 `SafeProjectPath` identity(Windows case/separator alias
  포함)로 재확인한다. operation ID는 64바이트/허용 문자만 받고 중복·동시 4개를
  제한하며, native preview budget은 5초다. 취소는 exact operation의 cooperative
  flag이고, transaction 각 row 전·commit 직전에 확인하여 commit 전 취소는 전체
  rollback한다. 이미 커밋된 transaction은 되돌리지 않는다.
- import dialog는 preview generation으로 늦은 응답을 무시하고, operation timeout/
  unmount 시 exact operation을 취소하며 unmount 뒤에는 비동기 state를 갱신하지 않는다.
  modal focus trap, Escape semantics, `aria-busy`/tab/tabpanel 관계와 완료 후 trigger
  focus 복구를 유지한다. 변경집합 액션은 form 안에서도 submit으로 오인되지 않는다.

### 구현 검증 및 잔여 범위

전용 worktree의 focused Rust gate는 `cargo test -p run-manager --lib -j1` 186개와
`cargo check -p run-manager --lib -j1`을 통과했다. offline pnpm store로 의존성을 복원해
Vitest 6 files/39 tests와 production build, `git diff --check`/format 검사를 통과했다.
Windows W3 packaged smoke는 CI/Windows acceptance에서 수행한다. directory handle 기반
relative open이 없는 OS의 final root identity-check 직후 교체 race와 committed
transaction의 사후 취소는 알려진 잔여 경계다. 원격 host, Kubernetes, DAG orchestration,
범용 tasks workflow는 이 구현 부록의 범위가 아니다.

후속 P1 보강에서 Cargo target auto-discovery는 위의 동일한 no-execution 경계를 유지한 채
고정된 표준 layout에 대한 bounded metadata-only 탐색으로 구현한다. `Cargo.toml` 외 source
내용은 읽지 않으며, 자동 target layout snapshot도 preview revision에 포함해 apply 시 stale를
검출한다. 이 보강의 focused Rust/Windows 검증은 해당 후보 PR의 CI gate에서 다시 수행한다.
