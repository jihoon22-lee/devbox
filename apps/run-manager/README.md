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
  - 실행 이력: 로그 보기, 해당 작업 재실행, 현재 stdout/stderr 스트림 로그 저장
- **안전한 lifecycle action** — 작업 활성 실행과 서비스 정지는 모든 UI 진입점에서 확인하고, 활성 작업 삭제와 정지 중이거나 snapshot을 확인할 수 없는 서비스 lifecycle 변경은 fail-closed로 비활성화
- **재시도 제어** — `retry_waiting` 서비스의 명시적 정지는 예약된 backoff를 취소하고, 재시작은 대기 시간을 건너뛰어 새 generation을 시작
- **제한된 로그 저장** — backend가 run ID로 해석한 app-owned 회전 로그만 decimal cursor로 읽고, 현재 스트림을 최대 50MiB까지 저장. 파일명에는 bounded opaque run ID만 사용하며 명령·경로·환경변수를 넣지 않음
- **실행 어댑터** — Windows(Job Object)·WSL(session/group), DPAPI 환경변수 보호

## 실행 이력·task import 계약 (#357/#358)

이력 조회는 하나의 parameterized SQLite query로 작업과 서비스 run을 함께 필터링한다. 날짜는
epoch milliseconds 반열린 범위이며, 아직 끝나지 않은 run의 duration은 조회 시각을 기준으로
계산한다. query ID·기간·duration·limit은 native 경계에서 상한과 순서를 검증하고, 결과는 기존
`RunView`의 redacted DTO만 반환한다. 로그 파일이나 환경변수는 이력 필터에서 읽지 않는다.

task import는 선택한 프로젝트 루트 바로 아래의 `package.json`과 `Cargo.toml`만 bounded read한다.
npm/Cargo/shell/network/.env를 실행하거나 읽지 않으며, script body와 environment 값은 저장하지
않고 환경 키 이름만 preview에 표시한다. target name은 제한된 문자 집합으로 검증하고 생성된
`npm run -- <name>`/`cargo run|test|bench --...` 명령과 canonical cwd를 확인한다. 모든 항목은
사용자 승인 전 `enabled=false` draft로 저장된다.
Cargo의 `autobins=false`는 자동 발견되는 기본 `cargo run`을 만들지 않으며, 명시적인 `[[bin]]`은
`name`이 없을 때 안전한 상대 `path`의 파일명에서 target 이름을 추론한다. 선택 ID는 preview에
실제로 존재하는 항목만 허용해 임의 항목을 apply할 수 없게 한다. VS Code `tasks.json` parsing은
이 후보의 범위가 아니며, 정의 export/import의 별도 후속 요구로 남긴다.

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
- `log-source/v1` source reference를 local boundary에서 검증하지만 이 PR은 Log Lens
  producer/handoff, remote logs, permanent archive를 연결하지 않는다. 해당 연결은 Log Lens
  bootstrap 이후 별도 integration PR에서 수행한다.

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: `docs/superpowers/specs/2026-08-12-run-manager-design.md`
