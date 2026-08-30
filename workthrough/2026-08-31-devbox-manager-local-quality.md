# Devbox Manager local-quality inspection (#491)

## Overview

Devbox Manager에 명시적 새로고침 기반의 로컬 품질 검사를 추가했다. 검사는 catalog와 검증된
설치 registry, `crates/integration` discovery 결과를 경로·원문 오류 없이 bounded한
`schemaVersion: 1` / `mode: local-only` DTO로 투영해 현재 메모리에만 표시한다. 설치·네트워크·
telemetry·local-quality persistence는 이 경계에 포함하지 않는다.

## Context

- Manager에는 설치/환경 진단과 별도로 현재 설치 계약과 앱 간 snapshot 계약을 함께 확인하는
  read-only 화면이 필요했다.
- registry를 읽을 수 없을 때 앱을 `not-installed`로 표시하면 실제 부재와 관찰 불가를 혼동하므로
  `unknown`을 별도 상태로 유지해야 했다.
- integration discovery의 path, generatedAt, raw error는 민감하거나 안정적인 public UI 계약이
  아니므로 고정 issue enum으로 분류하고 제거해야 했다.
- renderer는 native 응답을 신뢰하지 않고 exact key, 타입, count/truncation, state/root/status
  관계를 검증해야 했다.
- Windows screen reader/high-contrast와 packaged acceptance는 소스 PR의 자동화 범위가 아니라
  #493에 남겨야 했다.

## Changes Made

### 1. Native bounded local-quality projection

- `apps/devbox-manager/src-tauri/src/core/local_quality.rs`
  - `schemaVersion: 1`, `mode: local-only`, `healthy`/`attention` 상태와 ready/unavailable
    source 상태를 정의했다.
  - catalog app ID와 validated registry의 version/mode만 사용해 path-free installation
    health를 만든다. registry가 없거나 불일치하면 모든 app을 `unknown`으로 만든다.
  - `DiscoveryReport`를 snapshot/view/issue summary로 투영하면서 path, generatedAt, raw
    errors를 제거하고 `invalid`, `unreadable`, `unsafe`, `limit-exceeded`만 공개한다.
  - installation 64, snapshots 64, issues 64, views/snapshot 16, serialized output 256 KiB
    cap과 truncation/status 규칙을 적용했다.
  - privacy, unavailable-registry, invalid registry, issue classification, bound 회귀를 포함한
    8개 Rust 테스트를 추가했다.
- `apps/devbox-manager/src-tauri/src/commands/local_quality.rs`
  - explicit `inspect_local_quality` command를 `spawn_blocking`으로 실행한다.
  - local catalog/registry와 integration discovery만 읽고, 결과를 serialize해 256 KiB를
    넘으면 고정 오류로 닫는다.
- `apps/devbox-manager/src-tauri/src/commands/manager.rs`
  - Manager-visible catalog와 validated registry에서 local-quality에 필요한 identity만
    반환하는 observation helper를 추가했다. executable/root/manifest/locator path는 복사하지
    않는다.
- `apps/devbox-manager/src-tauri/src/commands/mod.rs`, `src-tauri/src/core/mod.rs`,
  `src-tauri/src/lib.rs`
  - command/core module과 Tauri command registration을 연결했다.
- `apps/devbox-manager/src-tauri/Cargo.toml`, `Cargo.lock`
  - 기존 workspace `integration` crate를 Manager command에 연결했다.

### 2. Frontend contract and UI behavior

- `apps/devbox-manager/src/types.ts`
  - local-quality envelope, installation, integration, snapshot/view/issue DTO와 고정 enum을
    TypeScript로 선언했다.
- `apps/devbox-manager/src/api.ts`
  - browser-only bounded fixture를 제공하고, native/mock 응답을 React로 전달하기 전에 exact
    keys, safe numbers, bounded IDs/SemVer, duplicate keys, collection truncation과 cross-field
    relationships를 검증한다.
  - root unavailable, registry unavailable, installed count, state/status contradictions를
    fail-closed하며 native 오류는 고정 메시지로 바꾼다.
- `apps/devbox-manager/src/App.tsx`
  - tab 진입만으로 검사하지 않고 `상태 새로고침` 버튼에서만 호출한다.
  - region의 `aria-busy`, last-good snapshot 보존, fixed error, request ID와 mounted guard를
    적용해 late/previous/unmounted response를 폐기한다.
- `apps/devbox-manager/src/App.css`
  - local-quality summary/table/snapshot/issue 상태와 responsive layout을 추가했다.
- `apps/devbox-manager/src/App.test.tsx`, `apps/devbox-manager/src/api.test.ts`
  - explicit refresh, accessible render, path-free output, last-good preservation, fixed error,
    unmount guard, exact response rejection과 unavailable-registry `unknown`을 회귀 검증한다.

### 3. Documentation and release boundary

- `docs/superpowers/specs/2026-08-31-devbox-manager-local-quality.md`
  - public contract, resource limits, renderer trust boundary, async/a11y behavior와 acceptance
    boundary를 고정했다.
- `apps/devbox-manager/README.md`
  - local-quality tab의 explicit/local-only/path-free/privacy와 browser mock/Windows acceptance
    경계를 기능 목록에 추가했다.
- `docs/architecture.md`
  - Manager data flow에 catalog/registry/discovery projection과 bounded memory contract를
    기록했다.
- `docs/roadmap.md`
  - W10 PR B/#491 구현 상태와 #493 packaged acceptance, no-version-bump 경계를 추가했다.

## Code Examples

### Path-free native projection

```rust
// apps/devbox-manager/src-tauri/src/core/local_quality.rs
let installation = build_installation_health(catalog, registry);
let integration = build_integration_health(discovery);
LocalQualitySnapshot {
    schema_version: 1,
    mode: "local-only",
    installation,
    integration,
    ..
}
```

The projection retains only catalog/registry identity and integration summary fields; path,
`generatedAt`, and raw native errors do not cross the DTO boundary.

### Relationship validation before render

```typescript
// apps/devbox-manager/src/api.ts
if (value.registryState === "unavailable") {
  if (value.registryRevision !== null
      || value.installedAppCount !== null
      || apps.some((app) => app.state !== "unknown")) {
    throw new Error(LOCAL_QUALITY_RESPONSE_ERROR);
  }
}
```

The same validator enforces exact keys, count/truncation relationships, unique producer/view keys,
root-state consistency, and status consistency.

### Last-good and late-response guard

```tsx
// apps/devbox-manager/src/App.tsx
const requestId = ++localQualityRequestIdRef.current;
try {
  const snapshot = await inspectLocalQuality();
  if (mountedRef.current && requestId === localQualityRequestIdRef.current) {
    setLocalQualitySnapshot(snapshot);
  }
} catch {
  if (mountedRef.current && requestId === localQualityRequestIdRef.current) {
    setLocalQualityError("최신 로컬 품질 상태를 확인하지 못했습니다. 이전 결과를 유지합니다.");
  }
}
```

## Verification Results

### Targeted implementation evidence

```text
Devbox Manager frontend tests (2 files)       71/71 passed   PASS
local-quality Rust tests                      8/8 passed     PASS
Manager production bundle                    PASS
  JavaScript raw                              312992 bytes (budget 345000)
  JavaScript gzip                              93326 bytes (budget 105000)
15-app accessibility contract                PASS
git diff --check                              PASS
```

The figures above are the targeted evidence after the stricter local-quality boundary tests.

### Repository-wide evidence

```text
Frontend workspace tests                    1488/1488 passed
All 15 frontend production builds           PASS
All 15 frontend bundle budgets              PASS
15-app accessibility contract               PASS
Rust workspace check                        PASS
Rust workspace clippy --all-targets -D warnings
                                             PASS
Rust workspace fmt --check                   PASS
Rust workspace tests                        PASS
Catalog/dependency/notices/checker fixtures  PASS
pnpm audit --audit-level moderate            0 known vulnerabilities
cargo-deny advisories/bans/licenses/sources  PASS
```

The dependency inventory generator updated only the `Cargo.lock` digest in
`THIRD_PARTY_NOTICES.md`, reflecting the existing workspace `integration` crate's new Manager edge.
Cargo-deny still reports the repository's pre-existing duplicate/yanked warnings; they do not fail
the checked policy and remain release follow-up input.

### Pending validation and acceptance

- GitHub Actions CI remains pending for the PR.
- Physical Windows screen-reader, high-contrast, and packaged acceptance remains #493.
- No RC/tag/release/version bump is created by this feature PR.

## Next Steps

- Push the reviewed branch and require all GitHub Actions checks before merge.
- Exercise the packaged Windows screen-reader/high-contrast/local-quality flow under #493.
- Keep version and release changes deferred to the designated release-preparation work.
