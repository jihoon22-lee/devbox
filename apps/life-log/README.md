# life-log — 자동 Life Log

하루의 PC·Git·파일 활동을 자동으로 모아 일일 로그와 통계를 만드는 앱. 활동 추적(activity-timeline)을 흡수해 별도 데이터 소스 설정이 필요 없다.
산출물: `LifeLog.exe` (`apps/life-log`).

## 주요 기능

- **일일 요약** — PC 사용시간, 앱별 사용, git 커밋 수, 생성 파일 수, 노트 수
- **캘린더 뷰** — 날짜 선택·이동, 일별 활동 타임라인
- **날짜 컨텍스트 메뉴** — 선택 날짜와 주·월 차트의 날짜를 우클릭 또는
  `Shift+F10`/Menu 키로 열어 정확한 `YYYY-MM-DD`를 복사한다. 메뉴를 닫으면 원래
  날짜 요소로 focus를 복원한다. 선택 날짜의 Markdown/JSON/CSV export를 제공하며,
  범위 export 버튼에서는 최대 366일의 시작·종료 날짜를 명시적으로 선택한다. 데스크톱은
  native 저장, 브라우저 빌드는 실제 local data가 없는 `browser-preview` 다운로드로
  구분한다.
- **기간 통계** — 주/월 사용량 차트, 앱 순위, 커밋 트렌드
- **git 프로젝트 연동** — git 경로 등록으로 커밋 집계
- **프로젝트 snapshot** — 등록 프로젝트와 최근 7일 활동의 숫자 요약을 Workbench용 `projects/v1` view로 발행
- **Knowledge 활동 source** — `knowledge-base/activity/v1`의 오늘 작성·수정 수와 최근 수정 시각을 Data Sources에 freshness와 함께 표시
- **로컬 export** — session·Git과 Run Manager·Knowledge source provenance를 같은
  `life-log/export/v1` 문서로 정렬해 Markdown/JSON/CSV로 만든다. export 시 현재 privacy
  규칙을 다시 적용하고, source의 producer/schema/snapshot version/generatedAt/freshness/
  named view/error code를 함께 기록한다. 현재 Run Manager/Knowledge snapshot은 요청 날짜와
  연결된 history가 아니므로 summary 수치에 섞지 않고 provenance만 기록한다.
  명백한 credential marker가 있는 session 제목도 `[redacted]`로 줄이며, snapshot 원문
  ID·credential·raw environment 값은 결과에 포함하지 않는다.

## 기술

- 백그라운드 폴러·세션 추적 → SQLite → React
- `crates/integration`의 자동 발견·검증 API로 모든 snapshot producer를 Data Sources에 표시 — 외부 DB 직접 조회 없음
- 공용 `packages/context-menu`는 위치·keyboard·focus만 공유하고 날짜 파싱·선택·
  clipboard·export 가용성은 Life Log가 소유
- native export는 DB·read-only integration snapshot·read-only `git log`만 사용하며
  cloud/LLM/network 전송을 하지 않는다. 브라우저 preview는 native DB/Git/snapshot을
  읽지 않고 네 source를 `browser_preview_only`로 표시한다. Markdown·JSON·CSV 모두
  고정된 row 순서와 source metadata를 사용해 같은 준비 snapshot/fixture에서 결정론적
  결과를 낸다. 브라우저 JSON/CSV preview도 같은 range/day/source 순서와 24열 CSV
  contract를 유지하며, 실제 데이터가 없다는 사실은 `RenderedExport.origin`과 source
  metadata로 구분한다.
- Git 조회는 검증된 absolute project path만 fixed argv로 `git log --format=%ct`에
  전달한다. `--since`/`--before`는 초 단위 보조 필터이고 최종 포함 조건은 정확한
  `[startMs, endMs)` millisecond 범위다. bounded child는 stdin을 `null`로 닫아
  interactive prompt를 차단하고, stderr는 버리며 subprocess별 timeout·stdout 상한을
  적용한다. 오류는 `git_timeout`, `git_output_too_large`, `git_failed` 같은 고정 code만
  남기고 path·remote URL·credential은 export/화면으로 반향하지 않는다.
- Windows 저장은 사용자가 native save dialog에서 경로와 overwrite를 확정한 뒤에만
  실행된다. 선택된 절대 경로의 parent/확장자를 검증하고 `crates/filesystem::atomic_write`
  로 sibling temporary file을 교체한다. 저장 직전에 byte length·format metadata와 JSON/CSV/
  Markdown artifact를 다시 parse/검증한다. 취소·잘못된 경로·저장 실패는 파일을 만들거나
  내부 경로/원문 오류를 화면에 반향하지 않는다. 브라우저 fixture에서는 명시적 download를
  사용하며 native path를 흉내 내지 않는다.
- range dialog와 날짜 context menu는 하나의 export busy guard를 공유한다. 진행 중에는
  날짜·형식 입력, 저장·취소·다른 날짜 action을 중복 변경·실행할 수 없고,
  native/브라우저 오류는 고정 안내만
  표시한다. pending 결과가 dialog가 닫혔거나 컴포넌트가 unmount된 뒤 도착하면 request
  token으로 폐기하며 detached UI를 갱신하지 않는다. dialog는 initial focus·Escape·양 끝
  Tab 순환·close 후 focus restore를 보장한다.
- CSV는 `record_type` 고정 header를 사용하는 단일 파일이다. summary/daily/app/git/session/
  source row를 순서대로 기록하고 RFC 4180 quote/CRLF escaping을 적용한다. source에는
  `snapshot_version`, `freshness_ms`, `view`도 고정 column으로 포함한다. JSON은 pretty
  output, Markdown은 source 규칙·범위·daily/session 표를 포함하며 native/browser
  Markdown cell 모두 pipe·역슬래시·backtick·개행을 안전하게 처리한다. 출력은 4 MiB, session은
  50,000건, 날짜 범위는 366일, 제목은 4 KiB로 제한한다. Run service는 256개/ID 256 bytes/
  uptime 100년 상한을 적용하고, Knowledge identifier는 512개/ID 128 bytes로 제한한다.
  Git source의 `errorCodes`와 각 JSON/Markdown/CSV project row의 `errorCode`는 bounded
  runner의 stable code만 담고, 오류가 있는 repository의 count는 0과 해당 code를 함께
  기록해 정상적인 0 commit repository와 구분한다.
- Knowledge의 `activity/v1` view는 producer·envelope/view schema, 단일 entry, 불투명 ID 형식·중복·개수 관계를 모두 검증한 뒤에만 사용한다. ID 자체는 frontend로 보내지 않는다
- Knowledge Base가 아직 구버전인 롤링 업그레이드 동안 기존 flat v1 통계도 읽되 `legacy-data` view로 구분한다. 손상·schema mismatch는 다른 source를 막지 않으며 producer version·generatedAt·freshness와 안전한 오류를 유지한다. 여러 version이 있으면 가장 높은 validated version 하나만 선택하고, 더 새로운 version이 손상/미지원이면 오래된 version으로 조용히 대체하지 않는다.
- 시작·프로젝트 변경·60초 주기로 `%LOCALAPPDATA%\devbox\integration\life-log\v1\summary.json`을 원자 교체
- `projects/v1` entry: `path`, `activityWindowStartMs`, `lastActivityAtMs`, `recentSessionCount`, `recentDurationMs`
- snapshot에는 창 제목·앱명·세션 원문·credential을 넣지 않으며, 상대·traversal·device/root 경로는 발행하지 않음

## 데이터

- `%LOCALAPPDATA%\com.devbox.lifelog\data.db` — 활동 세션 + 설정
- export는 사용자가 고른 대상 파일 외에 지속 상태를 만들지 않는다. export 문서의 날짜
  범위는 `start_ts >= start_ms && start_ts < end_ms`이고, 날짜별 행은 요청의
  `dayBoundaries`에 따라 session 시작 시각을 local civil-day에 배치한다. timezone/DST
  계산은 frontend가 만든 각 날짜의 epoch 경계를 authoritative input으로 전달하며 native가
  고정 24시간으로 재계산하지 않는다. 현재 Run Manager/Knowledge snapshot은 이력 DB를
  직접 읽지 않고 `latest-snapshot-out-of-range` source로 표시하며, 선택된 snapshot
  version·named view·generatedAt·capture freshness를 그대로 기록한다. 해당 latest 수치는
  요청 날짜 범위의 수치가 아니므로 `summary.run`/`summary.knowledge`에 넣지 않는다.

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

## Export wire contract (`life-log/export/v1`)

JSON의 최상위 순서는 `schemaVersion`, `range`, `rules`, `summary`, `daily`, `sessions`,
`sources`다. `rules`는 session window/session duration/daily bucket/privacy/app totals/Git/
snapshot 범위를 고정 문장으로 펼쳐 보여 준다. Session duration은 DB에 저장된 값을 보존하고
범위 경계에서 자르지 않으며, daily bucket은 입력으로 전달된 local civil-day 경계와 session
시작 시각으로 결정된다.
음수 ID/duration, 역전된 종료 시각처럼 손상된 session 숫자는 0으로 보정해 정상 자료처럼
표시하지 않고 고정 오류로 export 전체를 중단한다.
`range.endMs`는 exclusive이고 `range.timezone`은 입력을 만든 system-local timezone이다.
요청의 `dayBoundaries`는 각 local civil-day의 정확한 epoch 경계를 보존하므로 DST 전환일에도
날짜별 집계가 24시간 고정 폭으로 왜곡되지 않는다. `summary.appTotals`와 Git project는
duration/path의 deterministic 정렬을 사용하고, `sources`는 `life-log`, `git`, `run-manager`, `knowledge-base` 순으로
고정한다. source가 없거나 손상된 경우에도 전체 export를 실패시키지 않고 `available: false`
와 `no_safe_project_paths`, `snapshot_unavailable`/`snapshot_invalid`/
`snapshot_schema_unsupported`/`snapshot_payload_invalid`/`snapshot_changed_during_read`와
Git bounded runner code 같은 고정 error code만 기록한다. source metadata의
`schemaVersion`은 envelope schema, `snapshotVersion`은 실제로 선택해 읽은 version이며,
snapshot consumer가 요청한 named view는 `view`에 기록한다. `freshnessMs`는 export 당시
현재 시각으로 새로 산출한 값이 아니라 해당 snapshot/view reference가 보고한 capture age다.

Run Manager와 Knowledge의 현재 snapshot payload는 요청 범위에 해당하는 history가 아니므로
export summary에 포함하지 않는다. 대신 validated Run Manager flat/Knowledge `activity`
snapshot의 producer·schema/snapshot version, generatedAt, capture freshness, named view, out-of-range scope를
source row에 보존한다. 향후 range-scoped snapshot 계약이 생길 때만 해당 범위와 일치하는
digest를 summary에 포함할 수 있다. snapshot 원문에 있는 note ID·path·title·body는 export
boundary를 넘지 않는다. Git project path는
절대 경로·traversal/device path가 아닌 경우에만 포함하고, invalid path는 외부 command에
전달하지 않는다. common `filesystem::parse_safe_project_path`의 Windows drive/UNC/POSIX
규칙을 공유하며 최대 64개 unique project만 사용한다.

브라우저 결과의 `RenderedExport.origin`은 `browser-preview`이고 native 결과는 `native`다.
브라우저 preview의 모든 source는 `available: false`, `scope: browser-preview-only`,
`errorCode: browser_preview_only`이며, `life-log`를 포함해 native 성공이나 producer version을
가짜로 표시하지 않는다. 브라우저 download는 편의상 제공되는 범위/경계 preview일 뿐 native
DB export의 대체가 아니다.

## Local daily/weekly (+ existing monthly) digest (`life-log/digest/v1`)

일간·주간·월간 화면의 `Local digest`는 `get_digest` native command가 만드는 작은 요약 문서다.
입력은 `startDate`, `endDate`(civil date inclusive), `timezone`, `dayStart`, `dayEnd`(epoch
millisecond exclusive), 각 local civil day의 `dayBoundaries`, `period`(`day`, `week` 또는 `month`),
`filter.app`(없으면 `null`)으로 고정한다. native는 export와 동일하게 `dayBoundaries`를
authoritative input으로 사용하므로 DST 전환일을 고정 24시간으로 다시 계산하지 않는다. 일간은
정확히 하나의 local civil day, 주간은 월요일 시작 정확히 7일, 월간은 해당 월의 1일부터
마지막 날까지 28~31일만 허용하며, invalid date, non-contiguous boundary, 음수/역전 epoch,
과도한 범위는 DB·Git 조회 전에 고정 오류로 거부한다.

session은 기존 `life-log/export/v1`와 같은 bounded half-open SQL window(최대 50,000행)를
거치고 privacy/obvious credential marker를 재적용한 뒤 앱 exact filter를 적용한다. filter는
최대 256 UTF-8 bytes·control/credential marker 금지이며, 필터된 세션이 없으면 성공한 빈
digest(`0 sessions`, `0 active days`, 빈 app 목록)로 표시한다. app 합계는 duration 내림차순,
동률이면 UTF-8 byte 순으로 정렬하고 unique app은 2,048개까지다. 각 날짜에는 PC 사용량,
session 수, Git commit 수, top app, `hasActivity`를 제공하고 전체에는 평균 일일 사용량과
top app을 제공한다. 저장된 duration은 range에서 자르지 않고 시작 timestamp가 속한 boundary에
귀속한다.

Git은 export producer의 safe absolute project path·identity dedupe·fixed argv·null stdin·2초
timeout·256KiB stdout·폐기 stderr·stable error code를 그대로 공유한다. 앱 필터는 Git 결과에
영향을 주지 않으며, Git 오류 project row에는 count 0과 고정 code를 남긴다. Run Manager와
Knowledge는 현재 range-keyed history가 아닌 latest local snapshot이므로 digest 수치에 섞지
않고 `sources`에 producer/schema/snapshot version, generatedAt, freshness, named view와
`latest-snapshot-out-of-range` scope만 보존한다. source 순서는 `life-log`, `git`,
`run-manager`, `knowledge-base`로 고정하며, snapshot 원문의 note ID/path/title/body와 raw
환경 값은 경계를 넘지 않는다.

응답은 `schemaVersion: 1`, `period`, `range`, `filter`, `rules`, 고정 문장 `headline`,
`summary`, `daily`, `appTotals`, `git`, `sources`, 그리고 동일 입력에서 재현 가능한 Markdown
`markdown`을 포함한다. Markdown에는 Summary/Daily digest/Applications/Git projects/
Sources/Rules를 항상 포함하고 데이터가 없으면 명시적 empty 문장을 쓴다. 규칙 문장은 session
window·duration·DST bucket·privacy·app filter·Git·snapshot scope·외부 처리 금지를 펼쳐서
설명한다. 전체 document와 Markdown은 각각 4MiB 이하이며, serializer/renderer 오류와 내부
path·OS·Git stderr·원문 credential은 frontend에 반향하지 않는다.

`Copy digest`는 사용자가 누른 현재 Markdown만 OS clipboard에 한 번 기록하고 history·storage·
telemetry를 만들지 않는다. `Save digest`는 Windows native save dialog에서 사용자가 확정한
`.md` 경로에만 sibling temporary file + `atomic_write`로 기록하며 cancel/invalid path/
corrupt response는 파일을 만들지 않는다. 브라우저 빌드는 DB·Git·snapshot을 읽지 않고
`origin: browser-preview`와 네 source의 `browser_preview_only`를 포함한 동일한 구조의 preview를
명시적 Download로만 제공한다. period 전환·앱 필터 변경·새로고침은 request token으로 stale
응답을 폐기하고, busy 중 duplicate action을 막으며, copy/save 버튼·filter label·live status와
기존 keyboard/IME 동작을 유지한다. 자동 일기 문장 생성, cloud/local LLM, network fetch와
개인 활동 원문 외부 전송은 이 기능에 포함하지 않는다.
