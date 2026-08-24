# Knowledge Activity Snapshot과 Life Log Source 연결

## Overview

Issue #247의 P1-05-K 범위로 catalog에 선언돼 있었지만 consumer가 없던
`snapshot:knowledge-base/activity/v1`을 실제 앱 간 데이터 흐름으로 연결했다.
Knowledge Base는 오늘 작성·수정된 note의 숫자 요약과 경로 없는 불투명 식별자를 공용
integration root에 원자 발행한다. Life Log는 이 view를 다시 검증한 뒤 Data Sources 화면에
오늘의 note 수, 최근 수정 시각, snapshot freshness 또는 격리된 안전 오류를 표시한다.

Life Log는 Knowledge Base의 app-local SQLite나 note 파일을 직접 읽지 않는다. producer가
종료돼도 마지막 정상 snapshot을 읽을 수 있고, Knowledge snapshot 하나가 손상돼도 다른
producer의 발견과 표시는 유지된다. v0.4.x Knowledge Base가 남아 있는 롤링 업그레이드 동안은
기존 flat envelope도 제한적으로 읽는다.

이번 PR은 Knowledge capture/template/wikilink, Life Log export, Workbench preflight와 앱 version
bump를 포함하지 않는다. 각각 독립 기능 또는 release preparation 범위로 남긴다.

## Context

- `apps/catalog.json`은 이미 Knowledge Base가 `snapshot:knowledge-base/activity/v1`을 생산한다고
  선언했지만, 기존 producer는 `data`에 `notesModifiedToday`와 `lastModifiedAtMs`만 쓰는 flat
  envelope이었다.
- #244에서 Life Log Data Sources가 공용 `crates/integration` discovery 결과를 동적으로
  표시하게 됐지만, producer metadata만 보였고 Knowledge 업무 payload는 읽지 않았다.
- 계획 문서의 계약은 작성·수정 수와 note 식별자를 요구한다. note 경로나 본문을 전달하면
  Life Log가 Knowledge의 저장 구조와 privacy boundary를 침범한다.
- Knowledge 내부 변경 command 중 write/create만 snapshot을 갱신했다. rename/delete/daily note와
  외부 editor watcher 경로는 갱신하지 않아 실제 활동과 마지막 snapshot이 달라질 수 있었다.
- producer와 consumer가 동시에 실행될 수 있으므로 파일별 append나 부분 update 대신 기존
  integration crate의 완성 envelope 원자 교체를 유지해야 한다.

## Contract Decision

Knowledge Base envelope v1의 `data.views.activity`에 schema v1 entry 하나를 둔다.

```json
{
  "notesModifiedToday": 2,
  "lastModifiedAtMs": 1800000000000,
  "noteIds": ["note-7", "note-2"],
  "identifiersTruncated": false
}
```

- `notesModifiedToday`: snapshot 생성 시점의 UTC day 안에서 마지막으로 작성·수정된 note 수
- `lastModifiedAtMs`: DB 전체 note 중 가장 최근 수정 시각. 오늘 활동이 0이어도 과거 값은 유지
- `noteIds`: SQLite row id에서 만든 `note-<양의 정수>` 형식의 경로 없는 식별자
- `identifiersTruncated`: 오늘 note가 512개를 넘어서 ID 목록을 잘랐는지 표시
- `activity` view `freshnessMs`: 생산 시점에는 0이며 discovery가 파일 경과 시간을 합산

row id는 note 경로·제목·본문·tag를 포함하지 않고 snapshot 내부에서 개수 일관성과 중복을
검증하기 위한 불투명 값이다. Life Log backend까지만 읽고 frontend DTO에는 ID 자체를 넣지
않는다.

기존 v0.4.x flat v1은 `views` key가 아예 없을 때만 legacy로 해석한다. `views`가 있는데
`activity`가 없거나 schema가 다르면 legacy로 후퇴하지 않고 명시적 오류로 처리한다. 이 구분은
새 계약의 malformed snapshot을 구버전 데이터로 오인하는 것을 막는다.

## Changes Made

### 1. Knowledge activity/v1 producer

File: `apps/knowledge-base/src-tauri/src/integration.rs`

- `Envelope::with_views`, `SnapshotView`, `SnapshotViews`를 사용해 flat data를 versioned
  `activity` view로 전환했다.
- count query를 `modified_ts >= today_start AND modified_ts <= now`로 제한해 미래 timestamp가
  오늘 count에 들어오지 않도록 했다.
- 전체 `MAX(modified_ts)`는 오늘 활동이 없을 때도 최근 Knowledge 사용 시각을 보여 주기 위해
  별도로 유지한다.
- 동일 범위의 DB row id를 수정 시각 내림차순, id 오름차순으로 안정 정렬하고 최대 512개만
  `note-<id>`로 직렬화한다.
- SQL/직렬화 실패는 query, path, content를 반향하지 않는 고정된 오류로 변환한다.
- test root를 주입하는 `write_snapshot_in`과 결정적인 `now_ms` 경로로 실제 원자 writer와
  discovery를 같은 fixture에서 검증한다.

### 2. 모든 Knowledge 변경 경로의 snapshot refresh

Files:

- `apps/knowledge-base/src-tauri/src/commands/docs.rs`
- `apps/knowledge-base/src-tauri/src/commands/watcher.rs`

기존 write/create refresh를 유지하고 다음 성공 경로에도 best-effort write를 추가했다.

- note rename/move 후 old row 제거와 new row index가 끝난 시점
- note delete와 index 제거가 끝난 시점
- 존재하지 않던 오늘 daily note를 생성·index한 시점
- watcher debounce가 외부 editor의 생성·수정·삭제를 DB에 반영한 시점

모든 경로는 DB `MutexGuard`를 먼저 해제한 뒤 snapshot writer가 다시 잠금을 얻는다. 같은 mutex를
쥔 채 재잠금하는 deadlock을 만들지 않으며, snapshot 실패가 이미 성공한 note 저장을 되돌리거나
막지 않는다.

### 3. Life Log의 strict Knowledge consumer

File: `apps/life-log/src-tauri/src/commands/life.rs`

- production과 fixture가 같은 root를 쓰도록 `discover_report_in`과 `source_statuses_in` 경로를
  사용한다.
- Knowledge 이외 producer는 기존 generic SourceStatus 동작을 유지한다.
- Knowledge envelope version 1, `activity` view schema 1, entry 정확히 1개를 요구한다.
- payload는 safe fixed field만 deserialize하고 다음 의미 검증을 적용한다.
  - `lastModifiedAtMs`는 null 또는 0 이상의 epoch millisecond
  - 오늘 count가 양수면 최근 수정 시각이 반드시 존재
  - ID는 최대 512개, `note-` 뒤에 leading zero 없는 양의 십진수
  - ID는 모두 유일
  - truncated=false이면 count와 ID 수가 같음
  - truncated=true이면 count가 ID 수보다 큼
- validation을 통과한 ID 수만 `identifiedNotes`로 계산하고 note ID 문자열은 버린다.
- 새 view는 view freshness를, legacy flat envelope은 envelope/file freshness를 UI DTO에 보존한다.
- schema/payload 오류가 나도 producer version, generatedAt, freshness가 discovery에서 확인된
  상태라면 해당 진단을 유지한 unavailable row를 만든다.
- corrupt JSON처럼 discovery 자체가 실패한 producer는 기존 `SnapshotIssue` 격리 경로를
  사용하므로 다른 source row를 숨기지 않는다.

### 4. Life Log Data Sources UI

Files:

- `apps/life-log/src/api.ts`
- `apps/life-log/src/App.tsx`
- `apps/life-log/src/App.css`
- `apps/life-log/src/App.test.ts`

`SourceStatus`에 nullable `knowledgeActivity`를 추가하고 browser mock에도 실제 Knowledge row를
제공했다. `DataSourceRow`를 별도 export해 다음 상태를 같은 행에서 표시한다.

- producer/envelope version과 producer version
- 마지막 snapshot 또는 view freshness
- 정상 Knowledge source의 오늘 작성·수정 note 수와 최근 수정 시각
- legacy flat source 표시
- bounded ID가 일부만 포함된 경우 backend가 전달한 식별자 수
- unavailable source의 안전한 오류. 오류 상태에서도 확인 가능한 version/freshness는 숨기지 않음

React에는 count와 시각만 전달되며 note ID, path, title, body, tag는 interface에 존재하지 않는다.

### 5. Architecture와 앱 문서

Files:

- `apps/knowledge-base/README.md`
- `apps/life-log/README.md`
- `docs/architecture.md`

producer payload, 512-ID 상한, 갱신 trigger, legacy fallback, 오류 격리, stale 정책과
Knowledge DB 직접 조회가 없다는 경계를 현재 동작으로 기록했다. catalog 계약 문자열은 이미
정확해 `apps/catalog.json` revision을 불필요하게 올리지 않았다.

## Security and Failure Boundaries

- producer JSON에는 note path, title, body, tag, credential 또는 raw environment를 넣지 않는다.
- 공용 integration writer/reader가 10 MiB 상한, producer/path/version/generatedAt, JSON depth,
  symbolic link/reparse point와 민감 field/value 검사를 양쪽에서 적용한다.
- Life Log는 `%LOCALAPPDATA%\com.devbox.knowledgebase\data.db`를 알지 못하며 `docs` SQL을
  실행하지 않는다.
- note ID는 backend validation을 통과해도 frontend나 오류 문자열로 전달하지 않는다.
- malformed ID와 payload를 오류에 반향하지 않고 고정된 한국어 진단으로 바꾼다.
- 새 multi-view snapshot의 activity 누락/schema mismatch는 legacy fallback으로 숨기지 않는다.
- 손상 Knowledge producer는 다른 producer discovery를 중단하지 않는다.
- stale snapshot은 corruption이 아니다. producer가 꺼졌을 때도 마지막 정상 집계는 표시하되
  계산된 freshness를 함께 보여 준다.
- snapshot write는 note CRUD의 best-effort 후속 동작이다. integration 저장 실패 때문에 사용자의
  note 변경 자체를 실패로 바꾸지 않는다.

## Verification Results

### Knowledge producer와 Life Log consumer

```text
$ CARGO_BUILD_JOBS=1 cargo test -p knowledge-base -p life-log -j1
knowledge-base: 26 passed; 0 failed
life-log:       47 passed; 0 failed
```

producer fixture는 다음을 검증한다.

- 오늘 범위 count와 안정 정렬된 opaque ID
- 이전 날 note 제외와 전체 최근 수정 시각 유지
- 513개 note에서 ID 512개 상한과 truncation
- 실제 `summary.json` discovery에서 activity/v1 metadata 노출
- 직렬화된 snapshot에 path와 body가 없음

consumer fixture는 다음을 검증한다.

- 정상 activity/v1 집계와 view freshness 전달
- legacy flat v1 롤링 호환
- unsupported view schema에서 metadata/freshness와 안전 오류 유지
- path처럼 조작된 ID와 중복 ID 거부, raw value 비반향
- corrupt Knowledge JSON과 정상 Run Manager source의 동시 격리
- integration root 오류와 0-source 상태

### Compile and strict lint

```text
$ CARGO_BUILD_JOBS=1 cargo check -p knowledge-base -p life-log -j1
Finished `dev` profile

$ CARGO_BUILD_JOBS=1 cargo clippy \
    -p knowledge-base -p life-log --all-targets -j1 -- -D warnings
Finished `dev` profile
```

### Life Log frontend

```text
$ NODE_OPTIONS=--max-old-space-size=1024 \
    pnpm --filter life-log exec vitest run --passWithNoTests --maxWorkers=1
Test Files  1 passed (1)
Tests       12 passed (12)

$ NODE_OPTIONS=--max-old-space-size=1024 pnpm --filter life-log build
33 modules transformed
built successfully
```

추가 UI fixture는 정상 Knowledge activity count/last/freshness 표시와 schema 오류 행에서
version/freshness/error 동시 표시를 검증한다. worktree에 잠시 연결한 root/Life Log
`node_modules` symlink는 command trap으로 제거했고 잔여 pnpm/Vitest/Vite 프로세스가 없음을
확인했다.

### Repository policy and boundary audit

```text
$ cargo fmt --all -- --check
exit 0

$ python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml

$ python3 .github/scripts/test-check-dependencies.py
dependency policy regression tests passed

$ python3 .github/scripts/test-build-manifest.py
build-manifest notice tests passed

$ bash .github/scripts/check-catalog.sh
exit 0

$ rg 'com\.devbox\.knowledgebase|knowledge-base.*data\.db|SELECT.*FROM docs' apps/life-log
no matches
```

Cargo/pnpm dependency와 lockfile은 바뀌지 않았다. 전체 workspace Linux/Windows matrix는 로컬에서
중복 실행하지 않고 PR의 GitHub Actions 6개 gate를 권위 있는 검증으로 사용한다.

## Concentrated Review

PR 직전 한 번의 전체 review에서 다음을 확인했다.

- 계획서와 issue의 producer/consumer, freshness/error UI, fixture acceptance 충족
- Knowledge capture/template/wikilink, Life Log export 등 비범위 기능 미혼입
- app-local DB 직접 접근이나 새 외부 dependency 없음
- note path/content/credential 비직렬화와 ID frontend 비노출
- rename/delete/daily/watcher까지 snapshot lifecycle 완결
- legacy fallback이 새 malformed view를 삼키지 않음
- corrupt source 격리와 stale source usability 유지
- 임시 링크·빌드·test process cleanup과 `git diff --check` 통과

## Remaining Checkpoint

- 실제 Windows packaged Knowledge CRUD/watcher → snapshot write와 Life Log Settings 화면의
  count/freshness/error evidence는 나머지 P1 merge 뒤 계획서 §8.3 W1 checkpoint에서 수행한다.
- Knowledge capture, template, wikilink/graph는 각각 후속 Knowledge issue 범위다.
- Life Log export/retention UX와 Workbench preflight/template/retry는 별도 PR로 유지한다.
- Knowledge Base와 Life Log의 v0.5.0 version 원본 3종 bump는 각 앱의 마지막 기능 또는 release
  preparation PR에서 함께 처리한다.
