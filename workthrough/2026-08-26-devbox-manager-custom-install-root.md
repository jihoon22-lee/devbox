# Devbox Manager custom install-root (#308) 구현 및 PR 전 감사 기록

## Overview

Devbox Manager에 사용자가 선택한 로컬 설치 root를 native에서 preview하고, 별도 확인과
`registryRevision` CAS 검증을 통과한 경우에만 다음 portable 설치 root로 적용하는 흐름을
추가했다. 기존 설치를 이동·병합·삭제하지 않고, 새 root가 이미 존재하는 canonical 빈
디렉터리인지 확인한 뒤 빈 app-owned manifest를 독점 생성·sync하고 locator pointer를
atomic publish한다.

이 문서는 전용 worktree의 초안, 후속 보강, 루트 PR 전 감사와 전체 Linux
workspace gate를 기록한다. 별도 dependency install과 Windows packaged acceptance는
수행하지 않았으며 GitHub PR/CI 상태는 저장소 기록으로 추적한다.

## Context

기존 Manager는 app-local data directory를 고정 설치 root로 사용했고, runtime metadata locator는
이미 versioned schema를 갖고 있었지만 사용자가 이를 안전하게 선택하거나 적용하는 UI/command가
없었다. 단순 path 문자열을 저장하면 symlink/reparse 탈출, root/home/workspace 오선택, stale
preview, 손상된 registry fallback, 기존 사용자 파일 덮어쓰기와 raw OS 오류 반향이 생길 수 있다.

#308에서는 이를 empty-root pointer 전환으로 한정했다. 기존 설치 migration, root reset, binary
제거와 user-data 삭제는 #309 후속 범위로 남겼다.

## Changes Made

### 1. Bounded custom-root core

File: `apps/devbox-manager/src-tauri/src/core/custom_root.rs`

- `preview_custom_root`와 `apply_custom_root`를 분리했다. preview는 read-only이며 apply는
  preview 결과를 신뢰하지 않고 path, active state, candidate, permission, free-space를 재검사한다.
- path 4,096 bytes, locator 16 KiB, manifest 1 MiB/256 rows, candidate direct entries 4,096개
  bounds를 둔다. strict JSON unknown field, duplicate app ID, invalid version/mode와 invalid
  portable path는 거부한다.
- Windows `canonicalize`가 반환할 수 있는 `\\?\\` 표현은 저장·비교 시 안전한 일반 drive/UNC
  identity로 정규화하고, case/separator 차이만 같은 identity로 취급한다. device alias 자체는
  입력/locator에서 계속 거부한다.
- absolute literal, `.`/`..`, environment expansion/device prefix, filesystem root,
  home/workspace/current directory, symlink/reparse component와 canonical alias를 fail-closed한다.
- locator가 없을 때만 legacy default root를 read-only fallback으로 사용한다. present corrupt
  locator와 valid locator 뒤의 manifest/path 오류는 별도 root 추측 없이 중단한다. locator parent가
  symlink/reparse인 missing path도 fallback으로 오인하지 않는다. default root ID가 다른 path를
  가리키는 locator도 거부한다.
- active manifest record 또는 `apps`·partial·기타 artifact가 있으면
  `existing-install`로 보고 기존 파일을 migration하지 않는다. candidate는 미리 존재하는
  빈 directory만 허용한다.
- Unix `statvfs`와 Windows `GetDiskFreeSpaceExW`로 free space를 읽고 최소 128 MiB와 write
  permission을 확인한다. 확인 불가/부족/권한 없음은 적용 불가 상태다.
- `registryRevision` CAS와 overflow를 확인한 뒤 새 root `apps/`를 만들고 빈 `registry.json`은
  `create_new`로 독점 생성해 flush/sync한다. preflight 뒤 나타난 기존 manifest는 대체하지 않고,
  candidate가 exact empty `apps/`와 two-byte manifest만 포함하는지 최종 재검증한 뒤 완성된
  후보를 가리키는 locator만 atomic replace한다. 실패 시 이번 invocation이 만든 빈 항목만 안전하게
  rollback한다.
  publish 뒤 manifest가 다른 writer에 의해 `[]` 이외의 내용으로 바뀌었다면 이를 삭제하지 않고
  `rollback-failed`로 종료해 외부 변경을 보존한다.

### 2. Active-root lifecycle and destination safety

Files: `apps/devbox-manager/src-tauri/src/commands/manager.rs`,
`apps/devbox-manager/src-tauri/src/core/managed_install.rs`,
`apps/devbox-manager/src-tauri/src/core/runtime_metadata.rs`,
`apps/devbox-manager/src-tauri/src/core/mod.rs`,
`apps/devbox-manager/src-tauri/src/lib.rs`

- Manager registry, install, current, rollback, launch, folder/path 조회와 partial cleanup이
  locator가 가리키는 active root를 사용하도록 연결했다.
- non-legacy active locator는 선택 catalog revision과 provenance가 일치해야 하며, registry의
  모든 app ID가 현재 manager-visible/non-self-managed catalog 대상인지 lifecycle read마다
  확인한다. startup sync가 실패한 stale custom state는 이후 command가 계속 사용하지 않는다.
- `preview_install_root`/`apply_install_root` DTO와 commands를 등록했다. public response/error에는
  locator/manifest/OS raw error나 credential을 넣지 않는다.
- custom root가 active인 동안 기존 remove command는 #309 별도 기능 안내로 fail-closed한다.
- portable/installer destination은 `create_dir_all` 대신 component별 생성·검사를 사용한다.
  final destination과 `.partial` sibling이 symlink/reparse/non-file slot이면 download 전에
  중단한다.
- startup partial cleanup은 임의의 `*.partial`을 재귀 삭제하지 않는다. build catalog에 있는
  Manager 대상의 `apps/<app>/versions/<strict-version>/<app>.exe.partial` exact slot만 최대
  256 apps·app당 256 versions 범위에서 수집한다. 전체 preflight가 성공한 뒤에만 삭제하므로
  link/reparse, 특수 파일, 읽기 실패 또는 과대 tree가 있으면 어떤 partial도 변경하지 않으며,
  같은 version directory의 사용자 소유 `*.partial`은 보존한다.
- startup은 custom locator와 source manifest를 mutation 전에 다시 검증한다. 유효한 custom
  root는 선택된 최신 catalog revision으로 locator provenance와 registry revision만 전진시키며
  root/path/manifest identity는 유지한다. locator revision이 선택 catalog보다 앞서거나 custom
  root가 안전하지 않으면 locator를 downgrade·보존·재작성하지 않고 원본 bytes를 유지한다.

### 3. Shared launch consumer bounds

File: `crates/launch/src/installed.rs`

- locator/parser에 16 KiB locator, 4,096-byte path bounds를 추가했다.
- launch consumer의 locator와 app-owned manifest read를 bounded file read로 바꾸고 manifest
  row 수를 256개로 제한했다. missing locator만 read-only legacy fallback을 사용하고, present
  invalid locator와 symlinked parent는 fail-closed한다.

### 4. Frontend preview/confirm UX

Files: `apps/devbox-manager/src/api.ts`, `apps/devbox-manager/src/types.ts`,
`apps/devbox-manager/src/App.tsx`, `apps/devbox-manager/src/App.css`,
`apps/devbox-manager/src/App.test.tsx`

- typed `previewInstallRoot`/`applyInstallRoot` API와 status DTO를 추가했다.
- input 변경 시 preview를 폐기하고 generation ID, busy ref, mounted guard로 stale async/unmount와
  duplicate action을 차단했다. React StrictMode에서 두 번째 live effect가 동작하도록 mounted
  guard를 reset한다.
- IME composition 중 Enter preview를 막고, 입력 중 preview/apply busy 상태는 field/action을
  비활성화한다.
- root preview/apply가 진행되는 동안 tab 전환, refresh, 환경 진단, app 행 action과 batch
  선택·실행·재시도도 같은 operation guard로 비활성화한다. 반대로 metadata refresh나 환경
  진단이 진행 중일 때도 별도 read single-flight ref/state가 root·app mutation을 막으며,
  mutation이 소유한 후속 refresh만 명시적 internal 호출로 허용한다.
- status, revision, canonical candidate, free-space, install count, candidate count와 fixed
  Korean error를 accessible live/status/alert region에 표시한다. existing-install 상태에는
  migration/remove action을 표시하지 않는다.
- browser API mock은 UI 흐름 전용이고 native filesystem 적용 성공을 증명하지 않는다고 문서화했다.

### 5. Documentation synchronization

Files:

- `apps/devbox-manager/README.md` — 사용자 흐름, bounds, active locator, no-migration/no-removal,
  safe install destination과 browser mock 경계를 추가했다.
- `docs/architecture.md` — active-root data flow, preview/apply/CAS/rollback, metadata table와
  #309 ownership을 반영했다.
- `docs/product-opportunities.md` §6.10 — #308 제공 흐름, 명시적 비범위, PR/검증 경계를 상세화했다.
- `docs/roadmap.md` — P2 #308/#309 분리 및 dirty implementation draft 상태를 기록했다.
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md` P1-03/P2-11 — empty-root
  pointer 전환과 후속 removal/migration boundary를 명시했다.
- `docs/superpowers/plans/2026-08-26-devbox-manager-custom-install-root.md` — public contract,
  security, exact file mapping, fixture, PR/W2 acceptance를 정리했다.

### 6. Dependencies

`apps/devbox-manager/src-tauri/Cargo.toml`와 `Cargo.lock`에 platform-specific free-space
의존성을 추가했다.

- Unix: `libc = 0.2` (`statvfs`)
- Windows: `windows = 0.61` with `Win32_Foundation`, `Win32_Storage_FileSystem`

| direct target dependency | locked version / license / source | purpose and review |
|---|---|---|
| `libc` | 0.2.189 / MIT OR Apache-2.0 / crates.io | Unix `statvfs` FFI only. It was already present transitively, adds no package or network/runtime executable, and works fully offline. |
| `windows` | 0.61.3 / MIT OR Apache-2.0 / crates.io | Windows `GetDiskFreeSpaceExW` binding with two narrowly selected features. It was already locked transitively and adds no external process or online dependency. |

두 dependency는 기존 lock graph의 package를 target-specific direct edge로 승격했으므로 license/source
정책에 새 예외가 필요하지 않다. `Cargo.lock` digest 변경은 생성기로
`THIRD_PARTY_NOTICES.md`에 반영했고 dependency policy/notices regression gate를 다시 통과했다.

## Code Examples

### Read-only preview and CAS apply

```rust
// apps/devbox-manager/src-tauri/src/core/custom_root.rs
let preview = preview_custom_root(locator, default_root, common_root, input, catalog_revision, None)?;
if preview.registry_revision != expected_registry_revision {
    return Err(CustomRootError::RevisionMismatch);
}
// apply re-runs all checks before creating candidate/apps and publishing the locator.
```

```tsx
// apps/devbox-manager/src/App.tsx
const preview = await previewInstallRoot(installRootInput);
// The apply button is rendered only for `ready` and asks for a separate confirmation.
const result = await applyInstallRoot(installRootInput, preview.registryRevision);
```

### Fixed public errors

```rust
// commands/manager.rs
CustomRootError::ExistingInstall =>
    "기존 설치가 있어 자동 이동하지 않습니다. 별도 migration을 먼저 진행하세요.".to_string(),
CustomRootError::RevisionMismatch =>
    "설치 root 상태가 바뀌었습니다. 최신 preview를 다시 확인하세요.".to_string(),
```

## Verification Results

### Initial focused Rust verification (before post-checkpoint audit)

```text
cargo fmt --all -- --check                         PASS
cargo clippy -p devbox-manager --lib --offline -j1 -- -D warnings PASS
cargo test -p devbox-manager --lib --offline -j1   PASS (64 tests)
cargo test -p launch --lib --offline -j1            PASS (21 tests)
cargo check -p devbox-manager --lib --offline -j1  PASS
```

### Initial focused frontend verification

```text
pnpm --filter devbox-manager exec tsc --noEmit
  PASS
pnpm --filter devbox-manager test -- --run src/App.test.tsx
  PASS — 1 file, 16 tests
```

`git diff --check` PASS. 검증 중 temporary node_modules
symlink는 테스트 직후 제거했으며 worktree에 남기지 않았다.

`cargo check -p devbox-manager --target x86_64-pc-windows-msvc --offline -j1`는 WSL에
MSVC/Windows C toolchain이 없어 `aws-lc-sys` C compilation 단계에서 중단됐다. 이는
Windows W2/실제 packaged acceptance를 대체하지 않으며, source-level Windows 분기는
CI/Windows에서 확인해야 한다.

### Post-checkpoint audit and final focused verification (2026-08-27)

초기 dirty draft는 `493d332` checkpoint로 보존했고, `origin/main`
`48c285275c678ffcff2575f602f7dd08cb5a51b6`에 rebase할 때 roadmap 충돌의 #281·#308
문단을 모두 유지했다. checkpoint 이후의 read-only 감사에서 다음 보강을 적용했다.

- `active_install_location`, preview/apply의 default data-dir 조회가 directory를 만들지
  않도록 분리했다. locator 파일이 없을 때도 기존 locator parent의 symlink/reparse를 검사해
  안전하지 않은 parent를 legacy fallback으로 취급하지 않는다.
- manifest를 strict shape parser만으로 신뢰하지 않고, active root의 canonical identity와
  `apps/<app>/versions/<version>/<app>.exe` exact layout, regular executable, 모든 중간
  component의 link/reparse 상태를 `read_install_manifest`와 `write_registry`에서 검증한다.
- Windows device spelling의 slash 변형, filesystem root, case/separator identity와
  component-boundary path containment를 보강했다. runtime metadata consistency와 launch
  consumer도 동일한 경계 규칙을 사용한다.
- download `.partial`을 `OpenOptions::create_new`로 열어 기존 regular partial 또는 link를
  truncate하지 않고, stream/write/flush 실패 시 해당 invocation이 만든 partial만 제거한다.
  이제 사용하지 않는 production `is_over_limit` helper는 test-only로 격리해 `-D warnings`
  검증을 깨끗하게 유지한다.
- startup runtime metadata는 present corrupt/oversized locator를 default manifest/locator로
  덮어쓰지 않으며 원래 bytes를 유지한다. public root/download 오류는 fixed message만
  반환한다. frontend에는 unmount 뒤 늦은 preview 응답을 무시하는 fixture를 추가했다.
- rollback은 생성한 manifest가 여전히 exact regular `[]`일 때만 제거한다. publish 이후 내용이
  달라졌으면 외부 변경으로 간주해 파일을 보존하고 rollback 실패를 노출한다.
- startup partial cleanup은 catalog-derived exact executable partial만 bounded preflight 뒤
  제거하고, 사용자 sibling/nested partial은 보존한다. custom locator는 startup마다 재검증하며
  선택 catalog revision만 단조 전파하고 locator의 앞선 revision을 downgrade하지 않는다.
- frontend에는 root preflight가 pending인 동안 refresh·환경 진단·batch 선택을 비롯한 다른
  Manager 작업이 비활성화되고, 반대로 pending metadata refresh가 root와 app mutation을
  비활성화하는 fixture를 추가했다.
- Windows CI Clippy가 `cfg(windows)`에서만 컴파일되는 path identity/containment helper의
  `needless_return`을 발견했다. launch consumer와 Manager의 custom-root, managed-install,
  runtime-metadata 구현에서 같은 표현을 모두 정리했으며 Linux focused gate와 Windows CI가
  동일한 `-D warnings` 기준을 사용하도록 유지했다.
- 후속 Windows test gate는 `std::fs::canonicalize`가 반환하는 `\\?\C:\...` extended
  spelling과 production wire path에서 의도적으로 제거한 prefix가 달라지는 문제를 드러냈다.
  locator를 만드는 test fixture와 public path assertion도 production `canonicalize_path`를
  사용하게 바꿔 실제 저장 계약을 검증한다. 또한 Windows canonicalization은 정상적인 8.3
  component를 long-name spelling으로 확장할 수 있으므로 raw/canonical 문자열 동일성을
  reparse 안전성의 근거로 삼지 않는다. 입력 spelling의 모든 기존 component를 먼저
  `symlink_metadata`로 검사해 symlink/reparse를 거부한 뒤 canonical path만 파생하도록
  custom-root manifest와 portable/installer destination 경계를 보강했다. 문자열 동일성
  제거로 상대 경로가 새로 허용되지 않도록 Manager root와 active manifest root에는 별도의
  absolute-literal gate를 유지하고 relative-root 무변경 회귀 fixture를 추가했다.
- 같은 Windows run에서 Manager 자체 테스트 71개는 모두 통과했고, 남은 실패 5개는
  `crates/launch` 테스트 fixture가 raw `std::fs::canonicalize` 결과를 직렬화하거나 그대로
  비교하던 경우로 한정됐다. fixture의 root/executable/manifest 경로도 production
  `canonicalize_path` 계약으로 통일해 Windows extended prefix 표현 차이를 제거했으며 runtime
  검증이나 경로 안전성 기준은 완화하지 않았다.
- 다음 Windows run은 위 5개 fixture를 모두 통과했고 launch legacy fallback assertion 1개만
  남겼다. 원인은 v0.4.x `current.json` 경로가 `std::fs::canonicalize`의 `\\?\` spelling을
  그대로 반환하는 반면 versioned locator는 정규화 경로를 반환해 public lookup 결과가 설치
  방식에 따라 달라지는 production 일관성 문제였다. `crates/launch`의 legacy와 locator 경로가
  crate-level `canonicalize_path` 하나를 공유하게 통합해 current/latest fallback도 같은 normalized
  contract를 사용한다. 단순 expectation 완화가 아니며 containment와 regular-file 검증은 유지한다.

최종 검증은 다른 worktree의 stale Cargo metadata와 섞이지 않는 전용
`CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-manager-custom-root`를 사용하고 Rust job을
`-j4`로 제한해 수행했다. frontend는 Linux-native temporary mirror에서 전체 workspace를
검증한 뒤 정확한 temporary directory를 제거했다.

```text
cargo fmt --manifest-path apps/devbox-manager/src-tauri/Cargo.toml -- --check  PASS
cargo fmt --manifest-path crates/launch/Cargo.toml -- --check               PASS
cargo test -p devbox-manager -p launch -j4                                               PASS (75 + 23)
cargo check -p devbox-manager -p launch -j4                                              PASS
cargo clippy -p devbox-manager -p launch --all-targets -j4 -- -D warnings                PASS
pnpm --dir apps/devbox-manager test                                                       PASS (19)
pnpm --dir apps/devbox-manager build                                                      PASS
cargo test --workspace -j4                                                               PASS
cargo check --workspace -j4                                                              PASS
cargo clippy --workspace --all-targets -j4 -- -D warnings                               PASS
cargo fmt --all -- --check                                                               PASS
pnpm build                                                                                PASS (17 workspace projects)
python3 .github/scripts/check-dependencies.py check                                      PASS
python3 .github/scripts/test-check-dependencies.py                                       PASS
python3 .github/scripts/test-build-manifest.py                                           PASS
git diff --check                                                                          PASS
```

Windows test gate 경로 보정 후 focused 재검증도 같은 전용 target과 `-j3` 제한으로 수행했다.

```text
cargo fmt --manifest-path apps/devbox-manager/src-tauri/Cargo.toml -- --check  PASS
cargo test -p devbox-manager -p launch -j3                                  PASS (76 + 23)
cargo clippy -p devbox-manager -p launch --all-targets -j3 -- -D warnings    PASS
git diff --check                                                              PASS
```

마지막 launch fixture 보정 뒤에도 Manager 76개와 launch 23개 focused test, 두 package의
all-target Clippy 및 diff gate가 모두 통과했다.

legacy fallback canonicalization 통합 뒤 focused gate도 다시 통과했다.

```text
cargo test -p launch -p devbox-manager -j3                              PASS (23 + 76)
cargo check -p launch -p devbox-manager -j3                             PASS
cargo clippy -p launch -p devbox-manager --all-targets -j3 -- -D warnings PASS
git diff --check                                                         PASS
```

Windows target/package build, Tauri IPC, ACL/junction/reparse, free-space API와 packaged restart
smoke는 CI/W2 경계에서 수행한다.

### Deliberately skipped

- `pnpm install`, full frontend test suite
- Tauri GUI launch, Windows ACL/junction/reparse and packaged installer tests

## Next Steps / Remaining Risks

- PR 직전 focused launch test/check 및 `git diff --check`를 다시 수행했고, 모두 통과했다.
- Windows W2에서 `GetDiskFreeSpaceExW`, read-only ACL, junction/reparse, non-ASCII path,
  packaged restart와 `crates/launch` custom locator discovery를 확인한다.
- startup 이후 두 Manager 프로세스가 같은 locator/install을 동시에 쓰는 경우를 위한 persistent
  journal/lock은 아직 없다. preview/apply의 revision revalidation은 일반 stale UI와 정상적인
  revision 경합을 감지하지만, 마지막 atomic replace 직전의 외부 TOCTOU를 완전히 직렬화하지는
  않는다. partial cleanup와 bounded read도 metadata 확인 후 파일이 교체되는 race가 남아 있어
  Windows W2와 향후 OS-level handle/openat 계층에서 보강한다.
- 기존 regular `.partial`은 안전하게 덮어쓰지 않고, 재시작의 bounded exact-slot cleanup 전까지
  같은 프로세스 재시도를 거부한다. Manager가 만들 수 없는 이름이나 위치의 사용자 partial은
  cleanup 대상이 아니다. 이는 다운로드 데이터 보존을 우선한 정책이며 UI에 고정 실패 메시지를 반환한다.
- atomic replace는 filesystem-level single-file atomicity와 scoped rollback을 보장하지만,
  process 강제 종료와 두 Manager 프로세스의 install/write 동시성 전체를 해결하는 persistent
  journal/lock은 이 issue 범위가 아니다. revision CAS는 root apply 경합을 차단한다.
- 기존 custom root migration, safe binary/user-data removal, root reset/rollback은 #309 설계
  이후 별도 PR로 수행한다. #308에서 자동 이동·삭제로 확장하지 않는다.
