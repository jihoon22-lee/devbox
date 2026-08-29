# run-manager — Run Manager (예약 실행·서비스 관리)

예약 실행(크론 잡)과 상시 실행(서비스)을 한 곳에서 관리하는 앱.
산출물: `Run Manager.exe` (`apps/run-manager`).

## 주요 기능

- **작업(cron job)** — 이름, 명령, 작업 디렉터리, 환경변수, 실행 대상(Windows/WSL 배포판), cron 빌더 + 다음 실행 시각 미리보기
- **실행 정책** — 중복 실행 정책(skip/queue/kill-previous), occurrence 원자적 claim
- **서비스(service)** — start/stop/restart·자동 시작·재시작 정책(never/on-failure/always)·백오프·헬스체크(프로세스 생존/로컬 TCP)
- **관찰성** — 실행 이력, stdout/stderr 회전 로그 tail·검색, 실패 Windows toast 알림
- **실행 이력 필터** — 작업/서비스 종류, 대상, 상태, 시작·종료 날짜, 실행 시간(최대 30일) 조합
- **로컬 task import** — `package.json` scripts와 `Cargo.toml`의 로컬 target을 native parser로 미리보기 후 비활성 draft로 저장
- **행 컨텍스트 메뉴** — 작업·서비스·실행 이력의 우클릭/Shift+F10/Menu key 메뉴, 대상 행 우선 선택, 닫힌 뒤 focus 복구
  - 작업: 지금 실행, 활성화/비활성화, 편집, 로그 열기, 확인 후 삭제
  - 서비스: 실제 초기 인스턴스 상태에 따른 시작/정지/재시작, 편집, 정지 상태에서만 확인 후 삭제
  - 실행 이력: 로그 보기, 해당 작업 재실행, 현재 stdout/stderr 스트림 로그 저장, 확인 후 Log Lens 읽기 전용 열기
- **안전한 lifecycle action** — 작업 활성 실행과 서비스 정지는 모든 UI 진입점에서 확인하고, 활성 작업 삭제와 정지 중이거나 snapshot을 확인할 수 없는 서비스 lifecycle 변경은 fail-closed로 비활성화
- **재시도 제어** — `retry_waiting` 서비스의 명시적 정지는 예약된 backoff를 취소하고, 재시작은 대기 시간을 건너뛰어 새 generation을 시작
- **제한된 로그 저장** — backend가 run ID로 해석한 app-owned 회전 로그만 decimal cursor로 읽고, 현재 스트림을 최대 50MiB까지 저장. 파일명에는 bounded opaque run ID만 사용하며 명령·경로·환경변수를 넣지 않음
- **실행 어댑터** — Windows(Job Object)·WSL(session/group), DPAPI 환경변수 보호

## Integration snapshot 계약 (#474)

이 계약 보강은 v0.5.0 tag 이후의 post-release source correction이며 공개 v0.5.0 binary에는
포함되지 않는다.

Run Manager는 `crates/integration`의 versioned atomic snapshot을 주기적으로 발행한다.
`run-manager/v1/summary.json`은 기존 flat status payload를 정확히 유지한다. 즉
`activeServices`·`runs`·`lastRunAtMs`만 가진 `data`를 기록하므로 기존 Workbench/Life Log와
구버전 Launcher가 새 producer를 계속 읽을 수 있다. 새 named capability는 같은 producer와
version directory 안의 `run-manager/v1/jobs-services.json` sidecar에 발행한다. sidecar
envelope schema와 `jobs-services` view schema는 각각 1이며, named view에는 그 view 하나만
담는다. entry는 `id` 기준으로 정렬되고 전체 항목은 2,048개 이하로 제한한다.

`jobs-services` entry는 `id`, 안전한 표시용 `label`/`detail`,
`targetApp: "run-manager"`, `targetKind: "task"`, `payloadVersion: 1`,
`payload: { "id": "..." }`만 가진다. payload에는 command, cwd, environment 값이나
설정 여부, path, credential, log 원문을 복사하지 않는다. 저장 데이터에 제어문자·과도한
길이·credential 형태가 있는 label은 고정된 fallback label로 대체하고, 잘못된/중복 ID나
범위 초과 데이터는 snapshot 전체를 갱신하지 않아 마지막 정상 파일을 보존한다.

Workbench와 Life Log는 v1 flat status를 계속 읽는다. 새 Launcher는 named
`jobs-services.json` sidecar를 우선 사용하고, sidecar가 없는 구버전 producer에서는 v1 flat
`activeServices` fallback을 사용한다. 따라서 새 Launcher+구버전 Run Manager도 기존
active-service 결과를 유지하고, 새 producer+새 Launcher에서는 전체 job/service action을
검색할 수 있다. sidecar가 overflow 또는 projection 오류로 갱신되지 않으면 v1 status는 먼저
성공하고 sidecar의 last-good 파일은 atomic 경계에서 보존된다.

## 실행 이력·task import 계약 (#357/#358)

이력 조회는 하나의 parameterized SQLite query로 작업과 서비스 run을 함께 필터링한다. 날짜는
epoch milliseconds 반열린 범위이며, 아직 끝나지 않은 run의 duration은 조회 시각을 기준으로
계산한다. query ID·기간·duration·limit은 native 경계에서 상한과 순서를 검증하고, 결과는 기존
`RunView`의 redacted DTO만 반환한다. 로그 파일이나 환경변수는 이력 필터에서 읽지 않는다.

task import는 선택한 프로젝트 루트 바로 아래의 `package.json`과 `Cargo.toml` 내용만 bounded
read한다. npm/Cargo/shell/network/.env를 실행하거나 읽지 않으며, script body와 environment 값은
저장하지 않고 환경 키 이름만 preview에 표시한다. Cargo 자동 target을 판정할 때는 Cargo를
실행하지 않고 `src/lib.rs`, `src/main.rs`, `src/bin`, `examples`, `tests`, `benches`의 표준
layout을 fixed-depth·bounded metadata로만 확인한다. Rust source 내용이나 workspace member의
다른 `Cargo.toml`은 읽지 않는다.

target name은 제한된 문자 집합으로 검증하고 생성된 `npm run -- <name>`/`cargo run|test|bench
--...` 명령과 canonical cwd를 확인한다. 모든 항목은 사용자 승인 전 `enabled=false` draft로
저장된다. Cargo의 `autolib`/`autobins`/`autoexamples`/`autotests`/`autobenches`와 edition별
자동 discovery 기본값을 적용하며, 명시 target과 자동 target은 `(kind, name, path)` 기준으로
중복 제거한다. 명시 target의 파일이 없거나 target path가 root 밖·symlink/reparse point이면
fail-closed 한다. 자동 발견 binary도 항상 `cargo run --bin <name>`을 사용해 bare `cargo run`을
만들지 않는다. non-bin example과 `required-features` target은 실행 task로 만들지 않는다.

layout metadata snapshot도 opaque revision에 포함해 preview와 apply 사이의 target 추가·삭제·교체를
stale로 거부한다. VS Code `tasks.json` parsing과 virtual workspace member 탐색은 이 후보의
범위가 아니며, 정의 export/import의 별도 후속 요구로 남긴다.

preview에는 root filesystem identity를 포함한 SHA-256 opaque source revision이 붙는다. 적용 시
파일·root를 다시 읽어 revision을 비교하고 변경되었거나 안전하지 않으면 고정된 stale 오류로
중단한다. 같은 kind/name/cwd 충돌은 Windows 경로의 대소문자·separator alias까지 정규화해
건너뛰며, 선택된 project batch와 기존 definition JSON batch 모두 SQLite 한 transaction으로
저장한다. duplicate operation ID, cooperative cancel, 5초 native budget, 512KiB/file·128
item·4KiB root 상한을 적용한다. project cancel은 transaction commit 전까지 각 경계에서
확인되며 취소가 관찰되면 전체 batch를 rollback한다. definition JSON 저장은 bounded
non-cancellable operation으로 처리 중에는 취소를 가장하지 않는다. cancel/close는 preview
결과를 폐기하며, 이미 커밋된 transaction은 되돌리지 않는다.

기존 Run Manager SQLite schema v2의 `jobs`/`meta` 구조를 그대로 사용하므로 별도 column migration
없이 기존 DB에서 새 필터와 disabled draft를 읽고 쓸 수 있다. import preview schema는 DB schema와
독립적인 version 1이며, migration은 시작 시 기존 idempotent migration으로 계속 수행된다.

## 기술

- 공용 크레이트 `crates/wsl`·`crates/secrets`·`crates/integration`, 프로세스/서비스 실행은 자체 구현(`src-tauri/src/platform/`)
- 트레이 상주 + 백그라운드 tokio 루프 + SQLite
- `packages/diff-view`·`packages/context-menu`

## 로그 검색 계약 (#311)

실행 이력에서 선택한 run의 현재 보존 범위(stdout/stderr)를 대상으로 명시적으로 검색할 수
있다. 검색은 사용자가 `검색`을 누르거나 검색어에서 Enter를 누른 경우에만 시작하며, 입력 중
매 키 stroke마다 파일을 읽지 않는다.

- 기본 방식은 **literal**이다. `+`, `*`, `[` 같은 문자는 그대로 찾으며 정규식은 방식에서
  `regex`를 명시적으로 선택했을 때만 컴파일한다. Rust `regex` 엔진의 선형 시간 실행과
  bounded program budget을 사용하므로 catastrophic backtracking 엔진을 실행하지 않는다.
- `source`는 임의 경로나 원격 수집원이 아니라 이 run의 `stdout` 또는 `stderr` stream
  adapter다. `level`은 줄 앞의 `trace`/`debug`/`info`/`warn`/`error` 토큰(선택적 RFC3339
  timestamp 뒤 토큰 포함)을 best-effort로 인식한다. `startAt`/`endAt`은 epoch milliseconds
  반열린 시간 범위이며, timestamp가 없는 줄은 해당 run의 시작 시각을 기준으로 판정한다.
- 결과는 원문을 복제하지 않고 `log-source/v1`의 opaque `sourceId`, stream, 보존 snapshot
  기준 1-based line number, 인식된 level/timestamp만 반환한다. 선택 결과는 이전/다음 버튼과
  결과 목록으로 해당 stream·줄을 탐색하며, 화면에 남아 있는 로그 줄만 자동으로 scroll한다.
- 한 요청의 query/regex는 UTF-8 512바이트, 한 stream scan은 4MiB, 전체 scan은 8MiB,
  record는 16KiB/50,000줄, 결과는 500개로 제한한다. 상한에 닿으면 응답의 `truncated`를
  표시하며 writer를 붙잡은 채 전체 검색하지 않고 256KiB tail chunk 사이에 scheduler에
  양보한다. 동기 파일 metadata 복원과 bounded regex/text scan은 blocking worker에서 수행한다.
  rotation으로 cursor가 stale해지면 한 번만 현재 보존 경계에서 재시작한다.
- 잘못된 query/range/regex/source와 읽기 실패는 고정된 오류 코드·문구만 사용한다. query,
  log line, credential, 환경변수, 절대 경로는 오류나 검색 결과에 반향하지 않는다. 로그 본문은
  SQLite·telemetry·remote archive에 저장하지 않으며 기존 app-owned 회전 파일과 bounded
  viewer를 통해서만 읽는다.
- request와 `log-source/v1` DTO는 알 수 없는 field를 거부하므로 `absolutePath` 같은 추가
  경로를 payload에 숨길 수 없다. 모든 epoch-millisecond 입력·출력은 JavaScript safe integer
  범위 안에서만 WebView 경계를 통과한다.
- `log-source/v1` source reference는 `{ kind, sourceId, runId, stream }`으로만 발행한다.
  `sourceId`는 `run-manager:<run-id>:<stdout|stderr>`와 exact 일치하며, 실행 명령·cwd·환경변수·
  credential·절대 경로·로그 원문은 payload와 argv에 들어가지 않는다. 사용자가 실행 이력의
  `Log Lens` 버튼/메뉴에서 확인한 경우에만 공용 handoff store에 10분 TTL envelope을 만들고,
  AppLink argv에는 opaque kind/id만 전달한다. 발행 전에 DB의 `log_dir`는 canonical app-owned
  `logs/runs/<run-id>` 경로로 다시 resolve하며, 디렉터리가 없거나 소유 경계를 벗어나면 고정
  `logs-unavailable` 오류로 중단한다. Log Lens는 producer path나 DB를 받지 않고 같은 bounded
  identity를 고정 Run Manager app-data root에 다시 해석해 선택 stream의 회전 segment만 읽는다.
- Log Lens는 envelope을 claim한 뒤 source summary를 미리보기로 보여 준다. 사용자가 `읽기 전용
  source 추가`를 누를 때만 ack하고 지원되는 fixed adapter로 넘기며, 취소/실패는 restore한다.
  Run source reader는 logical offset cursor로 append를 이어 읽고 retention rotation/truncate를
  표시한다. 수동 검색·tail과 handoff source 모두 permanent archive를 만들지 않는다.
- handoff publish와 Log Lens launch는 producer 프로세스에서 single-flight로 직렬화한다. 이미
  같은 흐름을 처리 중이면 고정 `handoff-busy` 오류를 반환해 중복 envelope·중복 창 생성을 막고,
  launch 실패 시에는 방금 만든 exact pending envelope을 안전하게 제거한다. raw payload나
  경로는 오류에 노출하지 않는다.

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: `docs/superpowers/specs/2026-08-12-run-manager-design.md`
