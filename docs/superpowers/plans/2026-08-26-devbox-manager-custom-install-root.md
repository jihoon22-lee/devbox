# Devbox Manager custom install-root 계획 (#308)

## 목적과 범위

Devbox Manager가 이미 소유한 catalog, release manifest, portable layout과 versioned
install-root locator를 한 화면에서 연결해, 개발자가 매번 별도 경로를 찾아 설치하거나
환경을 다시 설정하지 않고 다음 휴대용 앱 설치 위치를 선택할 수 있게 한다. 이 기능은
대형 외부 도구를 내재하거나 다운로드하는 기능이 아니며, 로컬·오프라인 환경에서도
동작하는 native filesystem/metadata 기능이다.

이번 PR의 root 변경은 기존 설치를 옮기는 migration이 아니다. 사용자가 미리 만든
canonical 빈 디렉터리를 검증하고, 확인 후 그 디렉터리에 빈 Manager manifest를 준비한
뒤 locator의 pointer를 바꾸는 것만 허용한다. 기존 설치, binary, partial, user data는
그대로 둔다. 기존 설치 migration, root reset, binary 제거와 user-data 삭제는 후속
GitHub issue #309의 별도 소유 범위다.

## 선행 계약과 저장 위치

- 공용 catalog: `%LOCALAPPDATA%\devbox\catalog.json`
- versioned locator: `%LOCALAPPDATA%\devbox\install-roots\v1\registry.json`
- active app-owned manifest: `<locator.path>\registry.json`
- portable layout: `<active-root>\apps\<app-id>\versions\<version>\<app-id>.exe`
- locator schema: `schemaVersion = 1`, 양의 `registryRevision`, 양의
  `catalogRevision`, bounded `rootId`, canonical `path`, canonical `manifestPath`, 양의
  `updatedAtMs`

locator는 설치 목록을 복제하지 않고 active root와 그 root가 소유한 manifest의 위치 및
catalog provenance만 보관한다. manifest는 기존 Manager의 snake_case wire shape를 유지한다.

```json
{
  "schemaVersion": 1,
  "registryRevision": 5,
  "catalogRevision": 6,
  "rootId": "custom-<sha256-prefix>",
  "path": "C:\\Devbox-custom",
  "manifestPath": "C:\\Devbox-custom\\registry.json",
  "updatedAtMs": 1780000000000
}
```

portable record는 `{ app, version, mode: "portable", exe_path }`, installer record는
`{ app, version, mode: "installer", exe_path: "" }`만 허용한다. unknown field, duplicate
app ID, invalid app/version/mode, oversized bytes/rows는 fail-closed한다.

## 사용자 흐름과 상태 계약

### 1. 입력

frontend는 `설치 root 경로` 입력과 `미리 확인` 버튼을 제공한다. 입력은 4,096 bytes로
제한하고 `autocomplete`/spellcheck를 끄며, Enter는 IME composition 중 실행하지 않는다.
입력 변경은 현재 preview와 오류를 폐기한다. preview/apply 중복 동작은 ref와 disabled
상태 양쪽으로 막고, component unmount 뒤 늦은 native 응답은 상태를 바꾸지 않는다.

### 2. Preview

`preview_install_root({ path })`는 파일·locator·registry를 변경하지 않는 read-only
command다. backend가 다음을 순서대로 수행한다.

1. path를 trim하고 empty/NUL/4,096-byte 초과, `%`·`$`·`~`, `\\?\`·`\\.\`, `.`/`..`,
   non-absolute 입력을 거부한다.
2. 모든 existing component의 symlink와 Windows reparse point를 `symlink_metadata`로
   확인하고, canonical path가 입력 path와 다르면 거부한다. filesystem root,
   `USERPROFILE`/`HOME`, `DEVBOX_WORKSPACE`/`WORKSPACE`, current working directory와
   common metadata root는 후보로 허용하지 않는다.
3. 후보는 이미 존재하는 directory여야 한다. 부모나 후보를 자동 생성하지 않는다.
   direct entry는 최대 4,096개까지 읽으며 symlink/reparse entry는 안전하지 않은 상태로
   처리한다.
4. active locator가 없을 때만 legacy default Manager root를 read-only fallback으로
   사용한다. locator 파일이 있으면 16 KiB 이하 strict schema만 읽고, malformed,
   symlink, unknown field, invalid revision/path는 legacy fallback 없이 오류로 끝낸다.
5. active manifest는 최대 1 MiB·256 rows로 읽는다. record 또는 root의 `apps`,
   `.partial`, 기타 artifact가 하나라도 있으면 `existing-install`로 반환한다. active
   install count와 candidate entry count는 bounded UI 사실로만 제공한다.
6. candidate filesystem의 write permission과 OS free space를 확인한다. 쓰기 권한이 없으면
   `permission-denied`, free space를 확인할 수 없으면 `free-space-unavailable`, 128 MiB보다
   작으면 `insufficient-free-space`로 반환한다.

preview DTO는 raw error나 OS path를 오류에 넣지 않고 다음 값만 반환한다.

```text
status: ready | already-active | existing-install | candidate-conflict | permission-denied
        | insufficient-free-space | free-space-unavailable
canApply: status가 ready/already-active일 때만 true
registryRevision, catalogRevision
candidatePath: backend가 canonicalize한 검증 결과
rootId: candidate canonical path에서 결정론적으로 만든 opaque ID
freeSpaceBytes?, requiredFreeSpaceBytes (= 128 MiB)
activeInstallCount, candidateEntryCount
migration: blocked-existing-install | no-automatic-migration
```

`existing-install`, conflict, free-space 실패 상태에는 적용 버튼을 표시하지 않는다.
경로 자체는 사용자가 선택한 대상의 검증 결과이므로 preview panel에서만 보여 주고,
오류·로그·credential과 섞지 않는다.

### 3. Confirm/apply

사용자가 별도의 확인 대화상자를 승인한 경우에만
`apply_install_root({ path, expectedRegistryRevision })`를 호출한다. backend는 preview
결과를 신뢰하지 않고 다음을 다시 수행한다.

- locator revision과 active canonical root가 expected CAS 값과 일치하는지 확인
- active manifest/artifact가 여전히 비어 있는지 확인
- candidate canonical identity, empty 상태, symlink/reparse, protected path, free space 재확인
- `registryRevision + 1` overflow 확인

모든 검사가 통과한 경우에만 후보 안에 `apps/`를 component별로 생성·확인하고, 빈
`registry.json`은 `create_new`로 독점 생성한 뒤 flush/sync한다. `create_dir_all`로 경로를
따라가지 않으며, preflight 뒤 다른 writer가 만든 기존 파일도 대체하지 않는다. 완성된
비활성 후보가 exact empty `apps/`와 two-byte `registry.json`만 포함하는지 다시 검사한 뒤,
이를 가리키는 locator를 unique temporary sibling에 serialize·reparse하고 atomic
replace한다. locator에는 새 root ID, canonical path/manifest, 현재 catalog revision과
updated time을 기록한다.

locator commit 실패 또는 stale race가 발생하면 이번 invocation이 만든 빈 manifest와
empty `apps/`만 제거한다. cleanup 대상이 symlink/non-empty로 변했거나 cleanup 자체가
실패하면 `rollback-failed`를 반환하며 성공으로 숨기지 않는다. 특히 manifest는 여전히 exact
regular two-byte `[]`일 때만 제거하며, publish 이후 다른 내용으로 바뀌었다면 외부 변경을
보존한다. 기존 default/custom root의
registry, binary, partial, app-local user data에는 삭제·이동·덮어쓰기를 하지 않는다.

### 4. 적용 후 lifecycle

Manager의 installed/current/rollback/install-path/launch/open-folder/install 명령은 매번
locator를 읽어 active root를 결정하고, non-legacy locator의 catalog provenance가 현재 선택
catalog revision과 일치하며 manifest의 모든 app ID가 현재 Manager 대상인지 확인한다. startup
sync 실패 뒤 stale locator나 제거된 catalog app이 남아 있으면 lifecycle을 fail-closed한다.
custom root가 active인 동안 기본 root의 portable
remove command는 실행하지 않고 후속 #309 경계를 안내한다. 설치 시 apps/app/version 및
installer cache directory는 component별 symlink/reparse 확인 후 만들며, destination과
`.partial` sibling이 regular file slot인지 즉시 확인한다. locator/manifest가 손상되면
명령은 고정 메시지로 중단하고 다른 root를 추측하지 않는다.

## 보안·신뢰 경계

- frontend는 app ID, mode, path 문자열과 CAS revision만 보낸다. locator path, manifest
  path, executable 후보는 입력으로 받지 않는다.
- backend는 catalog-visible/non-self-managed app과 strict version을 다시 검증한다.
- canonicalization 전후 path가 다르거나 symlink/reparse component가 있으면 거부한다.
- root/home/workspace/env expansion, UNC/device alias, traversal, non-UTF8 display는
  fail-closed한다. arbitrary executable을 실행하거나 registry의 raw exe path를 곧바로
  신뢰하지 않는다.
- locator 16 KiB, manifest 1 MiB/256 rows, candidate/direct scan 4,096 entries,
  path 4,096 bytes의 상한을 유지한다.
- locator가 없는 legacy fallback도 locator 경로의 이미 존재하는 부모 component가
  symlink/reparse이면 사용하지 않는다. active manifest의 portable record는 raw executable의
  모든 component와 canonical exact layout을 다시 확인하고, `<root>/apps/<app>/versions/<version>/<app>.exe`
  밖이나 링크를 통해 root 안으로 보이는 경로를 trusted state로 취급하지 않는다.
- public DTO 및 UI 오류에는 raw input path, local path, URL, OS error, credential,
  manifest/locator 원문을 반향하지 않는다. 성공 preview의 canonical candidate display는
  사용자가 선택한 검증 대상 확인에 한정한다.
- preview는 read-only다. apply도 빈 후보의 metadata 준비와 locator pointer 전환만
  수행하며 기존 사용자 파일을 변경하지 않는다.

## 정확한 파일 매핑

### Rust/native

- `apps/devbox-manager/src-tauri/src/core/custom_root.rs`
  - bounded locator/manifest parser, safe path and protected-root checks
  - missing-vs-corrupt locator resolution
  - preview status 계산, free-space API, deterministic root ID
  - revision CAS, exclusive manifest creation/atomic locator publish, scoped rollback
- `apps/devbox-manager/src-tauri/src/core/managed_install.rs`
  - active root 아래 portable/installer destination component preparation
  - destination 및 `.partial` symlink/reparse guard
- `apps/devbox-manager/src-tauri/src/core/mod.rs`
  - custom root core export
- `apps/devbox-manager/src-tauri/src/commands/manager.rs`
  - `InstallRootRequest`, `InstallRootApplyRequest`, safe preview/apply DTO
  - `preview_install_root`/`apply_install_root` Tauri commands
  - registry/lifecycle를 active locator root으로 연결
  - lifecycle의 locator catalog provenance와 manifest catalog membership 검증
  - custom root removal을 #309까지 fail-closed
- `apps/devbox-manager/src-tauri/src/lib.rs`
  - invoke handler 등록
- `apps/devbox-manager/src-tauri/Cargo.toml`, `Cargo.lock`
  - Unix `statvfs`, Windows `GetDiskFreeSpaceExW`에 필요한 target dependency
- `crates/launch/src/installed.rs`
  - locator/manifest consumer의 bytes/path/row bound와 bounded read 유지

### Frontend

- `apps/devbox-manager/src/api.ts`
  - typed preview/apply invoke boundary와 browser-only mock
- `apps/devbox-manager/src/types.ts`
  - preview/apply status DTO
- `apps/devbox-manager/src/App.tsx`
  - explicit preview → confirm → apply flow, stale/unmount/IME/busy guards,
    accessible status/error/facts
- `apps/devbox-manager/src/App.css`
  - responsive root panel, focus/error/status styling, long canonical path wrapping
- `apps/devbox-manager/src/App.test.tsx`
  - explicit confirmation, preview invalidation, existing-install no-apply coverage

## 테스트·검증 계획

### Rust fixture

- 빈 후보 preview가 어떠한 파일도 만들지 않음
- candidate existing file/symlink/reparse, protected/root/home/workspace와 missing dir 거부
- missing locator만 default fallback; corrupt/oversized/present unsafe locator는 fallback 금지
- strict unknown-field, invalid record, duplicate, manifest bytes/rows bounds
- active record와 empty manifest + root artifact 각각 migration block
- permission/free-space unavailable/low/ready status 및 fixed minimum
- CAS stale, active-root race, revision overflow fail-closed
- exclusive manifest creation과 atomic locator publish, locator write 실패 시 생성 residue scoped rollback
- owned artifact 생성 뒤 candidate direct entries/apps emptiness/manifest bytes 최종 재검증
- custom root ID/revision/catalog provenance/deterministic DTO
- install destination component 및 `.partial` symlink guard
- missing locator의 symlink/reparse parent는 legacy fallback을 차단하고, 기존 regular
  `.partial`은 `create_new`가 덮어쓰지 않는지 확인
- active manifest의 exact executable layout, intermediate component link, canonical root identity
  재검증과 startup corrupt-locator non-overwrite를 확인

### Frontend fixture

- 입력 후 preview 전 apply 없음, confirm 거부 시 mutation 없음
- preview success에서 exact path/revision으로 apply 호출
- input change 및 IME composition이 preview를 폐기하고 Enter 중복 호출을 만들지 않음
- preview/apply busy 중 duplicate action 차단
- existing-install/conflict/low-space 상태에 migration/remove action 없음
- unmount 후 늦은 preview/apply response가 state update하지 않음
- label, `aria-describedby`, status live region, alert, keyboard focus와 긴 경로 CSS

### 실행 범위

WSL에서 `cargo fmt --all -- --check`, `cargo test -p devbox-manager --lib --offline -j1`,
`cargo check -p devbox-manager --lib --offline -j1`, `pnpm --filter devbox-manager exec
tsc --noEmit`, 해당 frontend unit test와 `git diff --check`를 수행한다. dependency install,
전체 workspace build/test, commit/push/PR은 이 초안에서 수행하지 않는다. PR 이후 CI가
전체 gate를 수행하고, Windows W2에서 실제 Tauri IPC, ACL/권한, NTFS junction/reparse,
disk-free API, packaged restart, custom root discovery와 locator consumer를 확인한다.

### 구현 감사 보강 및 집중 검증 결과 (2026-08-27)

초기 dirty draft는 `493d332` checkpoint로 보존한 뒤 `origin/main`
`48c285275c678ffcff2575f602f7dd08cb5a51b6` 위로 rebase했고, 충돌한 roadmap의 기존
문단을 양쪽 모두 보존했다. 이후 다음 보강을 같은 #308 기능 경계 안에서 적용했다.

- preview와 active-root 해석은 legacy data directory를 만들지 않는 read-only path 조회를
  사용한다. locator가 없는 경우에도 이미 존재하는 locator 부모의 symlink/reparse를 검사하며,
  corrupt/oversized/present unsafe locator는 default root로 우회하지 않는다.
- active manifest는 shape parser만 통과하는 것으로 충분하지 않다. portable executable이
  canonical active root와 동일하고 고정 layout에 있는 regular file인지, 중간 component가
  link/reparse가 아닌지 `read`와 registry publish 양쪽에서 확인한다.
- Windows device spelling(`\\?\`, `\\.\`, slash variant), filesystem root와 case/separator
  identity, path component boundary를 명시적으로 처리한다. runtime metadata consistency도
  같은 boundary-aware identity를 사용한다.
- installer/portable download의 `.partial`은 `OpenOptions::create_new`로 열어 기존 regular
  file 또는 link를 절대 truncate하지 않으며, stream/write/flush 실패 시 자신의 partial만
  제거한다. public command error는 fixed message만 반환한다.
- startup runtime metadata는 present corrupt locator를 읽은 뒤 default locator/manifest를
  덮어쓰지 않는다. 실패 시 원래 locator bytes를 보존하고 다음 실행에서 재시도한다.
- startup partial cleanup은 catalog의 Manager 대상과 strict version에서 계산한 exact
  `<app>.exe.partial` slot만 256 apps·app당 256 versions bound 안에서 수집하고, 전체 scan이
  성공한 뒤 삭제한다. 사용자 sibling/nested `*.partial`은 보존하며 link/reparse·특수 파일·
  unreadable/oversized tree가 있으면 어떤 target도 삭제하지 않는다.
- valid custom locator는 startup에서 active root/manifest를 다시 검증한 뒤 선택된 최신 catalog
  revision만 단조 전파한다. root/path/manifest는 유지하고 registry revision을 증가시키며,
  locator catalog revision이 선택 revision보다 앞서거나 root가 unsafe하면 원본 locator를
  downgrade·재작성하지 않고 fail-closed한다.
- root preview/apply single-flight 동안 tab, refresh, doctor, app action과 batch selection/action을
  함께 잠근다. metadata refresh/doctor도 read single-flight를 소유해 진행 중에는 root·app
  mutation을 막고, mutation 내부의 후속 refresh만 명시적으로 허용한다.
- locator publish rollback은 manifest가 exact empty `[]`인 경우에만 삭제한다. 다른 writer가
  내용을 바꾼 경우 파일을 보존하고 rollback 실패를 명시적으로 반환한다.

최종 집중 검증 결과:

```text
cargo fmt --manifest-path apps/devbox-manager/src-tauri/Cargo.toml -- --check  PASS
cargo fmt --manifest-path crates/launch/Cargo.toml -- --check               PASS
cargo test -p devbox-manager -p launch -j4                                               PASS — 75 + 23 tests
cargo check -p devbox-manager -p launch -j4                                              PASS
cargo clippy -p devbox-manager -p launch --all-targets -j4 -- -D warnings                PASS
pnpm --dir apps/devbox-manager test                                                       PASS — 1 file, 19 tests
pnpm --dir apps/devbox-manager build                                                      PASS
cargo test --workspace -j4                                                               PASS
cargo check --workspace -j4                                                              PASS
cargo clippy --workspace --all-targets -j4 -- -D warnings                               PASS
cargo fmt --all -- --check                                                               PASS
pnpm build                                                                                PASS — 17 workspace projects
git diff --check                                                                          PASS
```

Frontend fixture에는 explicit preview/confirm, input/IME stale 무효화, duplicate preview,
unmount 뒤 늦은 response 무시, root/read operation의 양방향 잠금과 existing-install
migration/remove 부재를 포함했다. Rust 검증은 다른 worktree와 artifact가 섞이지 않는 전용
Cargo target directory를 사용했고 frontend 검증용 temporary native mirror는 직후 제거했다.
전체 Rust/frontend workspace gate도 통과했다. Windows target/package build, 실제 Tauri
IPC/ACL/junction/reparse/free-space smoke는 CI/W2 경계에 남긴다.

## PR 경계와 후속 순서

논리 검토 단위는 core contract → Manager commands/active lifecycle → frontend UX/a11y →
docs/fixtures이지만 merge 단위는 기능 하나인 #308 PR 하나로 유지한다. #308이 main에
반영되고 CI/W2가 통과한 다음, #309가 existing-root migration이 아닌 명시적 safe removal,
binary/user-data 분리, custom-root reset/rollback 정책을 별도 설계·구현한다. #308에는
삭제·자동 이동·병합을 추가하지 않는다.

### PR 전 잔여 위험과 W2 확인 항목

- atomic replace는 파일 단위 원자성과 이번 호출이 만든 빈 manifest/apps에 한정한 rollback을
  제공하지만, 두 Manager 프로세스의 마지막 locator replace까지 persistent journal/lock으로
  직렬화하지는 않는다. preview/apply의 revision revalidation은 정상적인 stale 경합을
  감지하되 외부 writer의 최종 replace 직전 TOCTOU를 완전히 제거하지 않는다.
- bounded read와 partial cleanup은 metadata 확인 후 파일이 교체되는 OS race가 남아 있다.
  W2에서 Windows handle/reparse/junction 경합을 확인하고, 필요하면 native handle 기반
  open/rename 계층을 별도 설계한다.
- 기존 regular `.partial`은 덮어쓰지 않고 다음 Manager 시작의 active-root cleanup 전까지
  같은 프로세스 재시도를 거부한다. 기존 설치 migration, custom root removal, root reset,
  binary/user-data 삭제는 #309로 명시적으로 분리한다.
