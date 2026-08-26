# Devbox Manager safe app removal (#309) 구현 기록

## Overview

Devbox Manager의 portable 앱 제거를 안전한 preview/confirm/mutate 흐름으로 완성했다.
Manager가 실제로 소유한 active-root binary tree만 대상으로 하며, manifest의 경로 문자열을
그대로 삭제 path로 사용하지 않는다. 현재 catalog와 locator provenance, manifest digest, exact
layout을 native에서 재검증하고, 사용자 data·installer wizard가 소유한 위치·foreign entry는
건드리지 않는다.

이 기록은 #308 custom install-root가 이미 반영된 전용 worktree에서 #309를 이어서 구현한
내용이다. pre-squash #308 commit은 replay하지 않고 최신 main 위에 #309 단일 commit만 올렸으며,
PR #429에서 검증 중이다.

## Context

기존 Manager의 portable 제거는 registry app ID를 찾은 뒤 resolved install을 단순 삭제하는
경계였다. #309의 요구사항은 다음을 분리하는 것이다.

- 실행 파일과 Manager metadata는 제거하되 앱 사용자 data는 보존한다.
- default root와 #308 custom root를 같은 안전 규칙으로 처리한다.
- registry path, traversal, symlink/junction/reparse, 특수 파일과 foreign entry를 신뢰하지
  않는다.
- 삭제 전에 사용자가 대상과 보존 범위를 확인하고, stale UI 또는 partial failure를 복구한다.

## Changes Made

### 1. Exact, bounded removal core

File: `apps/devbox-manager/src-tauri/src/core/removal.rs`

- `inspect_portable_removal`은 catalog-safe app ID, bounded version, absolute registry
  executable과 canonical active root에서 expected path를 계산한다.
- 허용 tree는 `apps/<app>/current.json`, `versions/<version>/` 아래의 `<app>.exe`와
  `<app>.exe.partial`뿐이다. sibling/foreign entry, symlink/reparse, special file,
  traversal과 non-plain component는 preflight에서 거부한다.
- depth 16, entry 10,000, current metadata 64 KiB bounds를 적용하고, owned path를
  deepest-first로 정렬한다.
- `remove_portable_tree`는 recursive delete를 사용하지 않고 재검사된 exact 목록만
  `remove_file`/`remove_dir`로 처리한다. 경합·권한·잠금 실패는 bounded partial outcome과
  remaining count로 반환한다.
- target executable이 이미 없거나 일부 parent만 남은 interrupted tree는 `partial` 또는
  `missing`으로 표현해 같은 manifest evidence로 재시도한다.
- Windows의 canonical path가 extended-length prefix 또는 8.3 alias를 정규화하는 경우에도,
  deepest existing plain ancestor를 canonicalize한 뒤 missing tail만 다시 붙여 exact derived
  identity를 비교한다. device path·link/reparse·다른 layout은 계속 거부한다.
- filesystem root와 환경의 home/workspace/current directory를 보호해 broad root를 삭제
  대상으로 삼지 않는다.

### 2. Manifest snapshot, CAS and recovery

Files: `apps/devbox-manager/src-tauri/src/core/custom_root.rs`,
`apps/devbox-manager/src-tauri/src/commands/manager.rs`

- active manifest의 exact bytes와 SHA-256 digest를 `InstallManifestSnapshot`으로 보존한다.
- 정상 lifecycle parser는 모든 portable executable 존재를 요구한다. interrupted removal
  복구 parser/writer는 exact derived final executable이 없는 경우만 추가로 허용하고, strict
  schema, absolute path, canonical identity, intermediate link/reparse 검사는 유지한다.
- `preview_remove_app`은 current locator/manifest/catalog와 app-owned tree를 read-only로
  확인해 target path, version, owned count/size, state를 DTO로 반환한다.
- `remove_portable_app`은 process-wide mutex를 잡고 preview token의 registry revision,
  catalog revision, root ID와 manifest digest를 재검증한다.
- manifest에서 target record를 atomic claim한 뒤 tree를 제거한다. mutation 중 path/tree가
  바뀌면 중단하며, partial/error 때 manifest가 여전히 claim digest일 때만 원래 bytes를
  복원한다. 다른 writer의 manifest는 덮어쓰지 않는다.
- installer record는 실제 설치 위치·uninstaller ownership이 없어 항상 fail-closed한다.

### 3. Frontend preview/confirm UX

Files: `apps/devbox-manager/src/api.ts`, `src/types.ts`, `src/App.tsx`, `src/App.css`,
`src/App.test.tsx`

- context menu의 `제거`는 native preview만 호출하고, panel에 canonical target, 방식/version,
  Manager-owned size/count와 user-data 보존을 표시한다.
- `확인 후 제거` 버튼에서만 별도 confirm 뒤 CAS request를 보낸다. stale generation,
  single-flight busy ref, mounted guard로 중복/늦은 응답을 차단한다.
- stale rejection은 오래된 preview와 confirm button을 폐기하고 최신 preview 재확인 메시지만
  남긴다. partial result는 남은 count와 retry 안내를 표시한다.
- installer row에는 removal action을 활성화하지 않고 Manager가 uninstaller를 추적하지
  않는다는 경계를 표시한다. browser mock은 UI 흐름 전용이다.

### 4. Documentation synchronization

다음 문서에 #309 contract와 운영 경계를 반영했다.

- `apps/devbox-manager/README.md`: 사용자 기능, custom root, portable/installer removal
  경계와 user-data 보존
- `docs/architecture.md`: preview/CAS/exact-tree/recovery 흐름과 locator ownership
- `docs/product-opportunities.md` §6.10: #308와 #309 분리, API와 non-scope
- `docs/roadmap.md`: 2026-08-27 #309 구현 상태
- `docs/superpowers/plans/2026-08-27-devbox-manager-safe-removal.md`: 구현 계획, DTO,
  security/test/W2 acceptance

## Code Examples

### Derived removal path

```rust
let expected = root
    .join("apps")
    .join(app_id)
    .join("versions")
    .join(version)
    .join(format!("{app_id}.exe"));
// registry_executable must match this identity; it is never used as an arbitrary delete path.
```

### Preview token and remove request

```text
preview_remove_app({ appId })
  -> { state, targetPath, ownedEntryCount, ownedBytes,
       registryRevision, catalogRevision, rootId, manifestDigest }

remove_portable_app({ appId, expectedRegistryRevision,
  expectedCatalogRevision, expectedRootId, expectedManifestDigest })
```

## Verification Results

Rust commands use the dedicated target directory, `CARGO_INCREMENTAL=0` and `-j2`. The final root
review repeated the app's all-target gates on latest main `927b62c`; full workspace and Windows
gates remain delegated to the required PR CI.

```text
cargo fmt --all -- --check                                               PASS
cargo test -p devbox-manager -j2                                         PASS (82 tests)
cargo clippy -p devbox-manager --all-targets -j2 -- -D warnings          PASS
cargo check -p devbox-manager --all-targets -j2                          PASS
pnpm --filter devbox-manager test -- --maxWorkers=2                      PASS (21 tests)
pnpm --filter devbox-manager exec tsc --noEmit                           PASS
pnpm --filter devbox-manager build                                       PASS (tsc + vite)
python3 .github/scripts/check-dependencies.py check                      PASS
git diff --check                                                         PASS
```

Frontend dependencies were already installed in the main workspace. A temporary symlink mirror
was used only to run this worktree's app tests and was removed afterward; no dependency symlink is
part of the commit.

Windows packaged W2 checks (NTFS junction/reparse, ACL/lock failure, packaged restart and actual
Tauri IPC) remain for the release checkpoint. Source worktree cleanup and merge remain pending
until corrected PR CI passes.

첫 PR CI에서 Linux Rust/frontend/dependency gates는 통과했지만 Windows unit test 네 건이
`std::env::temp_dir()`의 lexical spelling과 canonical long-path spelling 차이로 실패했다. 삭제
범위를 느슨하게 만들지 않고 existing ancestor identity를 정규화하도록 수정했으며, ordinary
manifest read도 existing executable은 lexical spelling이 아니라 양쪽 canonical identity로
검증하도록 바로잡았다. 수정 뒤 Linux 82 tests와 all-target gates를 다시 통과시켰고 Windows
CI를 동일 PR 새 head에서 재실행한다.

## Integration status

The single feature commit was rebased with `--onto` and then advanced to current main `927b62c`,
which contains merged #308 (`b3fe815`), #294, and #284. This avoids replaying the pre-squash #308
commit. PR #429 is open; merge and exact source-worktree/branch cleanup remain pending until its
corrected latest-head CI is green.
