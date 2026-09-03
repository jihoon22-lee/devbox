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
  process/container identity를 기준으로 `opened`·`closed`·`changed`·`owner-changed`를 표시한다.
  실패한 poll은 baseline과 diff를 덮어쓰지 않으며, session timeline은 최대 256개 event만
  메모리에 둔다.
- **favorite와 pinned filter** — endpoint(port) favorite와 validated process identity
  favorite를 별도로 저장하고 pinned filter에서 합집합으로 표시한다. 저장 문서에는
  command line, executable path, secret, raw process input이 없다.
- **provenance** — 행과 상세 패널에 Windows, WSL distro, container engine/distro/ID를
  표시해 동일한 port 숫자의 출처를 구분한다.

## Run Manager·Workbench 관찰 correlation 계약

Port Manager는 공용 strict named view인 `snapshot:port-bindings/v1`의 `port-bindings` view를
읽기 전용으로 소비한다. Run Manager와 Workbench는 서로의 상태에 의존하지 않는 독립 producer로
각자의 view를 발행하며, native `list_port_observations` 응답은 `rows`와 producer별
`sources`를 함께 반환한다. correlation은 `source_app`, `target_kind`, `target_id`, `label`,
`confidence`, `action_key`, `logs_available`의 snake_case 필드이고, source 상태는
`producer`, `state`(`available`/`missing`/`invalid`/`stale`), `freshness_ms`로 진단한다.

각 producer view는 `generatedAt` 기준 180초 이내일 때만 `available`로 사용한다. missing·invalid·
stale source는 그 producer의 correlation만 격리하며, native listener row와 다른 producer의
correlation을 실패로 만들지 않는다.

응답 correlation은 confidence/source/id 기준으로 결정적으로 정렬한 뒤 listener 행당 64개,
snapshot 전체 4,096개로 제한한다. 제한에 닿으면 `correlations_truncated=true`와 UI 진단을 표시해
잘린 결과를 완전한 목록처럼 보이지 않게 한다.

- `verified` — Windows listener의 PID와 process creation FILETIME을 native에서 정확한 epoch
  milliseconds로 변환한 값이 Run Manager의 process identity와 정확히 일치한다.
- `declared` — Run Manager의 loopback address·TCP port와 Windows/WSL target(source·distro)이
  일치하지만 exact Windows process identity를 확인하지 못한 선언이다. `localhost`만 양쪽
  loopback stack을 허용하고, 구체적인 IPv4/IPv6 선언은 listener bind와 정확히 맞아야 한다.
  WSL에는 verified ownership을 부여하지 않는다.
- `expected` — Workbench profile이 저장한 expected port와의 연결 예측일 뿐, process ownership을
  주장하지 않는다.

correlation의 `action_key`는 opaque 값이다. owner action은 Run Manager task 또는 Workbench
profile을 native launch로 열고, Run correlation에서 `logs_available`가 true인 경우에만 stdout/
stderr Log Lens action을 제공한다. Log Lens handoff는 Run identity-only `{ kind, sourceId,
runId, stream }`만 전달하며 raw path·command·environment·log bytes는 snapshot, action payload,
argv 또는 handoff로 공유하지 않는다.

browser가 보관한 action key를 신뢰하지 않는다. owner/log action은 실행 직전에 native listener를
다시 수집하고 producer view를 다시 읽어 현재 correlation을 재검증한다. 기존 listener kill도
endpoint와 process identity를 native에서 다시 수집·비교한 뒤에만 수행한다. 이 관찰 경계는 자동
kill/restart를 실행하지 않는다. 동일한 선언을 다시 발행하는 주기적 producer heartbeat만으로는
action key를 회전시키지 않지만, listener identity나 target/run identity가 달라지면 즉시 달라진다.

refresh timeline은 session-only 메모리 상태다. 첫 성공 snapshot은 baseline으로만 사용해 event를
만들지 않고, 성공한 후속 snapshot의 endpoint/identity와 owner 설명 변화를 `opened`, `closed`,
`changed`, `owner-changed`로 관측 시각과 함께 기록한다. failed poll은 timeline·baseline을 변경하지
않으며, event는 최신 256개로 제한하고 저장하지 않는다. event에는 화면에 필요한 address,
process name, owner label만 축약하고 command line·executable path·action key는 보관하지 않는다.

이 cross-app correlation, owner navigation, Log Lens handoff는 v0.6.0 W08의 #493 hosted
Windows package gate를 통과했다. local/hosted 결과가 임의 사용자 PC의 모든 installed source
조합을 관찰했다는 뜻은 아니다.

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
