# Life Log 프로젝트 Snapshot Producer

## Overview

Issue #245의 P1-05-L 범위로 Life Log가 등록 프로젝트와 최근 활동의 privacy-safe 요약을
`snapshot:life-log/projects/v1`으로 발행하도록 구현했다. Life Log 내부 SQLite는 producer만
읽고, consumer는 `%LOCALAPPDATA%\devbox\integration\life-log\v1\summary.json`의 versioned
`projects` view만 읽는다. 앱 시작, 프로젝트 설정 변경, 60초 주기마다 완성된 view 전체를
원자 교체한다.

Workbench의 Life Log DB 직접 reader 제거와 snapshot 소비 전환은 후속 #246에 남겼다. 이
PR은 consumer 프로필 저장소나 Life Log의 export/digest, 원문 활동 저장을 변경하지 않는다.

## Context

- Workbench는 Life Log의 app-local `data.db`와 `settings` schema를 직접 알아야만 프로젝트를
  흡수할 수 있어 앱 소유권 경계가 깨져 있었다.
- 선행 #244에서 공용 discovery와 multi-view envelope은 마련했지만 Life Log가 실제로 소유한
  `projects` view는 아직 없었다.
- 등록 경로만 전달하면 최근 프로젝트 활동이라는 계획 요구를 충족하지 못하고, 반대로 창
  제목이나 세션 원문을 전달하면 privacy/credential 경계가 깨진다.
- 프로젝트 이름이 같은 여러 경로, 중첩된 이름, relative/traversal/device/root 경로와 손상된
  SQLite 행을 producer 단계에서 fail-safe로 다뤄야 했다.

## Changes Made

### 1. Privacy-safe 프로젝트 활동 집계

File: `apps/life-log/src-tauri/src/core/project_snapshot.rs`

- 등록 프로젝트를 최대 512개, 경로당 4,096 bytes로 제한하고 안전한 절대 Windows, UNC,
  POSIX 경로만 대상으로 삼는다.
- relative path, `.`/`..` component, Windows device path, drive/UNC/POSIX root, control character,
  Windows trailing dot/space·reserved device name·ADS·wildcard alias를 snapshot에서 제외한다.
- Windows slash/case 차이는 같은 identity로 deduplicate하고 첫 번째 사용자 표기를 보존한다.
- 최근 7일 세션을 SQLite index 범위로 streaming 조회해 메모리에 원문 목록을 적재하지 않는다.
- 기존 longest-basename 규칙으로 창 제목을 프로젝트에 내부 귀속하되, 같은 basename이 여러
  경로에 있으면 거짓 귀속을 피하기 위해 모두 미귀속한다.
- 각 프로젝트에는 `path`, `activityWindowStartMs`, `lastActivityAtMs`,
  `recentSessionCount`, `recentDurationMs`만 남긴다. app/title/session 원문은 반환 타입에 없다.
- 미래·역전 timestamp와 과대 duration은 현재 집계 window 안으로 clamp하고, 유효한 구간이
  없는 행은 건너뛴다. DB/schema 오류는 원문을 반향하지 않는 고정 오류로 반환한다.
- 최근 활동이 있는 프로젝트를 먼저, 나머지는 path 순으로 정렬해 consumer 결과를 안정화했다.

### 2. Versioned multi-view producer와 lifecycle

Files:

- `apps/life-log/src-tauri/src/integration.rs`
- `apps/life-log/src-tauri/src/lib.rs`
- `apps/life-log/src-tauri/src/commands/life.rs`
- `apps/life-log/src-tauri/src/commands/tracking.rs`
- `apps/life-log/src-tauri/src/core/mod.rs`

`Envelope::with_views("life-log", version, views)`에 schema v1 `projects` view 하나를 구성하고
`crates/integration::write_atomic`으로 `<common-root>/integration/life-log/v1/summary.json`을
교체한다. underlying DB를 현재 시각에 다시 조회한 값이므로 write 시점의 view
`freshnessMs`는 0이고, consumer discovery가 파일 경과 시간을 합산한다.

DB mutex는 entry/envelope을 구성하는 동안만 보유하고 filesystem write 전에 해제한다. 모든
producer write는 별도 mutex로 직렬화해 오래된 동시 writer가 새 설정을 뒤늦게 덮어쓰지 못하게
한다. 앱 시작 때 빈 프로젝트 목록도 blocking worker에서 즉시 발행하며, `set_projects` 성공
직후와 60초 background interval마다 같은 worker 경로로 갱신한다. snapshot 실패는 안전한
진단만 기록하고 Life Log의 UI thread와 추적·설정 기능을 중단시키지 않는다.

### 3. Catalog capability와 revision

Files:

- `apps/catalog.json`
- `crates/catalog/tests/catalog.rs`

Life Log의 정적 producer capability에 `snapshot:life-log/projects/v1`을 추가하고 monotonic
`catalogRevision`을 4에서 5로 올렸다. repository catalog test는 revision과 실제 producer
filter 결과가 Life Log 하나인지 고정한다. Workbench의 consumer 선언은 handoff가 아닌
read-only snapshot discovery이므로 catalog `accepts`에 추가하지 않는다.

### 4. 현재 동작 문서

Files:

- `apps/life-log/README.md`
- `docs/architecture.md`

발행 위치와 lifecycle, entry schema, 최근 7일 window, 제외되는 원문/unsafe path, catalog
revision 5의 producer capability를 현재 architecture에 반영했다.

## Code Example

```rust
let mut views = devbox_integration::SnapshotViews::new();
views.insert(
    "projects".into(),
    devbox_integration::SnapshotView {
        schema_version: 1,
        freshness_ms: 0,
        entries,
    },
);
let envelope = devbox_integration::Envelope::with_views(
    "life-log",
    env!("CARGO_PKG_VERSION"),
    views,
);
devbox_integration::write_atomic(
    &envelope,
    &devbox_integration::snapshot_dir("life-log", 1),
)?;
```

## Verification Results

### Life Log unit/integration fixtures

```text
$ CARGO_BUILD_JOBS=1 cargo test -p life-log -j1
running 42 tests
test result: ok. 42 passed; 0 failed
```

추가 fixture는 다음을 검증한다.

- safe absolute Windows/POSIX/UNC path와 Windows case/slash deduplication
- 프로젝트 512개·경로 4,096 bytes 발행 상한
- relative/traversal/device/root/control/trailing/reserved/ADS/wildcard alias path 제외
- 7일 window, longest basename, 중복 basename 미귀속, stable ordering
- 미래 end time clamp와 역전 interval 제외
- 창 제목에 raw Bearer credential이 있어도 serialized envelope에 title/app/credential 부재
- malformed sessions schema의 안전한 실패
- `projects/v1`, producer/schema identity, freshness와 discovery 결과
- 첫 snapshot을 두 번째 전체 view가 교체하고 `summary.json` 외 임시 파일이 남지 않음
- unsafe path만 있어도 유효한 empty view 발행
- 다른 producer target에 쓰려는 경우 identity mismatch로 생성 전 거부

### Rust checks and lint

```text
$ CARGO_BUILD_JOBS=1 cargo check -p life-log -j1
Finished `dev` profile

$ CARGO_BUILD_JOBS=1 cargo clippy -p life-log --all-targets -j1 -- -D warnings
Finished `dev` profile

$ CARGO_BUILD_JOBS=1 cargo test -p catalog -j1
running 11 tests
test result: ok. 11 passed; 0 failed

$ bash .github/scripts/check-catalog.sh
exit 0
```

초기 fixture는 false 조건에서도 `then_some` 인자가 eager evaluation되어 짧은 non-Windows
경로를 slice하는 panic을 재현했다. lazy `then`으로 바꾸고 모든 path class를 함께 다시
실행해 회귀를 고정했다.

WSL에서 `x86_64-pc-windows-msvc` check도 시도했지만 app code compile 전에 bundled
`libsqlite3-sys`가 MSVC의 `lib.exe`를 찾지 못해 중단됐다. GNU compiler로 MSVC C library를
빌드할 수 없는 로컬 툴체인 제약이며, 실제 Windows compile/test 결과는 PR의 Windows runner를
권위 있는 gate로 사용한다.

### Life Log frontend tests and build

```text
$ NODE_OPTIONS=--max-old-space-size=1024 \
    pnpm --filter life-log exec vitest run --passWithNoTests --maxWorkers=1
Test Files  1 passed (1)
Tests       10 passed (10)

$ NODE_OPTIONS=--max-old-space-size=1024 pnpm --filter life-log build
33 modules transformed
built successfully
```

worktree에 잠시 연결한 root/Life Log `node_modules` symlink는 명령 trap에서 제거했다. 전체
workspace Linux/Windows matrix는 PR의 GitHub Actions에서 검증한다.

## Remaining Checkpoint

- Workbench가 이 `projects/v1` entry를 schema/producer/freshness 검증 후 읽고 Life Log SQLite,
  `rusqlite`, app-local path 지식을 제거하는 작업은 독립 기능 #246이다.
- Knowledge activity source 연결은 #247이며 이 producer에 Knowledge 데이터를 섞지 않는다.
- 실제 Windows packaged build의 startup immediate write, 설정 변경 write, 60초 refresh와
  Workbench 소비 화면·로그 evidence는 나머지 P1 merge 후 계획서 §8.3 W1 checkpoint에서
  함께 남긴다. 이 PR의 Windows compile/test CI는 merge 전에 별도로 통과해야 한다.
