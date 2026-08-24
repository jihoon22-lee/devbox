# Snapshot Discovery와 Life Log Data Sources 연결

## Overview

Issue #244의 P1-05-I 범위로 `crates/integration`에 안전한 snapshot 자동 발견과
multi-view 계약을 추가하고, Life Log의 하드코딩 Run Manager reader를 공용 발견 결과로
교체했다. 기존 Run Manager·Knowledge Base flat snapshot은 그대로 호환하면서, 향후 producer는
`data.views`에 여러 kind를 모아 `summary.json` 전체를 한 번만 원자 교체할 수 있다.

## Context

- Life Log는 `core/readers.rs`에서 integration envelope·경로·reader를 중복 구현하고
  `run-manager`만 하드코딩했다.
- 새 producer가 생겨도 Data Sources에 자동으로 나타나지 않았고, 손상 producer와 정상
  producer를 함께 탐색하는 공용 API가 없었다.
- 여러 snapshot kind를 kind별 writer로 같은 파일에 기록하면 서로 덮어쓸 수 있으므로,
  한 producer/version당 파일 하나와 `data.views` 전체 교체 계약이 필요했다.
- snapshot은 앱 간 read-only 요약 경계이므로 크기, path identity, link/reparse point,
  timestamp/version, 민감 필드를 공용 코드에서 검증해야 했다.

## Changes Made

### 1. 공용 discovery·multi-view 계약

File: `crates/integration/src/lib.rs`

- 설계에 명시된 `discover() -> Vec<SnapshotRef>`와 진단용 `discover_report()`를 추가했다.
- fixture와 locator consumer가 같은 로직을 쓸 수 있도록 `discover_report_in`,
  `read_snapshot_in`, `snapshot_path_in`, `snapshot_dir_in`을 제공한다.
- `<integration-root>/*/v*/summary.json`만 스캔하며 producer/version 순으로 안정 정렬한다.
- producer 하나의 손상 JSON, schema/producer 불일치, 읽기 오류를 `SnapshotIssue`로 격리한다.
- `SnapshotView`, `SnapshotViews`, `Envelope::with_views`, `Envelope::views`를 추가했다.
- 각 발견 결과에 producer version, generated time, 파일 freshness와 view별 누적 freshness,
  entry count를 보존한다.
- 기존 flat `data` envelope은 유효하며 `views()`에서 빈 map으로 해석한다.

### 2. 원자 교체와 writer ownership

File: `crates/integration/src/lib.rs`

- 기존 `devbox_filesystem::atomic_write`를 유지해 완성된 envelope 하나만 노출한다.
- 기록 전에 envelope identity와 대상의 `<producer>/v<schema-version>` identity가 같은지
  검증해 다른 producer 파일을 실수로 덮어쓰지 못하게 했다.
- version/producer/integration 디렉터리의 symbolic link 또는 Windows reparse point를
  생성 전과 생성 후 모두 확인한다.
- 병렬 writer fixture는 최종 파일이 항상 완전한 JSON이며 고유 임시 파일이 남지 않음을
  검증한다.
- Windows `MoveFileExW`가 동시 replace에서 잠깐 반환하는 sharing/lock/access-denied만
  `crates/filesystem/src/lib.rs`에서 16회 bounded backoff로 재시도한다. 다른 Win32 오류는
  즉시 반환하며 최종 실패 시 호출자 소유 임시 파일 정리 계약을 유지한다.

### 3. Snapshot 보안 경계

File: `crates/integration/src/lib.rs`

- 파일과 직렬화 결과에 10MiB 상한을 적용했다.
- producer/view kind는 안전한 소문자 kebab identifier, schema version은 1 이상,
  producer version은 SemVer 형태, generated time은 실제 달력 날짜인 UTC 형식으로 검증한다.
- JSON depth를 제한하고 multi-view entry는 object만 허용한다.
- Authorization, Cookie, secret, password, credential, API key/token, private key,
  raw environment 계열 field와 대표적인 raw credential 문자열을 쓰기·읽기 양쪽에서 거부한다.
- parser·mismatch·I/O 오류는 snapshot 원문, secret 값, 공격자가 넣은 producer 값을
  오류 문자열에 반영하지 않는다.
- `read_snapshot`은 파일 없음만 `Ok(None)`으로 유지하고 다른 실패는 안전한 `Err`로 구분한다.

### 4. Life Log의 동적 Data Sources

Files:

- `apps/life-log/src-tauri/src/commands/life.rs`
- `apps/life-log/src-tauri/src/core/mod.rs`
- `apps/life-log/src-tauri/src/core/readers.rs` (삭제)
- `apps/life-log/src-tauri/Cargo.toml`
- `Cargo.lock`
- `apps/life-log/src/App.tsx`
- `crates/filesystem/Cargo.toml`
- `crates/filesystem/src/lib.rs`
- `THIRD_PARTY_NOTICES.md`

Life Log가 `devbox_integration::discover_report()`를 호출하고 정상 snapshot과 격리된 오류를
기존 `SourceStatus` shape으로 변환하도록 변경했다. source가 0개면 빈 목록, root 자체를 읽을
수 없으면 별도 안전한 진단 행을 반환한다. 같은 producer의 여러 schema version과 정상/오류
행이 React key에서 충돌하지 않도록 version과 상태를 key에 포함했다.

중복 `core/readers.rs`와 module export를 제거하고 Life Log에 공용 integration crate 의존성을
추가했다. producer 업무 데이터나 Workbench의 직접 DB 전환은 각각 후속 #245, #246 범위로
남겼다.

Life Log의 내부 crate dependency edge가 `Cargo.lock`에 추가되어 lockfile digest가 바뀌었으므로
공식 dependency generator로 notices의 Cargo SHA-256을 갱신했다. 외부 package inventory에는
변경이 없다.

### 5. Architecture 문서

Files:

- `apps/life-log/README.md`
- `docs/architecture.md`
- `crates/integration/Cargo.toml`

Life Log가 공용 discovery를 사용한다는 사실과 `data.views`, 오류 격리, 10MiB·민감 필드·
unsafe-link 방어선을 현재 architecture에 반영하고 integration crate 설명을 갱신했다.

## Code Examples

### Producer가 여러 view를 한 번에 구성

```rust
let mut views = devbox_integration::SnapshotViews::new();
views.insert(
    "status".into(),
    devbox_integration::SnapshotView {
        schema_version: 1,
        freshness_ms: 0,
        entries,
    },
);
let envelope = devbox_integration::Envelope::with_views(
    "run-manager",
    env!("CARGO_PKG_VERSION"),
    views,
);
devbox_integration::write_atomic(
    &envelope,
    &devbox_integration::snapshot_dir("run-manager", 1),
)?;
```

### Consumer의 자동 발견

```rust
pub fn integration_sources() -> Vec<SourceStatus> {
    source_statuses(devbox_integration::discover_report())
}
```

## Verification Results

### Integration contract tests

```text
$ CARGO_BUILD_JOBS=1 cargo test -p integration -j1
running 14 tests
test result: ok. 14 passed; 0 failed

$ CARGO_BUILD_JOBS=1 cargo clippy -p integration --all-targets -j1 -- -D warnings
Finished `dev` profile
```

포함 fixture: 0/1/N discovery, corrupt producer isolation, stable order, legacy flat 호환,
multi-view complete replacement, concurrent writer collision, freshness, sensitive field/value,
path identity, symlink escape, view forward compatibility, timestamp와 producer version validation.

### Life Log Rust tests

```text
$ CARGO_BUILD_JOBS=1 cargo test -p life-log --lib -j1
running 32 tests
test result: ok. 32 passed; 0 failed
```

추가 fixture는 정상/손상 source 동시 표시, integration root 오류 표시, 0-source 빈 목록을
검증한다.

### Related Rust consumer checks

```text
$ CARGO_BUILD_JOBS=1 cargo check \
    -p integration -p life-log -p run-manager -p knowledge-base -p workbench -j1
Finished `dev` profile
```

### Atomic writer and Windows branch checks

```text
$ CARGO_BUILD_JOBS=1 cargo test -p filesystem -p integration -j1
filesystem: 11 passed; integration: 14 passed

$ CARGO_BUILD_JOBS=1 cargo clippy -p filesystem -p integration \
    --target x86_64-pc-windows-msvc --all-targets -j1 -- -D warnings
Finished `dev` profile
```

Windows CI가 처음 재현한 `MoveFileExW` sharing/lock 경합은 공용 bounded retry로 수정했다.
같은 CI의 check와 clippy 단계가 통과한 뒤 integration 병렬 writer test가 실패했다는 로그를
근거로 했으며, unrelated path/permission 오류를 재시도하지 않도록 Win32 error code를 제한했다.

### Life Log frontend

```text
$ NODE_OPTIONS=--max-old-space-size=1024 \
    pnpm --filter life-log test -- --maxWorkers=1
Test Files  1 passed (1)
Tests       10 passed (10)

$ NODE_OPTIONS=--max-old-space-size=1024 pnpm --filter life-log build
33 modules transformed
built successfully
```

### Dependency policy

```text
$ python3 .github/scripts/check-dependencies.py generate
generated THIRD_PARTY_NOTICES.md
$ python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml
$ python3 .github/scripts/test-check-dependencies.py
dependency policy regression tests passed
$ python3 .github/scripts/test-build-manifest.py
build-manifest notice tests passed
```

임시로 연결한 root/Life Log `node_modules` symlink는 명령의 `trap`에서 제거했고 종료 후
존재하지 않음을 확인했다. 전체 workspace Linux/Windows matrix는 PR의 GitHub Actions에서
검증한다.

## Remaining Checkpoint

- 실제 Windows packaged build 화면·로그 evidence는 계획서 §8.3에 따라 나머지 P1 merge 후
  W1 checkpoint에서 함께 수행한다. 이 PR의 Windows CI는 merge 전에 별도로 통과해야 한다.
- Life Log project producer, Workbench DB reader 제거, Knowledge activity 소비는 각각
  #245, #246, #247의 독립 기능 PR이며 이번 변경에 중복 포함하지 않았다.
