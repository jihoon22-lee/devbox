# Everything+ Search Result Context Menu

## Overview

Issue #251의 P1-06-EV 범위로 Everything+의 name/content 검색 결과 행에
`@devbox/context-menu`를 적용했다. 각 결과에서 Open, Show in folder, Copy path,
Copy file name을 즉시 실행하고, catalog `path` capability와 실제 설치 상태를 모두 통과한
devbox 앱만 `Open in another app` submenu에 표시한다.

content index 확대와 advanced filter는 이 기능에 포함하지 않았다. 목표 앱 버전 0.4.0의
version bump도 release PR이 소유한다.

## Context

- 검색 결과에는 동일 동작의 작은 button 세 개만 있었고 keyboard로 행별 부가 동작을 여는
  경로, 파일명 복사, 다른 devbox 앱으로 전달하는 경로가 없었다.
- 검색 결과는 name과 content 두 table로 나뉘므로 어느 mode에서도 같은 메뉴 계약과 row
  selection 동기화가 필요했다.
- `Open in another app`은 설치되지 않은 앱이나 catalog가 선언하지 않은 앱을 보여주면 안 되고,
  frontend가 임의 app id나 executable 경로를 실행 경계로 보낼 수 없어야 한다.
- Everything+ index의 파일은 메뉴가 열린 뒤 삭제될 수 있다. launch 직전 backend 검증이 없는
  frontend-only 연결은 stale/relative/traversal path를 다른 앱에 전달할 수 있다.

## Changes Made

### 1. Name/content 결과 행의 앱 고유 메뉴

File: `apps/everything-plus/src/App.tsx`

두 결과 table의 행을 focus 가능한 context-menu trigger로 만들고 다음 exact topology를 공유한다.

```tsx
[
  { type: "item", id: "open", label: "Open" },
  { type: "item", id: "reveal", label: "Show in folder" },
  { type: "item", id: "copy-path", label: "Copy path" },
  { type: "item", id: "copy-name", label: "Copy file name" },
  {
    type: "submenu",
    id: "open-in",
    label: "Open in another app",
    items: catalogTargetItems,
  },
]
```

- pointer right-click과 Shift+F10/Menu key는 공용 hook의 한 경로를 사용한다.
- `onBeforeOpen`에서 `data-result-index`를 현재 name/content 결과 배열에 다시 대조하고
  `activeIdx`와 action snapshot을 우클릭한 행으로 먼저 맞춘다.
- 행은 `tabIndex=0`, implicit row role의 `aria-selected`, `aria-haspopup`/`aria-expanded`를
  제공한다.
- 메뉴 종료 후 공용 primitive가 원래 결과 행으로 focus를 복구한다.
- 검색 결과 배열이나 mode가 바뀌면 열린 메뉴와 action snapshot을 닫아 stale 결과에 대한
  후속 action을 막는다.
- 기존 row click은 hover로 갱신된 전역 index에 기대지 않고 클릭한 행의 exact path를 연다.

### 2. Catalog 기반 target discovery와 frontend 최소 정보

Files:

- `apps/everything-plus/src/api.ts`
- `apps/everything-plus/src-tauri/src/commands/actions.rs`
- `apps/everything-plus/src-tauri/src/core/open_targets.rs`
- `apps/everything-plus/src-tauri/src/core/mod.rs`
- `apps/everything-plus/src-tauri/src/lib.rs`

Rust command는 `devbox_launch::installed_targets("path")`를 사용한다. 이 API가 build/runtime
catalog freshness, install-root locator, Manager manifest와 실제 executable을 이미 검증하므로
Everything+는 앱 allowlist나 설치 위치를 중복 소유하지 않는다.

```rust
pub fn select_open_targets(
    source_app_id: &str,
    path_targets: Vec<InstalledTarget>,
) -> Vec<EverythingOpenTarget>
```

`EverythingOpenTarget`은 `id`와 `displayName`만 serialize한다. resolved executable은 Rust
process 안에 남고 frontend IPC payload에는 포함되지 않는다. source app은 capability 결과에서
제외하며, 대상 순서는 catalog 순서를 유지해 새 path-capable 앱이 추가돼도 Everything+ UI를
수정하지 않는다.

Browser preview도 하드코딩 목록 대신 build-time `apps/catalog.json`에서 mock target을 만든다.
packaged runtime은 반드시 `open_targets` command 결과만 사용한다. locator/manifest가 없거나
유효한 설치 대상이 하나도 없으면 submenu를 disabled로 표시하며 오류 fallback 실행을 하지 않는다.

### 3. Launch 직전 target/file 재검증

File: `apps/everything-plus/src-tauri/src/core/open_targets.rs`

`prepare_open_request()`는 frontend 입력을 신뢰하지 않고 다음 순서로 fail closed 처리한다.

1. app id를 ASCII lowercase로 정규화하고 현재 설치/capability 교집합에서 exact match한다.
2. path가 비어 있지 않은 절대 경로인지 확인한다.
3. `.`/`..` component를 거부한다.
4. 실행 시점에 metadata를 다시 읽어 존재하는 regular file인지 확인한다.
5. 통과한 값만 `OpenTarget::Path`와 `from=everything-plus`로 만들고
   `devbox_launch::launch_open()`에 전달한다.

오류 문자열에는 받은 app id나 로컬 path를 반향하지 않는다. frontend도 submenu item id를
현재 command 결과와 다시 대조하므로 DOM에서 조작한 임의 `open-in:*` action은 IPC를 호출하지
않는다.

### 4. Interaction, failure, and Rust fixtures

File: `apps/everything-plus/src/App.test.tsx`

기존 Query single-instance fixture를 유지하면서 result menu fixture 6개를 추가했다.

- 다른 행 우클릭 시 selection-first 동기화와 exact menu topology
- Open, Explorer reveal, path/name clipboard의 exact 선택 행 값
- catalog-derived submenu에서 선택한 app id와 path만 `openIn`으로 전달
- Shift+F10 open과 action 뒤 원래 행 focus 복구
- 설치 대상 없음에서 disabled submenu 및 launch non-call
- content 결과의 동일 메뉴와 launch failure의 복구 가능한 오류 표시

File: `apps/everything-plus/src-tauri/src/core/open_targets.rs`

Rust fixture 3개는 catalog 순서/source 제외, Path request shape, case-normalized target 선택,
missing target, relative/traversal path와 raw input non-echo를 검증한다. 실제 process는 띄우지 않고
임시 regular file까지만 사용한다.

### 5. Workspace wiring, locks, notices, and documentation

Files:

- `apps/everything-plus/package.json`
- `apps/everything-plus/src-tauri/Cargo.toml`
- `pnpm-lock.yaml`
- `Cargo.lock`
- `THIRD_PARTY_NOTICES.md`
- `apps/everything-plus/README.md`
- `docs/architecture.md`

추가한 의존성은 기존 workspace 내부의 `@devbox/context-menu`와 `devbox-launch`뿐이다. 새
registry package, 외부 runtime, sidecar, daemon, network/download 경로, Tauri permission은
없다. Cargo/pnpm lockfile에는 Everything+ importer의 내부 edge만 추가됐다.

notices inventory와 license 본문은 바뀌지 않았지만 generator가 lockfile provenance hash를
추적하므로 `THIRD_PARTY_NOTICES.md`의 두 SHA-256만 생성 스크립트로 갱신했다. README에는
결과 메뉴를, architecture에는 executable 비노출·catalog/install 교집합·파일 재검증 경계를
기록했다.

## Verification Results

### Frontend interaction and regression tests

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter everything-plus test -- --maxWorkers=1
Test Files  2 passed (2)
Tests      14 passed (14)
exit 0
```

기존 Query delivery/ordering/invalid input fixture와 새 context-menu fixture가 함께 통과했다.

### Frontend production build

```text
$ NODE_OPTIONS=--max-old-space-size=768 pnpm --filter everything-plus build
vite v7.3.6
43 modules transformed
dist/assets/index-B3uTYGEh.js  216.88 kB | gzip 67.96 kB
dist/assets/index-DfJa_ea7.css   6.10 kB | gzip  1.77 kB
dist/assets/event-DNkUz6YC.js    1.10 kB | gzip  0.59 kB
exit 0
```

### Rust formatting, tests, compile, and lint

```text
$ cargo fmt --all -- --check
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p everything-plus -j1
28 passed; 0 failed
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p everything-plus core::open_targets -j1
3 passed; 0 failed; 25 filtered out
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo check -p everything-plus --all-targets -j1
Finished dev profile
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo clippy -p everything-plus --all-targets -j1 -- -D warnings
Finished dev profile
exit 0
```

### Dependency, notice, catalog, and diff gates

```text
$ python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml

$ python3 .github/scripts/test-check-dependencies.py
dependency policy regression tests passed

$ python3 .github/scripts/test-build-manifest.py
build-manifest notice tests passed

$ bash .github/scripts/check-catalog.sh
exit 0

$ git diff --check
exit 0
```

첫 dependency check는 내부 edge가 바꾼 lockfile hash 때문에 generated notices가 stale이라고
정확히 실패했다. `.github/scripts/check-dependencies.py generate`로 갱신한 뒤 같은 gate를 다시
실행해 통과했다. notices를 수동 편집하지 않았다.

모든 로컬 명령은 frontend heap 768 MiB, Vitest 1 worker, Cargo 1 job과 Linux-native shared
target directory를 사용했다. 전체 workspace와 Windows 검증은 GitHub Actions를 권위 있는
PR gate로 사용한다.

## Security and Failure Boundaries

- frontend는 target executable, locator path, Manager manifest를 받지 않는다.
- catalog `path` capability와 설치 manifest가 모두 확인된 target만 표시·실행한다.
- target 제거/변조 race는 command가 현재 target 목록을 다시 계산해 fail closed한다.
- 상대·traversal·누락·directory result는 launch 전에 거부하며 raw input을 오류에 넣지 않는다.
- clipboard는 사용자가 선택한 현재 결과의 path/name만 쓰고 background read나 persistence가 없다.
- 결과 배열 변경은 열린 메뉴를 닫아 stale action snapshot을 폐기한다.
- content index 확대, advanced filter, saved query, arbitrary external executable 연결은 비범위다.

## Follow-up

- #252~#261: 나머지 기존 앱별 context menu
- #276: Everything+ content index 확대
- #315~#317: advanced filter, saved query, 주변 행 preview
- Windows W1: packaged WebView2의 pointer/Menu key, nested-menu viewport flip, Explorer reveal,
  cold/hot target app launch와 focus restore evidence
