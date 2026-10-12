# S0a 구조 확인 결과

- 상태: 자동 측정·문서·소유 자원 정리 완료(2026-10-12 KST). Task 1–8 별도 검토 통과; R1/R2 실기는 유보이며 PASS가 아니다.
- 측정 시작: 2026-10-12 KST. 원격 계획 기준 `4aa88e62`.
- 환경: Windows 11 `10.0.26200.9457`, Ubuntu WSL 2.7.12.0(커널 6.18.33.2-2), systemd 259, tmux 3.6, Rust 1.98.1, PowerShell 5.1.26100.9444, Node 24.18.0, pnpm 9.0.0, Claude Code 2.1.296, codex-cli 0.162.1.
- 실험 소스·원시 증거: `/home/jihoon/projects/devbox-spikes/s0a/`. 사용자 데이터 대신 새 실험 파일과 임의 바이트를 사용한다.

| 항목 | 합격 기준 | 측정값 | 판정 |
|---|---|---|---|
| R3 브리지 왕복 64B p50 | ≤ 5ms | 0.5194ms | PASS |
| R3 처리량 | ≥ 20 MiB/s | 60.5919 MiB/s | PASS |
| R3 바이너리 8MiB 일치 | true | true | PASS |
| 브리지 입력 종료 | 제한 시간 안 정상 종료 | 5초 이내, exit 0 | PASS |
| R2 socket activation | 첫 연결에 서비스 기동 | inactive → active, fd3 상속 확인 | PASS |
| R13 EnvironmentFile로 사용자 도구 찾기 | claude·codex·pnpm·cargo 경로 출력 | 네 도구 모두 로그인 환경의 경로로 해결 | PASS |
| 실행 결과 기록(ExecStopPost) | ok/fail/kill 세 결과 | success exited 0 / exit-code exited 3 / success killed TERM | PASS |
| 데몬 재시작 중 실행 생존 | active | 독립 실행 PID 유지, 30초 뒤 정상 종료 | PASS |
| tmux `-D` 지원 | 지원/미지원(대체 기록) | tmux 3.6, foreground unit 정상 | PASS |
| tmux 서버 unit 유지 + scope 측정 | CPU 증가·정지 시 정리 | CPU 증가·메모리 관찰·소유 PID/cgroup 정리, 데몬 재시작 중 세션 유지 | PASS |
| 대체 화면 무효화 | ESC[?1049h 없음 | 실제 attach 출력에서 없음, 300줄 capture | PASS |
| read-only,ignore-size attach | 오류 없음 | 읽기 전용 입력 차단, 110×35 크기 유지 | PASS |
| PTY + tmux 입력 왕복(SC10 예비 측정) | p50 ≤ 10ms·p95 ≤ 25ms | PTY p50 0.3602ms·p95 0.4638ms; 보수적 합 0.9816/1.2268ms | 예비 PASS |
| `systemctl --user is-system-running` | running 또는 degraded | running | PASS |
| R10 systemd 아래 interop | cmd.exe ver 출력 | 절대 경로 cmd.exe 성공, WSL_INTEROP 없이 동작 | PASS |
| R10 DPAPI 왕복(systemd 아래) | OK | 32바이트 일치, protect 551.808ms / unprotect 523.850ms | PASS |
| R5 Claude hook(`--settings`) | SessionStart·UserPromptSubmit·Notification 기록, 사용자 설정 불변 | 실제 TUI 전달 확인, 설정 SHA/mtime 불변 | PASS |
| R5 Codex notify(`-c`) | agent-turn-complete JSON, 사용자 설정 불변 | 실제 TUI 전달 확인, 설정 SHA/mtime 불변 | PASS |
| R1 유휴 30분 유지 | WSL·데몬 유지 | S0 확인에서 채움 | 유보 |
| R2 shutdown 뒤 복귀 | 다시 연결 + 걸린 시간 | S0 확인에서 채움 | 유보 |

## 브리지 측정

Windows PowerShell에서 `wsl.exe -d Ubuntu --exec <probe> bridge`를 실행했다. 단순 Linux echo 대역으로 Windows 경로를 대신하지 않았다. 64B·4KiB 각각 1,000개 표본의 nearest-rank 백분위를 계산했다. 처리량은 1MiB를 64회 보내고 되받아 내용까지 비교한 경과 시간에 대한 payload 크기이며, 송수신 합계로 두 배 부풀리지 않았다.

| 경로 | 64B p50 | p95 | p99 | 4KiB p50 | 처리량 | 8MiB / EOF 종료 |
|---|---:|---:|---:|---:|---:|---|
| 직접 실행한 probe | 0.5194ms | 0.6959ms | 0.8718ms | 0.4946ms | 60.5919 MiB/s | 일치 / exit 0 |
| systemd socket activation | 0.6214ms | 0.7630ms | 0.8831ms | 0.5017ms | 63.2260 MiB/s | 일치 / exit 0 |

- 첫 행 원시 증거: `task1-measurements.json`(UTC `2026-10-12T14:03:22.9373395Z`), `task1-bench-console.txt`.
- 두 번째 행: `task2-run-7f8pjp2a/measurements.json`.
- 직접 실행의 프로세스 시작 → 표지 수신은 107.03ms, socket activation 경로는 116.4724ms다. WSL이 이미 실행 중인 측정이며 콜드 VM 기동 시간은 아니다.
- 좁은 회귀 검사는 stdin을 연 상태의 64B 응답, 8MiB 정확한 응답, EOF 후 종료를 확인한다. 최종 소유권 보완 뒤 1개 검사 통과(`task1-tests-safety.log`). 별도 검토에서 원시 표본으로 백분위를 다시 계산하고 일치함을 확인했다.
- 초기 실험 시간 초과는 PowerShell 비동기 쓰기 뒤 `FlushAsync()` 누락으로 확인했다. 원래 Rust `tokio::io::copy`로 되돌린 대조 검사도 작은 응답을 정상 처리했다. Rust copy를 결함으로 판정하지 않는다. 보존된 로그와 당시 관찰의 구분은 `task1-notes.txt`에 남겼다.
- 실험 runner와 회귀 검사는 private 0700 디렉터리, 서버 준비 확인, 소유 소켓만 정리하는 경계, 신선한 결과 파일과 실패 종료 코드를 사용한다. 소유권 보완 후 측정 로직은 그대로여서 성능 검사를 반복하지 않았다.

## systemd와 실행 환경

- `devbox-s0a-20261012@probe.service`는 첫 연결 전 inactive, 연결 뒤 active였다. MainPID 2156897의 fd3 `socket:[21136359]`와 `/proc/net/unix`의 소유 소켓이 일치했다. 로그의 문자열이 아닌 상속된 descriptor로 socket activation을 확인했다.
- EnvironmentFile 없이 실행한 대조군은 claude·pnpm·cargo를 찾지 못했고 codex만 `/usr/local/bin/codex`에서 찾았다. EnvironmentFile 적용 후 네 도구 모두 로그인 셸의 경로로 해결됐다. 계획의 MISSING 예상을 도구 전체에 일괄 적용하지 않았다.
- 환경 파일은 PATH·HOME·LANG·CARGO_HOME·RUSTUP_HOME만 선택해 기록했다. 디렉터리 0700, 파일 0600, Unix socket 0600이다. systemd manager의 전역 환경은 바꾸지 않았다.
- 증거: `task2-run-7f8pjp2a/`의 `ownership.json`, `descriptor-proof.txt`, 전후 unit 상태, journal, 도구 대조 결과. 별도 검토에서 소유 파일 hash와 현재 descriptor를 확인했다.

## 실행 결과와 수명

- 정상·실패·SIGTERM 모두 ExecStopPost 결과가 남았다. 실패한 실행은 exit 3으로 구분하고, 강제 종료는 TERM으로 구분했다.
- 강제 종료 대상 parent/child PID 2162045·2162046의 소유 cgroup membership을 먼저 확인했다. 중지 후 두 PID가 사라지고 해당 cgroup이 비거나 제거됐음을 확인했다. 시스템 전체의 같은 이름 프로세스를 결과 판정에 쓰지 않았다.
- probe PID 2156897 → 2162059 재시작 중에도 독립 실행 PID 2162055는 active였다. 30초 실행이 끝난 뒤 결과는 `success exited 0`이었다.
- CPUUsageNSec 1,004,825,000, MemoryCurrent 3,362,816을 읽었다. 실험용 transient unit 5개는 모두 수거돼 LoadState=not-found였다. Task 2의 template·환경 파일 hash와 활성 socket은 보존했다.
- 증거: `task3-run-a59w8yji/{results.json,ownership.json,review-notes.txt}` 및 결과 파일. 별도 검토에서 script hash·다섯 결과와 Task 2 보존 상태를 대조했다.
- 첫 시도 `task3-run-b5t7v78n`은 systemd의 `$$` 확장 때문에 실험 스크립트가 실패했다. `--expand-environment=no`로 명령 전달을 수정했다. 이 실패는 제품·설계 불합격으로 분류하지 않는다.

## tmux와 PTY

- tmux 3.6의 foreground 서버를 전용 transient unit으로 실행했다. `-D`와 별도 명령을 함께 주는 계획의 탐지 예제 대신 실제 기한 있는 서버 기동으로 확인했다.
- `line-1`부터 `line-300`까지 순서대로 capture됐다. 실제 attach의 바이트 출력에는 `ESC[?1049h`가 없었다. 읽기 전용 클라이언트에서는 실험 파일 생성 입력이 막혔고, 쓰기 가능한 대조 클라이언트에서는 같은 입력이 동작했다. 작은 클라이언트가 붙어도 세션은 110×35를 유지했다.
- 200개 고유 UTF-8 표지의 정확한 응답을 받았고 timeout/EOF는 0회였다. p50 0.360156ms, p95 0.463830ms다. 두 백분위와 직접 브리지의 합은 0.8796/1.1597ms, 더 보수적인 socket-activation 브리지와 합은 0.9816/1.2268ms다. 각각의 백분위 합은 전체 경로의 백분위 실측값이 아니다.
- scope의 CPUUsageNSec는 168,641,000 → 468,705,000으로 증가했고 MemoryCurrent는 7,299,072였다. scope를 명시적으로 멈춘 뒤 소유 PID와 cgroup이 정리됐다.
- 데몬 역할 probe PID 2162059 → 2170974 재시작 중 tmux PID 2170157과 세션 a1은 유지됐다. 초기 지연 측정의 tmux PID 2167149와 후속 실험을 위해 다시 만든 서버 PID를 혼동하지 않는다.
- 추가 수명 실험에서 tmux 자체를 멈추면 기본 대역 프로세스는 SIGHUP 1을 받아 종료됐다. HUP 처리 대역은 살아 있어 scope를 명시적으로 멈춰야 했다. `signal.json=1`이 HUP 근거이고, `nohup-signal.json=15`는 마지막 명시적 정리 SIGTERM의 근거다. 실제 Claude/Codex가 PTY 소실 후 계속 사용 가능하다는 뜻은 아니다.
- 따라서 01-design §4.3의 무조건 생존 문구를 보정했다. 별도 scope 구조와 명시적 정리 정책은 그대로다. 데몬 재시작 시 세션 유지 기준은 통과했다.
- 증거: `task4-run-pt11__ib/`의 results·ownership·capture·attach·samples·signal·daemon-restart·scope-cleanup 기록. 별도 검토에서 200개 표본의 백분위를 재계산하고 소유 scope/PID의 정리를 확인했다.
- 초기 0.6초 CPU probe가 공통 검증 잠금 없이 실행된 절차 이탈이 있었다. 후속 수명 실험은 잠금을 사용했다. 측정 로직이 바뀌지 않은 지연 검사는 반복하지 않았다.

## Windows interop와 DPAPI

- 직접 셸과 `systemd-run --user --wait --pipe --collect` 양쪽에서 절대 경로 cmd.exe로 Windows 버전을 읽었다. 각각 49.441ms, 91.989ms다.
- 서비스 환경에는 WSL_INTEROP와 WSL_DISTRO_NAME이 모두 없었지만 호출은 성공했다. 명시적 interop socket 대체 경로는 사용할 필요가 없었다. 이번 호스트·세션에서 확인한 결과이며 재부팅·로그인 세션 종료 뒤까지 입증한 것은 아니다.
- 새로 생성한 32바이트 임의 키를 DPAPI CurrentUser로 봉인하고 복원했다. 키·봉인 blob·복원 값은 메모리와 stdin/stdout pipe 안에만 두고 인자·파일·로그에 쓰지 않았다. strict Base64 decode와 constant-time 비교로 일치를 확인했다.
- systemd 경로의 protect는 551.808ms, unprotect는 523.850ms이며 두 단계 합은 1,075.658ms다. 직접 unprotect 대조는 478.277ms였다. 시간에는 PowerShell 기동과 systemd 경로의 경우 unit 실행 비용이 포함된다. 개별 키 풀기는 1초 아래이며, 데몬 시작 때 한 번 푸는 설계를 유지한다.
- 실험 unit 4개 모두 LoadState=not-found·ActiveState=inactive로 수거됐다. Task 2의 빌린 환경·template·runner hash 및 소켓 identity는 보존됐다.
- 증거: `task5-run-ow1c4o67/{results.json,ownership.json}` 및 runner. 별도 검토에서 실제 systemd 호출 경로·키 비영속성·명령과 결과의 구분·소유 자원 정리를 확인했다.

## 설계 반영

현재까지 구조·결정을 바꾸는 대체 경로가 필요한 불합격은 없다. tmux 종료와 SIGHUP의 사실 관계를 01 §4.3에 보정했으며, 측정·스크립트 보정은 PROGRESS §5에 기록한다. SC10의 최종 판정은 S1에서 브리지·데몬·PTY·tmux 전체 경로와 출력 부하를 포함해 수행한다.

## hook 세부(S1 입력)

실제 대화형 TUI에서 측정했다. 증거는 `task6-run-8iljg2r6/{events.jsonl,notify-correlation.json,manifest.json,config-before.json,config-after.json,teardown.json}`이다. 초기 synthetic fixture cwd의 신뢰 화면과 효과가 없었던 Codex session-only trust override는 성공 근거에서 제외했다. 성공 실행은 이미 신뢰된 main cwd였고 명령 대상은 절대 경로의 소유 fixture뿐이었다. fixture cwd에서 성공했다고 보고하지 않는다.

- Claude: 같은 session_id의 `SessionStart`, `UserPromptSubmit`, `PermissionRequest`, `Notification(notification_type=permission_prompt)`를 받았다. PermissionRequest → Notification은 6,018ms였다. 따라서 PermissionRequest로 즉시 waiting, permission_prompt/agent_needs_input 알림은 fallback, idle_prompt는 idle, 알 수 없는 알림은 무시한다. 취소 뒤 Stop은 없었으며 완료 턴의 Stop을 실측한 것으로 주장하지 않는다. 예시 필드: `{"hook_event_name":"PermissionRequest","session_id":"<same-session>"}`, `{"hook_event_name":"Notification","notification_type":"permission_prompt","session_id":"<same-session>"}`(식별자 투영).
- Codex: `pong` 주 thread `01a13084-46f7-7421-8a71-344f7f1ae68d`, turn `01a13084-d8c6-7883-aca6-6a1004f92a32`의 완료 JSON이 **단일 인자**로 전달됐다. 2,427ms 뒤 자동 제목 thread `01a13084-e0ea-7c82-871d-b9bd18d8c2e3`의 notify도 같은 DEVBOX_AGENT_ID로 왔다(제목 요청은 보내지 않았다). 양쪽 필드는 `type/thread-id/turn-id/cwd/input-messages/last-assistant-message`뿐으로 parent/source 구분자가 없다.
- 정확한 주 thread rollout metadata는 `source=cli`, `thread_source=user`, cwd=main이었다. 제목 ID의 rollout은 exact-ID 검색에서 없었다. ENV/cwd 또는 첫 notify만으로 바인딩하면 오염될 수 있다. 정확한 thread의 source·작업 launch/workdir 검증과 현재 turn lifecycle 상관관계를 S1 Task 8·10에 필수로 반영했다. 구현·완성된 바인딩 검증은 S1에 남으며 S0a 완료 notify 관찰과 구분한다.
- Codex 파일 승인 화면은 실제로 관찰하고 취소했다. `codex-wait.txt`의 승인 UI(`Would you like to run the following command?`, `Yes, proceed (y)`, `No, and tell Codex what to do differently (esc)`)를 출력 fallback 패턴 입력으로 사용하되 버전별 회귀가 필요하다. 취소 뒤 hello 파일은 없었다. notify가 승인 대기를 전달했다고 주장하지 않는다.
- `~/.claude/settings.json`, `~/.codex/config.toml` SHA와 mtime는 전후 일치했다. 사용자 설정을 수정하지 않았고 main은 clean, 소유 CLI 세션만 정리했다. 당시 Task 4의 a1/server는 Task 8 정리를 위해 남겼다.

사전 확인한 공식 계약: Codex `notify`는 단일 JSON 인자로 턴 완료를 전달하며, TUI의 승인 알림과는 별도다([OpenAI 설정 문서](https://learn.chatgpt.com/docs/config-file/config-advanced#notifications)). Claude command hook은 stdin JSON을 받는다([Claude hook 문서](https://code.claude.com/docs/en/hooks)).

## 정리

Task 8에서 Task 2 template/env hash·두 소켓 device/inode와 Task 4 config hash·PID 2170157/cgroup·유일한 a1 session을 확인했다. 정확한 probe socket → probe service → private tmux service를 멈춘 뒤 소유 template/env/socket과 빈 env 디렉터리만 제거하고 daemon-reload했다. wildcard stop/rm, 전역 reset-failed, 공유 서비스·Docker·네트워크 조작은 없었다. `task8-cleanup.json`의 전후 상태에서 두 probe unit·tmux service·네 owned scope는 모두 inactive/not-found이고 소유 파일/소켓은 없다. Task 1/3/5/6 종료 기록과 함께 활성 실험 자원 정리를 확인했다. main clean을 재확인했다. 외부 실험 소스·원시 증거·fixture는 S1 완료까지 보존한다.

## 수동 측정과 S0b 준비 판정

`task7-deferred.json`: R1 connected-client 30분 유휴는 S0 확인 7, R2 shutdown 재연결 시간은 확인 4·8로 유보했다. 관측·경과 시간은 null이고 실제 대기는 실행하지 않았다. 설치 앱 **창을 열고 연결한 상태**로 확인한다. 트레이/닫기 동작은 S1 Task 23이며 S0b에서 조기 구현하지 않는다. 앱 종료 뒤 WSL 정지와 v0.9.0 시작 시간 비교는 계획에 따라 생략하고 S1 SC3 절대 기준(2초)을 검증한다.

자동 측정은 합격이고 구조 대체가 필요한 실패는 없다. 기존 아키텍처 안의 hook source/turn 계약 보정을 01 §9.2와 04 Task 8·10·resume에 반영했다. **Task 8 최종 검토까지 통과했으며 S0b 착수 조건을 충족했다**. R1/R2는 S0 확인에서 판정하고, SC10 전체 경로·Codex 바인딩 구현은 S1에서 검증한다. S0b 착수·Windows 수용·PR/CI 완료를 뜻하지 않는다.

## 최종 통합 확인

Task 1–8 구현/실험과 별도 검토를 마쳤다. 커밋 전 `pnpm exec biome ci .`는 공통 검증 supervisor의 잠금·CPU/메모리 제한 아래에서 1,942개 파일을 검사해 통과했다(수정 없음). 문서만 바뀌었으므로 제품 전체 build/test·PR·CI를 새로 실행하지 않았다.

## ADR 0017 초안 — 재구축: 단일 앱 + WSL 데몬

상태: 채택(초안, S0b Task 1에서 ADR로 이동) · 기록일: 2026-10-12

맥락: 사용 빈도 1위인 에이전트 기능(세션 유지, hook 상태 감지, 알림, 재개)이 v0.9.0 구조에 없었고, 리뷰가 확정한 결함 약 20건 중 에이전트·터미널 결함은 0건이었다. 네 제품·설치 세대·제품 간 연결·Windows 프로세스 처리·WSL helper 중계 및 Windows·WSL 이중 경로 파일은 전체 약 49.7만 줄의 약 25%(파일 단위 하한), `.github`·`scripts`는 5.6만 줄이었다. 사용자는 데이터 이전 없이 WSL 중심 재구축을 결정했다.

결정: `01-design.md`를 원장으로 하는 단일 Windows Tauri 앱 + WSL Linux 데몬 구조로 다시 짓는다. S0a 자동 측정은 위 결과와 같이 합격했다. R1/R2 수동 판정은 S0b S0 확인에 남긴다. 별도 scope는 PTY 소실의 SIGHUP 생존을 보장하지 않으며 명시적 종료 정책을 유지한다. hook은 Claude 즉시 PermissionRequest와 Codex 주 thread·현재 turn의 검증된 source 계약을 사용한다.

결과: ADR 0001·0002·0003·0004·0007·0009·0010·0011·0012·0014·0015를 대체한다. 0005·0006·0008·0013·0016은 01 §13의 형태로 유지한다. Windows 설치본 수용, SC3·SC10 전체 경로 및 세션 tracker 회귀는 후속 단계의 gate이며 이 초안의 측정 PASS로 대체하지 않는다.
