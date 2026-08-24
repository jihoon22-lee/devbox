# Workbench Life Log Snapshot Consumer 전환

## Overview

Issue #246의 P1-05-W 범위로 Workbench가 Life Log의 app-local SQLite `data.db`와
`settings` table을 직접 읽던 유일한 integration 경계 위반을 제거했다. Workbench는 이제
`%LOCALAPPDATA%\devbox\integration\life-log\v1\summary.json`의
`projects` schema v1 view만 `crates/integration`을 통해 발견·검증·소비한다.

누락된 snapshot은 정상 no-op이다. 손상, producer/schema 불일치, 과대 entry 목록,
malformed activity summary, unsafe path는 기존 Workbench ProjectProfile을 전혀 바꾸지 않는
fail-closed fallback이다. producer가 꺼져 있다는 이유만으로 마지막 정상 snapshot을 버리지는
않으며, integration discovery가 계산한 view freshness를 consumer 결과에 유지한다.

이 PR은 Workbench preflight, template, retry와 후속 Knowledge activity source 연결을 포함하지
않는다. Workbench 목표 version 0.2.0 bump도 앱의 마지막 v0.5.0 기능 또는 release preparation
PR에 남긴다.

## Context

- 기존 `absorb_life_log_projects`는 `%LOCALAPPDATA%\com.devbox.lifelog\data.db`를 열고
  `SELECT value FROM settings WHERE key = 'projects'`를 실행했다. Workbench가 Life Log의 실제
  bundle identifier, DB 위치, table/key schema와 `rusqlite` 구현을 모두 알아야 했다.
- 선행 #244는 bounded discovery, multi-view envelope, producer/schema/path/freshness와 secret
  validation을 공용 `crates/integration` 계약으로 만들었다.
- 선행 #245는 Life Log가 등록된 안전한 프로젝트 경로와 최근 7일의 숫자 요약만
  `snapshot:life-log/projects/v1`으로 원자 발행하도록 만들었다.
- snapshot은 로컬 사용자가 변조할 수 있으므로 producer가 이미 검증했다는 사실만 믿고 path를
  Workbench profile로 저장하면 안 된다. consumer도 entry schema와 path를 재검증해야 한다.
- Life Log와 Workbench가 같은 absolute/root/traversal/device/reserved-name 규칙을 두 번째로
  필요로 하므로 `CONVENTIONS.md`의 두 번째 실사용 공통화 원칙에 따라 filesystem crate로
  추출했다.

## Changes Made

### 1. Versioned projects/v1 consumer

File: `apps/workbench/src-tauri/src/commands/workspace.rs`

- `devbox_integration::discover_report_in`으로 `life-log` envelope v1과 `projects` view v1을 정확히
  선택한다.
- discovery reference의 `freshnessMs`는 producer가 기록한 view freshness와 현재 파일 경과
  시간을 합친 값이며 `LifeLogAbsorbReport`에 남긴다.
- `devbox_integration::read_snapshot_in`으로 동일 producer/version을 다시 읽어 파일 크기,
  symlink/reparse point, envelope producer/version/generatedAt, JSON depth, forbidden secret
  field/value를 공용 계약에서 재검증한다.
- 최대 512 entries만 허용한다. 각 entry는 `path`, `activityWindowStartMs`,
  `lastActivityAtMs`, `recentSessionCount`, `recentDurationMs`의 type과 내부 일관성을 확인한다.
- 같은 schema version 안의 안전한 추가 metadata는 무시해 forward compatibility를 유지한다.
- snapshot 전체를 먼저 `ProjectProfile` 후보로 변환하고 canonical identity까지 계산한다.
  임시 `ProfileStore`의 모든 upsert가 성공한 뒤에만 caller store를 교체하므로 entry 중간 실패가
  일부 profile만 남기지 않는다.
- file missing은 `Ok(default report)`이고 corrupt/schema mismatch/unsafe entry는 raw input을
  반향하지 않는 고정 오류다. startup은 안전한 진단만 남기고 기존 파일을 유지한다.

### 2. Safe absolute project path 계약 공통화

Files:

- `crates/filesystem/src/project_path.rs`
- `crates/filesystem/src/lib.rs`
- `apps/life-log/src-tauri/src/core/project_snapshot.rs`

`parse_safe_project_path`는 실제 filesystem I/O 없이 다음 path class를 판정한다.

- root가 아닌 absolute Windows drive path
- server/share 아래 실제 프로젝트를 가리키는 UNC path
- root가 아닌 absolute POSIX path

경로당 4,096 bytes를 상한으로 두고 relative, `.`/`..`, control character, Windows device path,
drive/UNC/POSIX root, trailing dot/space, reserved device name, ADS와 wildcard alias를 거부한다.
Windows identity는 slash/case spelling을 접고 POSIX identity는 case를 보존한다.

Life Log producer의 기존 private parser를 같은 공용 함수로 교체했으며 42개 producer tests가
동일 발행 동작을 고정한다. Workbench consumer도 같은 parser로 local-tampering entry를 다시
검사한다.

### 3. ProjectProfile canonical identity 보강

File: `crates/wsl/src/path.rs`

- Windows drive path만 받던 `canonical_project_key`에 일반 UNC identity를 추가했다.
- 일반 UNC는 case/slash를 접은 `unc:<server>/<share>/...` key가 된다.
- `\\wsl$\<distro>\...`와 `\\wsl.localhost\<distro>\...`는
  `wsl:<distro>:...`로 접어 같은 distro/path의 WSL profile과 중복되지 않는다.
- UNC root, traversal과 Windows device path는 identity로 사용할 수 없다.
- `/mnt/<drive>/...` snapshot 경로는 기존 `wsl_to_windows` 규칙으로 drive path로 변환해
  Windows 실행 앱에서 사용할 수 있게 한다.
- `/mnt` 밖의 POSIX 경로는 snapshot에 안전하게 존재할 수 있지만 distro가 없이는
  ProjectProfile로 정확히 표현할 수 없다. 임의 distro를 만들지 않고 건너뛴 수만 안전한
  startup diagnostic에 남긴다.

### 4. Direct DB dependency 제거와 atomic profile persistence

Files:

- `apps/workbench/src-tauri/Cargo.toml`
- `Cargo.lock`
- `THIRD_PARTY_NOTICES.md`
- `apps/workbench/src-tauri/src/lib.rs`

Workbench의 `rusqlite` dependency, `Connection`, Life Log bundle identifier, `data.db` path와 SQL
문장을 모두 제거했다. 대신 이미 공용인 `integration`과 두 번째 실제 소비자가 된
`filesystem` crate만 사용한다.

internal workspace dependency graph 변경으로 달라진 `Cargo.lock` digest를 generated notices에
반영했다. 외부 Rust/frontend dependency 행은 바뀌지 않았다.

startup absorption도 기존 direct `std::fs::write` 대신 Workbench CRUD와 같은 `save_store`
경로를 사용한다. `save_store`는 `devbox_filesystem::atomic_write`로 완성된 JSON을 unique sibling
temporary file에 sync한 뒤 교체한다. snapshot 검증 실패나 저장 실패가 기존 profile 파일을
부분 overwrite하지 않는다.

## Security and Failure Boundaries

- Workbench는 Life Log DB를 읽거나 수정하지 않는다.
- snapshot consumer는 integration root 밖의 경로를 읽지 않고 symbolic link/reparse point를
  따라가지 않는다.
- generic integration validator가 Authorization, Cookie, credential, secret, token, raw
  environment 계열 key와 credential-looking string을 거부한다.
- Workbench 오류에는 snapshot JSON, path, credential 원문을 포함하지 않는다.
- path는 실행하거나 존재 여부를 probing하지 않는다. profile에 저장 가능한 안전한 identity만
  만든다.
- corrupt/malformed/unsafe snapshot 하나는 기존 ProjectProfile과 다른 producer를 변경하지
  않는다.
- stale은 corruption이 아니다. producer가 꺼져도 마지막 정상 project registration은 여전히
  유용하므로 freshness를 보존하면서 읽는다.

## Verification Results

### Shared path and canonical identity

```text
$ CARGO_BUILD_JOBS=1 cargo test -p filesystem -p wsl -j1
filesystem: 15 passed
wsl:        31 passed
```

추가 fixture는 Windows drive/UNC/POSIX, case/slash identity, oversized UTF-8 path, relative,
traversal, roots, device path, reserved alias, control character, 일반 UNC canonical identity와
WSL UNC/profile dedup을 검증한다.

### Workbench consumer

```text
$ CARGO_BUILD_JOBS=1 cargo test -p workbench -j1
running 24 tests
test result: ok. 24 passed; 0 failed
```

consumer fixture는 다음을 검증한다.

- valid Windows/UNC/`/mnt` snapshot absorption과 canonical dedup
- distro를 알 수 없는 POSIX entry의 명시적 skip count
- view freshness 전달
- missing snapshot no-op
- corrupt JSON이 raw credential을 반향하지 않고 기존 store를 보존
- valid JSON 안의 forbidden credential field도 persistence와 raw-value echo 없이 거부
- schema mismatch, unsafe relative/traversal path와 inconsistent activity summary의 complete
  snapshot rejection
- 512-entry bound
- unknown safe entry metadata forward compatibility

### Producer regression

```text
$ CARGO_BUILD_JOBS=1 cargo test -p life-log -j1
running 42 tests
test result: ok. 42 passed; 0 failed
```

공용 parser 추출 뒤에도 #245의 path exclusion, dedup, recent activity aggregation,
privacy-safe serialization과 atomic producer tests가 모두 통과했다.

### Compile and strict lint

```text
$ CARGO_BUILD_JOBS=1 cargo check \
    -p workbench -p life-log -p filesystem -p wsl -j1
Finished `dev` profile

$ CARGO_BUILD_JOBS=1 cargo clippy \
    -p workbench -p life-log -p filesystem -p wsl \
    --all-targets -j1 -- -D warnings
Finished `dev` profile
```

첫 producer regression run은 옮겨진 `MAX_PROJECT_PATH_BYTES` test import 누락을 발견해 공용
상수를 test-only import로 고쳤다. 첫 strict Clippy run은 수동 `Default` 구현을 지적해 derive로
정리했다. 두 수정 뒤 해당 전체 검증을 다시 통과했다.

### Workbench frontend

```text
$ NODE_OPTIONS=--max-old-space-size=1024 \
    pnpm --filter workbench exec vitest run --passWithNoTests --maxWorkers=1
Test Files  3 passed (3)
Tests       12 passed (12)

$ NODE_OPTIONS=--max-old-space-size=1024 pnpm --filter workbench build
35 modules transformed
built successfully
```

worktree에 잠시 연결한 root/Workbench `node_modules` symlink는 command trap에서 제거했다.
전체 workspace Linux/Windows matrix는 PR의 GitHub Actions를 권위 있는 gate로 사용한다.

### Direct DB boundary audit

```text
$ rg 'rusqlite|Connection::open|data\.db|com\.devbox\.lifelog|SELECT value FROM settings' \
    apps/workbench
no matches

$ cargo tree -p workbench --edges normal | rg rusqlite
no matches
```

### Repository policy checks

```text
$ python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml

$ python3 .github/scripts/test-check-dependencies.py
dependency policy regression tests passed

$ python3 .github/scripts/test-build-manifest.py
build-manifest notice tests passed

$ bash .github/scripts/check-catalog.sh
exit 0

$ cargo fmt --all -- --check
exit 0
```

## Documentation

- `apps/workbench/README.md`에 `projects/v1` 입력 위치, failure fallback, freshness와 direct DB
  부재를 현재 동작으로 기록했다.
- `docs/architecture.md`의 알려진 direct SQLite 예외를 제거하고 Life Log producer → Workbench
  consumer, all-or-nothing profile absorption과 stale policy를 기록했다.

## Remaining Checkpoint

- Knowledge snapshot을 Life Log activity source로 연결하는 작업은 독립 #247이며 이 consumer에
  Knowledge 데이터를 섞지 않는다.
- Workbench services/ports와 WSL proposal, preflight/template/retry는 각각 별도 issue다.
- Workbench 0.2.0 version bump는 마지막 Workbench v0.5.0 기능 또는 release preparation PR에서
  Cargo/package/Tauri 세 원본을 함께 변경한다.
- packaged Life Log immediate/60-second write와 Workbench startup consume, corrupt fallback의
  Windows 화면·로그 evidence는 나머지 P1 merge 뒤 계획서 §8.3 W1 checkpoint에서 남긴다.
