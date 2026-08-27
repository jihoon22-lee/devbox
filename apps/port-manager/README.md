# port-manager — Port & Process Manager

현재 PC에서 사용 중인 listener와 연결된 프로세스를 한 화면에서 확인하고,
같은 endpoint와 같은 프로세스 실행인지 다시 확인한 뒤 안전하게 종료하는 앱이다.
산출물: PortManager.exe (apps/port-manager).

## 주요 기능

- **listener 목록** — Windows native, WSL distro, Docker published-port source를
  source / proto / port / local address / state로 구분한다.
- **Windows 상세** — process name, bounded full command line, executable path, PID,
  process creation FILETIME을 표시한다. 권한 때문에 읽지 못한 값은 비워 둔다.
- **WSL 상세** — distro, protocol, address/port, PID, command, Linux
  proc pid stat의 start tick을 표시한다.
- **컨테이너 상세** — Docker engine, distro, container ID/name과 published port를
  표시한다. 컨테이너는 OS PID로 종료하지 않고 WSL Desktop의 명시적 stop action
  handoff 대상으로만 취급한다.
- **검색·필터** — port 번호, process/container name, distro, PID, endpoint,
  protocol/state를 부분 일치로 검색하고 TCP/UDP·state·pinned를 필터링한다.
- **Open localhost** — LISTENING native/WSL/container row의 localhost URL을
  제한적으로 열 수 있다.
- **bounded auto-refresh와 pause** — 1–60초 범위의 interval을 저장하고, 수동
  pause/resume 중에는 timer가 새 native poll을 시작하지 않는다. 이미 진행 중인
  poll은 single-flight로 한 번만 완료된다.
- **refresh diff** — 첫 성공 snapshot은 baseline으로만 삼고, 다음 성공 snapshot부터
  process/container identity를 기준으로 `new`·`closed`·`changed`를 표시한다. 실패한
  poll은 baseline과 diff를 덮어쓰지 않는다.
- **favorite와 pinned filter** — endpoint(port) favorite와 validated process identity
  favorite를 별도로 저장하고 pinned filter에서 합집합으로 표시한다. 저장 문서에는
  command line, executable path, secret, raw process input이 없다.
- **provenance** — 행과 상세 패널에 Windows, WSL distro, container engine/distro/ID를
  표시해 동일한 port 숫자의 출처를 구분한다.

## Identity-safe 종료 계약

프론트엔드는 PID 하나를 보내지 않는다. 선택한 행의 endpoint와 다음 identity를
함께 보낸다.

- Windows: PID + process creation FILETIME ticks. The FILETIME is serialized as
  a decimal string, not a JavaScript number, so its 100 ns precision is not rounded.
- WSL: distro + PID + proc start tick
- Container: engine + distro + container ID

백엔드는 실행 직전에 동일한 source를 다시 조회해 endpoint와 identity를 모두
비교한다. endpoint가 사라졌거나 PID가 재사용되어 start time/start tick이 달라지면
고정된 stale-target 오류로 중단한다. Windows는 같은 identity를 확인한 process
handle에서만 TerminateProcess를 호출하고, WSL은 같은 start tick을 다시 확인한
뒤 고정 argv인 wsl.exe -d DISTRO -- kill -TERM -- PID를 실행한다. established
connection이나 bare PID는 종료 대상이 아니다.

컨테이너 stop은 process kill이 아니다. 현재 구현은 검증된
ContainerStopHandoff(target app wsl-desktop, action stop-container, engine, distro,
container ID)를 반환한다. one-time atomic handoff store와 WSL Desktop 소비 UI는
applink protocol v2 작업의 소유 영역이며, 이 앱은 그 경계 밖에서 container engine을
직접 제어하지 않는다.

## Privacy와 bounded input

- netstat, WSL, Docker child output은 source당 2 MiB를 넘으면 거부한다.
- listener는 최대 4,096개, WSL distro는 최대 16개, unique process detail lookup은
  최대 256개다. 동일 distro/PID의 여러 endpoint는 한 번 조회한 start tick/command를 공유한다.
- 한 listener snapshot의 child 명령은 공통 15초 deadline을 공유하며, deadline이나
  stdout 상한을 넘긴 child는 종료한다.
- command line, executable path, process/container name은 bounded 문자열이다.
  Windows process-start identity도 decimal string으로 bounded serialize한다.
  command line의 password/token/secret/API key/authorization/cookie 계열 값은
  화면에 표시하기 전에 redacted 처리한다.
- child stderr와 OS 오류 원문은 UI로 전달하지 않는다. 실패 메시지는 PID, path,
  distro, command, credential을 포함하지 않는 고정 문구다.
- view preference JSON은 app-local data 아래에서만 읽고 64 KiB, 종류별 favorite 256개,
  endpoint field bounds, 1–60초 interval을 strict 검증한다. `deny_unknown_fields`와
  atomic replace를 사용하며 invalid/oversized/symlink 문서는 읽지 않는다. 저장 실패 시
  현재 화면 preference를 commit하지 않는다.
- 외부 실행은 Windows netstat와 검증된 WSL argv뿐이며 셸 문자열 조합,
  임의 executable/path, arbitrary process kill, 자동 재실행은 제공하지 않는다.

## 기술

- Windows netstat -ano output과 WSL ss -H -lntup output의 순수 parser
- Windows process handle identity: GetProcessTimes, QueryFullProcessImageNameW
- WSL /proc start tick과 cmdline fixture parser
- Docker ps의 ID/name/published-port bounded parser
- 공용 크레이트 crates/process, crates/wsl 사용
- Tauri child process는 console window 없이 실행하고 bounded stdout만 읽는다
- native preferences: `src-tauri/src/core/preferences.rs` + atomic app-local JSON
- frontend refresh/diff/favorite model: `src/refresh.ts` + request generation/unmount guards

## 개발

- 순수 로직: src-tauri/src/core/listeners.rs
- preferences/adapter: src-tauri/src/core/preferences.rs,
  src-tauri/src/commands/preferences.rs
- Tauri listener adapter: src-tauri/src/commands/ports.rs
- frontend state/fixture: src/App.tsx, src/refresh.ts, src/App.test.tsx
- Windows 실행/빌드: pnpm tauri dev / pnpm tauri build
- focused Rust 검증: cargo test --manifest-path apps/port-manager/src-tauri/Cargo.toml --lib -j2
- focused frontend 검증: pnpm --filter port-manager test && pnpm --filter port-manager build
