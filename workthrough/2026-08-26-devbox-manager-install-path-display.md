# Devbox Manager Install Path Display Workthrough

- Date: 2026-08-26
- Issue: #275 `feat(devbox-manager): install path 표시`
- Branch: `feat/devbox-manager/install-path-display`
- Base: `608d8aab0213ca131b34cccc57eca710ab34ddfe`
- Target: Devbox Manager 0.4.0 / v0.5.0 P1-09-12
- Status: implementation, direct review and local PR-wide gates complete; GitHub Actions pending

## Outcome

설치된 앱 행에서 `Paths` 버튼 또는 컨텍스트 메뉴의 `설치 경로 정보`를 선택하면 다음 정보를
명시적인 읽기 전용 패널에서 확인할 수 있다.

1. 실제 executable
2. 실제 install root
3. 해당 상태를 읽은 source manifest

표시 값은 frontend registry snapshot에서 꺼내지 않는다. 공용 `crates/launch`가 runtime/build catalog,
versioned install-root locator, canonical root, canonical source manifest와 manifest의 모든 portable
executable을 검증한 뒤 만든 증거만 별도 IPC가 반환한다.

portable은 Manager가 exact layout과 executable을 소유하므로 세 경로를 모두 표시한다. installer는
Manager가 설치 마법사의 최종 경로를 증명하지 못하므로 executable/root를 `null`로 반환하고 UI에서
명확하게 “Manager가 실제 설치 위치를 추적하지 않습니다”라고 표시한다. source manifest는 설치 방식과
버전 기록의 provenance이므로 계속 표시한다.

## Problem and Existing Boundary

기존 Manager는 lifecycle 안전성을 위해 `InstalledAppView`와 `CurrentView`에서 `exe_path`를 제거했다.
실행, rollback, 폴더 열기와 제거는 app ID만 받고 backend가 registry와 고정 layout을 다시 검증한다.
이 원칙은 임의 path를 action 입력으로 바꾸는 공격과 stale frontend path 사용을 막는다.

동시에 사용자는 현재 설치 위치를 직접 확인할 방법이 없었다. `설치 폴더 열기`는 portable에만 제공되고
경로를 표시하지 않으며, installer는 Manager가 실제 위치를 알 수 없다. #275는 이 간극을 메우되 기존
lifecycle DTO와 action 계약을 넓히지 않는 독립 표시 기능이다.

## Scope

### Included

- catalog-managed installed app별 읽기 전용 경로 조회
- portable canonical executable/root/source manifest 표시
- installer source manifest 표시와 executable/root unavailable 설명
- row `Paths` action
- app-row context menu `설치 경로 정보`
- 긴 경로 wrap과 text selection
- locator/catalog revision/manifest/path fail-closed fixture
- 조회 전후 filesystem byte 불변 fixture
- portable/installer frontend rendering fixture

### Excluded

- 경로 선택 또는 변경
- custom install root 생성·이동·migration
- registry/current/locator 쓰기
- install/update/remove/rollback/launch
- Explorer open 또는 clipboard copy
- installer의 install directory/uninstaller discovery
- legacy locator fallback을 통한 표시
- Data Inspector, support bundle, Related Tools

## Design Decisions

### 1. Explicit display DTO, not lifecycle DTO expansion

기존 `installed`와 `current` 응답은 계속 path-free다. 새 `install_path(appId)`만 검증된 표시 경로를
반환한다. 따라서 path가 실행·제거 action의 입력으로 재사용되는 계약은 생기지 않는다.

### 2. `crates/launch` owns locator resolution

locator와 installed manifest는 Repo Manager, Workbench 등 여러 앱의 launch discovery에서도 사용한다.
Manager가 동일한 parser를 별도로 복제하면 revision, legacy fallback과 canonical identity 규칙이 달라질
수 있다. 이미 두 번째 소비자인 `crates/launch`에 표시용 검증 결과를 추가했다.

### 3. No legacy fallback for explicit evidence

프로세스 실행 discovery는 v0.4.x migration을 위해 locator가 없거나 손상되면 legacy layout을 읽을 수
있다. 그러나 “source manifest”를 사용자에게 실제 증거로 표시하는 기능은 provenance가 불명확한 fallback을
사용하지 않는다. 유효한 versioned locator와 revision 일치가 없으면 고정 오류로 종료한다.

### 4. Installer location stays unknown

Manager registry의 installer row는 `mode`, `version`과 빈 `exe_path`만 소유한다. setup process spawn은
마법사의 성공, 사용자가 고른 directory나 설치 완료를 증명하지 않는다. Windows uninstall registry나
Program Files scan을 추측하면 다른 제품 또는 stale entry를 잘못 연결할 수 있으므로 이 PR에서는 하지
않는다.

### 5. Read-only means no adjacent convenience mutation

표시 패널은 path를 선택할 수 있는 text만 제공한다. copy button, Explorer open, path picker와 edit action은
추가하지 않았다. 조회 API도 `AppHandle` data directory creation helper, registry writer, opener와 process
API를 호출하지 않는다.

## Data Flow

```text
row Paths / context menu
  -> installPath(appId)
  -> Tauri install_path { appId }
  -> catalog-managed target validation
  -> runtime catalog path + install-root locator path
  -> installed_path_details_from_paths(...)
       -> select fresh runtime/build catalog
       -> require locator catalogRevision equality
       -> canonicalize root and source manifest
       -> require source manifest inside root
       -> parse and validate every manifest row
       -> verify every portable executable against exact layout
       -> select requested app evidence
  -> safe UTF-8 display DTO
  -> labeled read-only panel
```

## Public Contract

Request:

```json
{
  "appId": "port-manager"
}
```

Portable response shape:

```json
{
  "appId": "port-manager",
  "mode": "portable",
  "executable": "<canonical-root>/apps/port-manager/versions/0.2.2/port-manager.exe",
  "installRoot": "<canonical-root>",
  "sourceManifest": "<canonical-root>/registry.json"
}
```

Installer response shape:

```json
{
  "appId": "port-manager",
  "mode": "installer",
  "executable": null,
  "installRoot": null,
  "sourceManifest": "<canonical-root>/registry.json"
}
```

Errors are fixed Korean messages and never contain the rejected locator, manifest, executable, parser error or OS
error. The frontend cannot provide any of those paths.

## Validation Sequence

1. Reject an app ID that is not manager-visible or is self-managed.
2. Resolve the platform runtime catalog and versioned locator paths.
3. Select the newer valid runtime catalog or build catalog with existing catalog policy.
4. Parse a schema-v1 locator with positive registry/catalog revisions and bounded root ID.
5. Require selected catalog revision to equal locator `catalogRevision`.
6. Require literal root/manifest to be absolute and free of env markers, tilde and dot segments.
7. Reject symlinked root or source manifest.
8. Canonicalize root and reject filesystem root, user home or current working directory.
9. Require canonical literal equality so aliases cannot be displayed as provenance.
10. Require source manifest to be a canonical regular file inside root.
11. Parse the manifest as an array with no unknown fields.
12. Reject invalid/duplicate app IDs, invalid semantic version triples and unknown modes.
13. Require installer `exe_path` to be empty.
14. For every portable row, require an absolute, non-symlink executable.
15. Derive `<root>/apps/<id>/versions/<version>/<id>.exe` and require canonical equality.
16. Require every manifest app to exist in the selected catalog.
17. Return only the requested installed target.
18. Require the returned source manifest to equal the canonical manifest used by the current Manager list.

Validating the whole manifest prevents a valid-looking target row from being displayed while the source manifest as
a whole is corrupt or has unknown ownership.

## Backend Changes

### `crates/launch`

Added `InstalledPathDetails`:

- `app_id`
- `mode`
- `executable: Option<PathBuf>`
- `install_root: Option<PathBuf>`
- `source_manifest: PathBuf`

`InstalledManifest` now retains validated mode, canonical root and canonical source manifest in addition to the
existing app ID and portable executable maps. `installed_targets` and process launch behavior are unchanged.

Added `installed_path_details_from_paths` as a pure read-only query. It has no process or write dependency and takes
all filesystem paths explicitly for deterministic tests.

### Devbox Manager command

Added `InstallPathView` and `install_path`. The command obtains only platform-owned metadata locations, calls the
shared resolver and converts canonical paths to UTF-8. Non-UTF-8 paths fail with a fixed message rather than using a
lossy display that could conceal identity differences.

## Frontend Changes

Added `InstallPathInfo` and `installPath(appId)`. The non-Tauri mock supplies a portable fixture so browser development
shows the completed panel.

The app provides two entry points:

- installed row `Paths` button with an app-specific accessible name
- context menu `설치 경로 정보`, enabled for both portable and installer records

The panel includes:

- selected catalog display name
- `읽기 전용` badge
- `Executable` label and value/unavailable reason
- `Install root` label and value/unavailable reason
- `Source manifest` label and value
- explicit validation/no-mutation explanation
- installer-specific ownership explanation
- close button

Long path text uses `overflow-wrap: anywhere` and remains text-selectable. It does not become a clickable link.

The path query participates in the existing operation guard. While it is pending, all row lifecycle/context actions
are disabled and the selected row shows `...` in the Paths button. A regular refresh or any lifecycle operation clears
the previous path evidence so stale values are not retained after state changes.

## Security Review

| Threat | Control |
|---|---|
| arbitrary frontend path inspection | IPC accepts catalog app ID only |
| stale/forged runtime metadata | selected catalog and locator revisions must match |
| manifest escape | canonical source manifest must be a regular file inside canonical root |
| symlink/reparse alias | shared locator loader rejects symlink root/manifest/executable and canonical mismatch |
| executable substitution | exact derived app/version executable equality |
| unknown manifest ownership | every row must match selected catalog |
| same app ID from a different root | source manifest must match the active Manager-list manifest |
| path/error reflection | fixed public errors; raw parser/OS/path errors discarded |
| unsafe installer inference | executable/root stay `null` |
| accidental mutation | injected-path fixture snapshots locator, manifest and executable bytes |
| lifecycle privilege expansion | existing path-free DTO/action contracts unchanged |

## Tests

### Rust `launch`

- portable returns exact canonical executable, root and source manifest
- installer returns no executable/root and the same verified source manifest
- manifest, locator and executable bytes remain unchanged after both queries
- catalog revision mismatch returns `InvalidLocator`
- a different active Manager manifest returns `UnsafeManifest` without reflecting its path
- a symlinked locator returns `InvalidLocator` without reflecting its path
- existing unsafe root/manifest/executable, traversal, dot-segment, symlink, catalog and fallback fixtures remain

Current result:

```text
cargo test -p launch --jobs 1
21 passed; 0 failed
```

### Rust Devbox Manager

The new command compiles through the registered Tauri handler and all existing batch, managed-install, runtime
metadata, download and lifecycle fixtures remain green.

```text
cargo test -p devbox-manager --jobs 1
51 passed; 0 failed
```

### Frontend

- portable Paths action calls the exact app ID
- all three verified paths render in a labeled region
- `읽기 전용` state is visible
- install, open-folder and remove APIs are not called by the display action
- installer executable/root render the unavailable reason twice
- installer ownership explanation renders
- existing 8 context-menu and 2 batch tests remain green

```text
pnpm --filter devbox-manager exec vitest run --maxWorkers=1
Test Files  1 passed (1)
Tests       12 passed (12)

pnpm --filter devbox-manager build
TypeScript compile: passed
Vite production build: passed
```

## Bundle

| Asset | #274 main | #275 | Delta |
|---|---:|---:|---:|
| JS | 218,297 B | 220,407 B | +2,110 B |
| JS gzip (`gzip -n`) | 67,954 B | 68,501 B | +547 B |
| CSS | 5,721 B | 6,372 B | +651 B |
| CSS gzip (`gzip -n`) | 1,834 B | 1,982 B | +148 B |

No package, Rust dependency, sidecar, runtime download, storage schema or Tauri capability was added.

## Files

- `crates/launch/src/installed.rs`
- `crates/launch/src/lib.rs`
- `apps/devbox-manager/src-tauri/src/commands/manager.rs`
- `apps/devbox-manager/src-tauri/src/lib.rs`
- `apps/devbox-manager/src/types.ts`
- `apps/devbox-manager/src/api.ts`
- `apps/devbox-manager/src/App.tsx`
- `apps/devbox-manager/src/App.css`
- `apps/devbox-manager/src/App.test.tsx`
- `apps/devbox-manager/README.md`
- `docs/architecture.md`
- `docs/roadmap.md`
- `docs/product-opportunities.md`
- `docs/superpowers/specs/2026-08-15-ux-improvements-design.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

## PR-wide Gates

- `cargo fmt --all --check`: passed
- `cargo clippy -p launch -p devbox-manager --all-targets --jobs 1 -- -D warnings`: passed
- `cargo test --workspace --jobs 1`: passed
- `cargo check --workspace --jobs 1`: passed
- `NODE_OPTIONS=--max-old-space-size=768 pnpm -r --workspace-concurrency=1 build`: passed
- `pnpm install --frozen-lockfile --prefer-offline`: passed, cache-only reuse
- `pnpm audit --audit-level moderate`: no known vulnerabilities
- dependency notices and dependency policy regression tests: passed
- build-manifest notice tests: passed
- catalog consistency: passed
- `cargo deny --locked check`: advisories, bans, licenses and sources passed
- GitHub Actions Linux, Windows, frontend, dependency and catalog gates: PR에서 확인 예정

## W1 Packaged Checkpoint

- portable row displays Windows canonical executable/root/source manifest without clipping the panel
- values match the packaged Manager locator and manifest evidence
- installer row shows two unavailable fields and source manifest without probing Program Files/registry
- Paths/query does not change manifest/current/locator timestamps or bytes
- corrupt locator revision or missing executable produces a fixed recoverable error with no raw rejected path
- keyboard context menu opens the exact row, selects `설치 경로 정보`, restores focus and permits panel close

## Next

#276 WSL Desktop Docker compact 표시 is the next P1-09 feature. Custom install root selection and movement remain
P2, and install/remove behavior remains outside #275.
