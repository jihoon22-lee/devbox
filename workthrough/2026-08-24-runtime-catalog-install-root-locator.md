# Runtime Catalog and Install-root Locator

## Overview

P1-03-M [#239](https://github.com/jihoon22-lee/devbox/issues/239)의 runtime catalog
배포, versioned install-root locator, 실제 설치 target discovery를 연결했다. Devbox
Manager가 신뢰 가능한 build-time catalog를 공용 위치에 원자적으로 발행하고 Manager
소유 설치 manifest의 canonical 위치를 locator로 공개한다. 메뉴 소비자는
`crates/launch::installed_targets`를 통해 최신 catalog capability와 실제 설치된 portable
executable의 교집합만 얻는다.

이 변경은 custom install root를 이동하거나 제거하는 UI를 구현하지 않는다. 해당 후속 기능이
안전하게 붙을 수 있도록 locator의 schema, revision, write/fallback 경계만 먼저 고정한다.
Repo Manager target이나 실제 앱별 handoff 기능도 이 PR의 범위 밖이다.

## Context

catalog v2 기반은 build/runtime freshness와 capability filter를 제공했지만 runtime 파일을
발행하는 소유자, 실제 설치 root를 찾는 계약, 설치된 target만 추리는 연결 계층은 없었다.
v0.4.x의 `crates/launch`는 `%LOCALAPPDATA%\com.devbox.devboxmanager`를 직접 추측했고,
Manager 프론트의 browser mock은 13개 앱을 TypeScript 배열로 다시 복사했다.

이 상태에는 다음 문제가 있었다.

- Manager가 갱신한 catalog를 다른 앱이 안정적으로 발견할 수 없다.
- custom root 지원을 추가하면 소비 앱마다 설치 위치 추측이 갈라질 수 있다.
- 손상된 registry나 경로 traversal이 legacy fallback을 통해 우회될 수 있다.
- Windows에서는 기존 fixed `.tmp` + `std::fs::rename` 교체가 기존 파일 위에서 일관되게
  동작하지 않는다.
- catalog identity를 Rust include와 TypeScript mock 양쪽에서 관리해야 한다.

## Changes Made

### 1. Cross-platform atomic file replacement

`crates/filesystem::atomic_write`를 추가했다. target과 같은 디렉터리에 PID와 monotonic
sequence가 포함된 고유 임시 파일을 `create_new`로 만들고, 전체 bytes를 write/flush/fsync한
뒤 파일을 닫고 commit한다. 호출자가 디렉터리 정책을 소유하도록 parent는 자동 생성하지
않는다.

- Unix는 동일 filesystem의 `rename` 뒤 parent directory를 sync한다.
- Windows는 `MoveFileExW`의 `REPLACE_EXISTING | WRITE_THROUGH`로 기존 target 교체를
  지원한다.
- commit 실패 시 해당 호출이 만든 임시 파일만 제거한다.
- `crates/integration::write_atomic`, Manager의 `registry.json`, `current.json`, runtime
  catalog, locator가 같은 primitive를 사용한다.

fixture는 최초 생성과 기존 target 교체 후 완전한 bytes만 남는지, 고유 임시 파일이 남지
않는지, 호출자가 소유하지 않은 parent를 만들지 않는지를 검증한다.

### 2. Runtime catalog publisher

Manager 시작 시와 설치 registry가 성공적으로 바뀐 뒤
`%LOCALAPPDATA%\devbox\catalog.json`을 동기화한다.

- build-time catalog는 schema v2와 positive `catalogRevision`을 통과해야 한다.
- runtime 파일이 없거나 손상되었거나 build보다 stale이면 build 사본을 원자 기록한다.
- 같은 revision은 rewrite하지 않는다.
- 더 높은 유효 revision은 downgrade하지 않고 보존한다.
- 더 높은 runtime revision을 보존했을 때 locator에도 그 effective revision을 provenance로
  기록한다.

설치 자체의 registry commit이 성공한 뒤 metadata 동기화가 실패하면 설치 성공을 거짓
실패로 바꾸지 않는다. 안전한 고정 오류만 로그로 남기고 다음 앱 시작에 재시도한다.

### 3. Versioned install-root locator

고정 공용 위치
`%LOCALAPPDATA%\devbox\install-roots\v1\registry.json`에 다음 최소 정보만 기록한다.

- `schemaVersion`: 현재 1
- `registryRevision`: locator 내용 변경 때 단조 증가
- `catalogRevision`: 발행 당시 effective catalog provenance
- `rootId`: 기본 root는 `devbox-manager-default`
- `path`: canonical Manager install/data root
- `manifestPath`: Manager가 소유하는 canonical `registry.json`
- `updatedAtMs`: 양수 갱신 시각

초기 실행은 누락된 Manager manifest를 빈 JSON 배열로 만들고 locator revision 1을
발행한다. 동일 root/manifest/catalog 조합은 rewrite하지 않는다. catalog provenance가
바뀌면 revision을 정확히 한 번 올린다. 유효한 다른 root ID는 후속 custom-root 기능의
상태로 간주해 기본 root로 덮어쓰지 않으며, `write_locator_if_newer`는 equal/lower candidate가
현재 locator를 덮어쓰지 못하게 한다.

### 4. Installed target discovery and fail-closed boundary

`crates/launch`는 locator parser와 다음 공개 API를 제공한다.

- `runtime_catalog_path`
- `install_root_registry_path`
- `parse_install_root_locator`
- `resolve_installed_from_paths`
- `installed_targets` / `installed_targets_from_paths`

`installed_targets(capability)`는 catalog crate가 선택한 build/runtime catalog에서 exact
capability를 받는 앱만 고른 뒤 Manager manifest의 실제 portable executable과 결합한다.
installer mode는 executable을 추측하지 않고 제외한다.

locator와 manifest 경계에서는 다음을 검증한다.

- schema/revision/root ID/absolute literal path
- root와 manifest가 direct symlink가 아니며 canonical literal과 동일한지
- manifest가 root 내부 regular file인지
- manifest row의 app ID, strict three-part numeric version, mode, 중복 여부
- portable executable이 정확히
  `<root>/apps/<id>/versions/<version>/<id>.exe`인지
- executable이 symlink로 root 밖을 탈출하지 않는지

locator가 없거나 JSON 자체가 손상된 경우에만 v0.4.x 고정 Manager root를 read-only
fallback으로 읽는다. 일단 locator가 유효하면 그 뒤의 manifest 또는 executable 오류는
legacy root로 우회하지 않고 fail closed한다. legacy `current.json`과 latest-version 경로도
같은 exact layout, safe app ID, numeric version, symlink/canonical containment 조건으로
강화했다.

### 5. Runtime consistency diagnosis

Manager doctor에 `runtime-metadata` 항목을 추가했다. 다음이 모두 성립할 때만 정상이다.

- runtime catalog revision이 build-time보다 낮지 않음
- locator catalog provenance가 선택된 runtime revision과 일치
- locator root/manifest가 canonical literal이며 manifest가 root 내부 regular file
- 기본 root ID이면 locator root가 실제 Manager data root와 일치
- root/home/current workspace 같은 위험 root가 아님

오류는 secret, raw JSON, absolute path를 반향하지 않고 고정된 복구 안내만 반환한다.

### 6. Manager catalog single source

브라우저 개발 모드의 `MOCK_CATALOG` 13개 수동 배열을 제거하고
`apps/devbox-manager/catalog.json`의 apps를 직접 import한다. TypeScript `CatalogApp`에도
v2의 `accepts`, `produces`, `actions` 계약을 추가해 Rust와 프론트가 같은 schema를
표현한다.

## Design Decisions

### Locator, not a second installation database

공용 registry는 앱별 설치 상태를 복제하지 않는다. 고정 위치에는 root와 app-owned
manifest를 찾는 locator만 두며, 설치/업데이트/rollback의 진실 원본은 계속 Devbox
Manager의 `registry.json`이다. 이 구조는 후속 custom root 기능이 root를 바꿔도 소비자가
Manager 내부 정책을 재구현하지 않게 한다.

### Missing/corrupt locator migration versus trusted-boundary failure

v0.4.x 사용자는 locator 없이 시작하므로 missing/corrupt locator에는 read-only fallback이
필요하다. 반면 유효한 locator 뒤의 manifest/path 오류까지 fallback하면 공격자가 신뢰
경계를 부분적으로 조작해 옛 위치를 실행시킬 수 있다. 따라서 locator validation 전과 후를
명확히 나누고 후자는 fail closed한다.

### Catalog and registry revisions are independent

`catalogRevision`은 catalog 내용 freshness이고 `registryRevision`은 locator 변경 순서다.
catalog 변경만으로 설치 root를 무효화하지 않지만, locator가 어떤 catalog를 기준으로
발행되었는지 consistency 진단할 수 있도록 provenance를 기록한다.

### Safe retry after installation commit

설치 registry를 원자적으로 commit한 뒤 runtime metadata 발행이 실패할 수 있다. 이때
설치 전체를 실패로 보고 재다운로드시키는 대신 성공 상태를 유지하고 다음 시작에 metadata만
재동기화한다. write 순서와 retry 경계를 분리해 외부 상태와 사용자 메시지가 어긋나지 않게
했다.

## Verification

로컬에서는 자원 점유를 제한하기 위해 package 단위와 `-j1`로 검증했다. 저장소 전체
frontend/Linux Rust/Windows Rust 검증은 PR의 GitHub Actions matrix가 수행한다.

- `cargo test -p filesystem -j 1` — 11 passed
- `cargo test -p integration -j 1` — 5 passed
- `cargo test -p launch -j 1` — 17 passed
- `cargo test -p devbox-manager --lib -j 1` — 37 passed
- `cargo check -p devbox-manager -j 1`
- `cargo check -p filesystem --target x86_64-pc-windows-gnu -j 1`
- `cargo check -p launch --target x86_64-pc-windows-gnu -j 1`
- `cargo fmt --all`
- `CARGO_BUILD_JOBS=1 cargo clippy -p filesystem -p integration -p launch -p devbox-manager --all-targets -- -D warnings`
- `python3 .github/scripts/check-dependencies.py check`
- `bash .github/scripts/check-catalog.sh`

주요 회귀 fixture는 corrupt/stale/newer runtime catalog, equal/lower locator revision,
catalog provenance change, future custom root preservation, missing/corrupt locator legacy
fallback, valid locator 뒤 corrupt manifest fail-closed, version/path mismatch, traversal app ID,
symlink escape, 안전한 오류 문자열을 포함한다.

## Follow-up Boundaries

- custom install root 선택·이동·제거 UI는 P2-11 후속 issue에서 이 locator를 소비한다.
- Repo Manager 및 다른 앱의 실제 target/menu 연결은 각 기능 issue에서
  `installed_targets`를 소비한다.
- Windows packaged build smoke와 화면/로그 evidence는 계획된 W1 checkpoint에서 수행한다.
