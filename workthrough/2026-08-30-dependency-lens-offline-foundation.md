# Dependency Lens Offline Foundation

## Overview

GitHub issue #484의 첫 reviewable boundary로 Repo Manager에 bounded offline dependency
inventory를 추가하고, source를 포함하지 않는 `dependency-summary/v1` snapshot을 통해 Workbench의
`Dependencies > Packages` 요약까지 연결했다. 분석은 lockfile과 로컬 manifest만 읽으며 package
manager, shell, build script, registry 또는 network를 실행하지 않는다.

## Context

- Dependency Lens는 별도 신규 앱보다 Repo Manager 상세 패널에서 먼저 검증하기로 했다.
- Repo Manager는 package 이름과 graph를 보여 줄 수 있지만, Workbench에는 프로젝트 경로와
  package-level 세부 정보를 복제하지 않는 aggregate만 필요했다.
- 지원 범위는 Cargo.lock v3/v4, pnpm lock v5~v9, package-lock v1~v3, uv.lock v1과 각 local
  manifest다. Gradle은 감지하되 현재 parser 미지원 상태로 구분한다.
- registry advisory/license/latest-version enrichment와 update/install mutation은 별도 #484
  후속 PR 경계다.

## Changes Made

### 1. Stable source-omitting project identity

- `crates/integration/src/lib.rs`
  - canonical source를 snapshot에 넣지 않고 namespace-separated SHA-256 기반 opaque ID로
    변환하는 `opaque_identity`를 추가했다. 이 ID는 snapshot 경계에서 source를 생략한 채
    안정적인 correlation을 제공할 뿐, 비밀값·암호학적 익명성·추측 불가능성을 보장하는
    credential 대체 수단은 아니다.
  - namespace/source 길이와 control character를 제한하고 lowercase hex를 고정했다.
- `crates/integration/Cargo.toml`
  - workspace lock에 이미 존재하는 `sha2 0.11.0`을 직접 사용하도록 선언했다.
- `Cargo.lock`
  - 새 package resolution 없이 integration/repo-manager의 기존 package dependency edge만
    갱신했다.

### 2. Repo Manager bounded offline parser and detail panel

- `apps/repo-manager/src-tauri/src/core/dependency_lens.rs`
  - Cargo, pnpm, npm, uv/Python lock graph를 deterministic DTO로 정규화했다.
  - `package.json`의 dependencies/devDependencies/optionalDependencies/peerDependencies와
    `packageManager`를 manifest-declared direct 후보 분류에 반영하고, npm nested version은
    root 위치와 exact resolved version을 함께 확인해 가능한 범위에서 direct를 판정한다.
    package-manager의 모든 resolver semantics를 재현하거나 모든 direct 판정을 exact하다고
    주장하지 않는다.
  - pnpm lock v5~v9의 slash/`@`/peer 표기와 importer가 기록한 direct version을 처리하며,
    URI-shaped version과 안전하지 않은 package name은 거부한다. pnpm v9 `packages`의
    `peerDependencies` range-only metadata는 추측성 edge나 unresolved edge count로 확장하지 않고
    의도적으로 생략하며, 해소된 `dependencies`/`optionalDependencies` edge는 `snapshots`에서 읽는다.
  - direct/transitive package, resolved edge, duplicate versions, missing/stale/invalid/unsupported
    source와 truncation을 계산한다.
  - depth 8, 방문 directory 10,000개·directory당 entry 10,000개, invalid/oversized input을
    포함한 input 256개, file 4 MiB, total 24 MiB, line 16 KiB, package 4,096개,
    resolved/unresolved reference를 합친 edge 16,384개로 제한한다.
  - nested Git repository와 symlink/reparse point를 건너뛰고 directory/file identity 및 read
    전후 안정성을 no-follow handle과 metadata로 재검증한다.
  - URL, integrity/hash, credential metadata는 parser DTO에 복사하지 않는다.
- `apps/repo-manager/src-tauri/src/commands.rs`
  - 선택 repository를 재검증하는 `dependency_inventory` command를 추가했다.
  - process-wide try-lock single-flight와 10초 filesystem budget을 적용하고 snapshot publication은
    local detail 결과와 분리된 best-effort로 유지한다.
- `apps/repo-manager/src-tauri/src/core/mod.rs`
- `apps/repo-manager/src-tauri/src/lib.rs`
  - 새 core module과 Tauri command를 등록했다.
- `apps/repo-manager/src-tauri/Cargo.toml`
  - 공용 integration crate와 기존 lock의 `sha2 0.11.0`, `toml 1.1.4`를 연결했다.
- `apps/repo-manager/src/api.ts`
  - typed dependency report DTO, fixed public error, native/browser wrapper를 추가했다.
- `apps/repo-manager/src/components/DependencyLensPanel.tsx`
- `apps/repo-manager/src/App.tsx`
- `apps/repo-manager/src/App.css`
  - 사용자의 명시적 `의존성 분석` action, source 진단, aggregate metrics, duplicate versions,
    filterable graph inventory와 duplicate/package/expanded-edge DOM 상한을 제공한다.
  - repository 전환/unmount의 늦은 응답은 request sequence로 폐기한다.
- `apps/repo-manager/src/components/DependencyLensPanel.test.tsx`
  - exact repository invocation, source/duplicate/edge 표시, client filter, fixed error와 stale response
    폐기를 검증한다.

### 3. Versioned summary producer and Workbench consumer

- Repo Manager는 `/integration/repo-manager/v1/summary.json`의
  `dependency-summary/v1` view에 source를 포함하지 않는 project opaque ID, opaque input revision,
  scan timestamp와 aggregate counts만 atomic replace한다. package names, relative/absolute paths와
  source metadata는 제외한다. direct count는 manifest/importer가 선언한 후보를 lockfile에 연결해
  계산한 분류이며, 범용 resolver의 exact 결과를 의미하지 않는다.
  project 256개, 90일 보존 상한을 적용하고 기존 다른 view를 보존한다.
- `apps/workbench/src-tauri/src/core/dependency_summary.rs`
  - 전체 view를 deny-unknown DTO와 동일한 count/identity bound로 먼저 검증한 뒤 선택 profile의
    canonical identity digest와 일치하는 entry 하나만 반환한다.
  - per-project `scannedAtMs` 기준 fresh(24시간), stale(7일), expired를 판정하고 missing/corrupt를
    구분한다.
- `apps/workbench/src-tauri/src/commands/dependencies.rs`
  - profile ID와 현재 profile store의 canonical identity를 native에서 다시 확인하는 read-only
    `package_dependency_summary` command를 추가했다.
- `apps/workbench/src-tauri/src/commands/mod.rs`
- `apps/workbench/src-tauri/src/core/mod.rs`
- `apps/workbench/src-tauri/src/lib.rs`
  - consumer module과 command를 등록했다.
- `apps/workbench/src/api.ts`
- `apps/workbench/src/components/PackageDependencySummaryPanel.tsx`
- `apps/workbench/src/App.tsx`
- `apps/workbench/src/App.css`
  - 기존 app/distro/path/port/service dependency health를 `Environment`, Repo Manager aggregate를
    `Packages`로 분리했다.
  - summary 상태, aggregate counts, ecosystem counts와 Repo Manager 분석 안내만 보여 주며
    Workbench에서 repository나 package manager를 열지 않는다. 로딩 상태는 `aria-busy`로
    노출한다.
- `apps/workbench/src/components/PackageDependencySummaryPanel.test.tsx`
- `apps/workbench/src/App.test.tsx`
- `apps/workbench/src/App.applink.test.tsx`
  - aggregate rendering, missing guidance, native-error redaction, profile switch race와 기존 App flow
    compatibility를 검증한다.

### 4. Catalog and architecture contract

- `apps/catalog.json`
  - catalog revision을 13으로 올리고 Repo Manager의
    `snapshot:repo-manager/dependency-summary/v1` producer capability를 선언했다.
- `crates/catalog/tests/catalog.rs`
  - repository catalog revision과 exact producer lookup을 고정했다.
- `apps/devbox-manager/src-tauri/src/core/catalog.rs`
  - 전체 workspace test에서 발견된 stale catalog revision 기대값을 13으로 갱신하고 Repo Manager의
    새 producer capability를 Manager의 build-time catalog adapter에서도 고정했다.
- `THIRD_PARTY_NOTICES.md`
  - 공식 dependency-policy generator로 변경된 `Cargo.lock` digest를 재생성했다. package inventory
    자체는 바뀌지 않았다.
- `apps/repo-manager/README.md`
- `apps/workbench/README.md`
- `docs/architecture.md`
  - parser/snapshot/privacy/freshness/failure 경계, offline scope와 후속 network scope를 기록했다.
  - 기능 PR은 버전을 유지하고 #493 v0.6.0 release preparation에서 Repo Manager와 Workbench를
    minor bump 대상으로 처리한다.

### 5. Dependency policy record

- `sha2 0.11.0` ([crates.io](https://crates.io/crates/sha2), RustCrypto, MIT OR Apache-2.0): 같은 canonical identity를 두 앱에서 중복
  구현하지 않고 공용 snapshot 경계에서 안정적으로 digest하기 위해 사용한다. 이미 workspace
  lock/notices와 다른 shipped app에 존재하며 새 package, runtime download, sidecar 또는 resource를
  추가하지 않는다.
- `toml 1.1.4` ([crates.io](https://crates.io/crates/toml), toml-rs, MIT OR Apache-2.0): Cargo.lock/Cargo.toml/uv.lock/pyproject의 공개 TOML
  문법을 bespoke partial parser 없이 bounded parse하기 위해 사용한다. 이미 workspace lock/notices에
  존재하며, 다른 workspace consumer의 TOML dependency와는 별도 resolved package일 수 있다.
- 대안인 수동 TOML parser는 quoting/table/array edge case에서 fail-open risk와 유지보수 비용이
  커서 제외했다. SHA 구현 복제도 producer/consumer identity drift 위험 때문에 제외했다.
- 새 advisory exception은 추가하지 않았다. dependency-policy/cargo-deny는 기존 pinned package를
  그대로 검사한다. 새 resolved package 수와 별도 설치 resource 증가는 0이며, Windows static binary
  delta와 최종 asset 크기는 #493 packaged release evidence에서 기록한다.

## Code Examples

### Explicit offline analysis boundary

```rust
// apps/repo-manager/src-tauri/src/commands.rs
let _analysis = dependency_analysis_lock()
    .try_lock()
    .map_err(|_| DEPENDENCY_LENS_ERROR.to_string())?;
let context = validated_repository_context(&request.path, DEPENDENCY_LENS_ERROR)?;
let mut report = analyze_repository(&context.worktree, Duration::from_secs(10))?;
revalidate_repository_context(&context, DEPENDENCY_LENS_ERROR)?;
```

### Source-omitting cross-app identity

```rust
// crates/integration/src/lib.rs
digest.update(namespace.as_bytes());
digest.update([0]);
digest.update(canonical_source.as_bytes());
```

### Workbench per-project freshness

```rust
// apps/workbench/src-tauri/src/core/dependency_summary.rs
let freshness_ms = now_ms.saturating_sub(entry.scanned_at_ms);
let status = if freshness_ms <= FRESH_MAX_MS {
    PackageDependencyStatus::Fresh
} else if freshness_ms <= EXPIRED_AFTER_MS {
    PackageDependencyStatus::Stale
} else {
    PackageDependencyStatus::Expired
};
```

## Verification Results

### Offline dependency setup

```text
pnpm install --frozen-lockfile --offline
Scope: all 20 workspace projects
Lockfile is up to date, resolution step is skipped
Already up to date
Exit code: 0
```

The offline frozen install completed from the existing pnpm store without network access or lockfile changes.

### Frontend tests and builds

```text
pnpm --filter repo-manager test -- src/components/DependencyLensPanel.test.tsx --maxWorkers=1
Test Files 1 passed; Tests 5 passed

pnpm --filter workbench test -- src/components/PackageDependencySummaryPanel.test.tsx --maxWorkers=1
Test Files 1 passed; Tests 4 passed

pnpm --filter workbench test -- src/App.test.tsx src/App.applink.test.tsx --maxWorkers=1
Test Files 2 passed; Tests 44 passed

pnpm --filter repo-manager build
TypeScript + Vite build passed

pnpm --filter workbench build
TypeScript + Vite build passed
```

### Catalog contract

```text
bash .github/scripts/check-catalog.sh
WINDOWS PACKAGED SMOKE CONFIG OK: release catalog and 15 app contracts align
VERIFY DOWNLOADED RELEASE TESTS OK
windows installer acceptance config: PASS
Exit code: 0
```

### Focused Rust regression

```text
cargo test -p repo-manager dependency_lens -j2
23 passed; 0 failed
```

The focused Dependency Lens test module includes the checked-in monorepo offline smoke fixture and
the parser-boundary regression cases; both pass. This is targeted evidence only, not final
workspace-wide release evidence.

### Full Linux workspace verification

```text
cargo check --workspace -j2
passed

cargo clippy --workspace --all-targets -j2 -- -D warnings
passed

cargo fmt --all -- --check
passed

cargo test --workspace -j2
passed, including doc-tests and the checked-in dependency scan smoke

pnpm -r --workspace-concurrency=2 build
Scope: 19 of 20 workspace projects; passed

CI=1 pnpm -r --workspace-concurrency=1 test
Test Files: 150 passed; Tests: 1,262 passed

bash .github/scripts/run-frontend-scope.sh typecheck all ''
passed
```

The first full Rust test run exposed one stale Devbox Manager catalog revision assertion (12 versus
the new repository revision 13). The assertion and exact producer capability were corrected, its focused
regression passed, and the complete workspace test was rerun successfully.

### Dependency, catalog, and release-contract checks

```text
pnpm audit --audit-level moderate
No known vulnerabilities found

cargo deny --locked check
advisories ok, bans ok, licenses ok, sources ok

python3 .github/scripts/check-dependencies.py check
python3 .github/scripts/test-check-dependencies.py
python3 .github/scripts/test-build-manifest.py
python3 .github/scripts/test-validate-release-input.py
bash .github/scripts/check-catalog.sh
passed

git diff --check
passed
```

`cargo-deny` retained the repository's existing allowed duplicate-version warnings; it added no new
advisory exception and all four enforced policy categories passed.

### Remaining packaged verification

Windows compile CI and packaged interaction acceptance cannot be substituted by the WSL checks above.
They remain pending: Windows compile/test/Clippy runs on this PR's required GitHub Actions, while final
installer/bundle interaction and asset-size evidence belongs to #493 v0.6.0 release preparation. This
workthrough does not claim packaged-release verification.

## Next Steps

- Complete #484 network advisory/license/latest-version enrichment as a separate PR with its own consent,
  timeout, caching and rollback contract.
- Run Windows packaged interaction acceptance and record final bundle size during #493 release preparation.
- Apply Repo Manager and Workbench minor version bumps together with their Cargo/package/Tauri versions in
  #493; this feature PR intentionally does not create partial version triples.
