# Devbox Manager 안전한 앱 제거 계획 (#309)

## 목적과 범위

Devbox Manager가 직접 설치한 portable 앱의 Manager 소유 binary tree만 확인 후
제거한다. 사용자가 선택한 app ID는 현재 catalog와 active install-root locator에서 다시
확인하고, active app-owned manifest의 exact executable layout을 증명한 뒤에만 삭제한다.
기본 root와 custom root를 같은 경계로 처리하며, 앱 사용자 데이터와 Manager가 소유하지
않는 파일은 제거 대상에 포함하지 않는다.

이번 기능은 다음 네 가지 상태를 하나의 안전한 흐름으로 묶는다.

1. `preview_remove_app`가 read-only preflight를 수행하고 대상 tree의 사실을 보여 준다.
2. 사용자가 별도의 확인을 승인한다.
3. `remove_portable_app`가 revision/root/manifest digest를 compare-and-swap으로 재검증한
   뒤, 검증된 path 목록만 삭제한다.
4. 삭제 실패나 중단 시 원래 manifest를 보수적으로 복구하고, 남은 항목 수와 재시도
   안내를 반환한다.

Installer 앱의 실제 설치 위치나 uninstaller는 Manager가 소유하지 않으므로 이 command의
제거 대상이 아니다. 강제 삭제, arbitrary path 입력, 기본 사용자 data 삭제, 기존 설치
migration/reset은 범위에 포함하지 않는다.

## 저장 계약과 신뢰 경계

- locator: `%LOCALAPPDATA%\\devbox\\install-roots\\v1\\registry.json`
- active manifest: `<active-root>\\registry.json`
- portable app tree: `<active-root>\\apps\\<app-id>\\`
- executable: `<active-root>\\apps\\<app-id>\\versions\\<version>\\<app-id>.exe`
- Manager 상태: `current.json`, `versions/` 아래의 검증된 version directory와
  `<app-id>.exe.partial`만 app-owned layout으로 간주한다.

Manifest의 `exe_path`는 증거일 뿐 deletion path가 아니다. native code가 catalog-safe
`app_id`, strict version, active canonical root으로 expected path를 계산하고, manifest의
absolute literal path와 canonical identity가 모두 일치하는지 확인한다. 모든 기존 path
component의 symlink/reparse point를 거부하며, Windows junction/reparse와 특수 파일을
따라가지 않는다.

## Native command 계약

### Preview

`preview_remove_app({ appId })`는 다음을 변경 없이 확인한다.

- app ID가 현재 `managerVisible && !selfManaged` catalog entry인지
- locator provenance와 active manifest가 현재 Manager catalog revision에 맞는지
- manifest가 strict schema, bounded bytes/rows, known app ID를 만족하는지
- portable record가 exact `<root>/apps/<app>/versions/<version>/<app>.exe`를 가리키는지
- app root가 Manager가 만든 `current.json`/`versions` layout만 포함하는지
- target tree가 regular file/directory이며 link, reparse, special, foreign entry가 없는지
- tree depth와 entry 수가 bounded limit 안인지

Preview는 `ready`, `partial`, `missing`, `unsupported-installer` 상태와 함께 canonical
app-root, version, Manager-owned entry/byte count, 사용자 data 보존 여부를 반환한다.
`partial`은 이전 시도가 version directory 또는 executable을 일부 지운 상태이며, `missing`
은 app tree가 이미 없지만 manifest record가 남은 상태다. 둘 다 exact manifest record를
정리하는 retryable 상태다.

### Confirm/remove

확인 후 frontend는 preview에서 받은 opaque token을 다음 request로 보낸다.

```json
{
  "appId": "code-pad",
  "expectedRegistryRevision": 7,
  "expectedCatalogRevision": 6,
  "expectedRootId": "custom-…",
  "expectedManifestDigest": "<64 lowercase/uppercase hex chars>"
}
```

native command는 process 안에서 removal mutex를 잡고 locator, manifest bytes/digest,
catalog revision, app-owned tree를 다시 읽는다. token이 stale하거나 path/tree가 바뀌면
파일을 건드리지 않고 고정 오류를 반환한다. 일치하면 manifest에서 해당 record를 먼저
atomic하게 제외한다. 그 뒤 preflight가 수집한 exact file/directory 목록을 깊은 순서로
하나씩 제거한다. recursive `remove_dir_all`이나 사용자가 입력한 path는 사용하지 않는다.

완료 결과는 `removed`와 removed/remaining entry count를 반환한다. 하나라도 권한·잠금·I/O
문제로 남으면 `partial` 결과가 되고, manifest가 여전히 이번 호출이 쓴 digest일 때만
원래 bytes를 복원한다. 복구 경쟁이 감지되면 외부 manifest를 덮어쓰지 않고 재시작 및
남은 파일 확인 안내를 반환한다. 복구 parser는 이미 삭제된 *exact final executable*만
missing으로 허용하며 schema, path, link/reparse 검사는 유지한다.

## Frontend 흐름

앱 행 context menu의 `제거`는 곧바로 mutation하지 않고 preview를 시작한다. Preview panel은
검증된 app-root와 방식/version, Manager-owned count/size, “앱 사용자 데이터 보존”을
표시한다. Installer record는 제거 action 없이 Manager가 uninstaller를 추적하지 않는다는
설명을 표시한다.

`확인 후 제거` 버튼만 native mutation을 호출하며 별도 `window.confirm`을 요구한다.
Preview/apply 중 single-flight guard, stale generation, component unmount guard로 중복
호출과 늦은 응답을 차단한다. confirmed request가 stale이면 오래된 preview를 폐기하고
새 preview를 다시 확인하도록 고정 메시지를 보여 준다. partial 결과는 남은 항목 수와
잠금/권한 해결 후 재시도 안내를 보여 주고, user data 삭제 action은 제공하지 않는다.

## 정확한 파일 매핑

### Rust/native

- `apps/devbox-manager/src-tauri/src/core/removal.rs`
  - exact portable layout preflight, bounded ownership list, link/reparse/special/foreign
    entry rejection, deepest-first non-recursive removal, partial outcome
- `apps/devbox-manager/src-tauri/src/core/custom_root.rs`
  - manifest bytes/digest snapshot과 interrupted-removal용 missing-final-executable 검증
- `apps/devbox-manager/src-tauri/src/core/managed_install.rs`
  - 기존 compatibility wrapper를 safe removal core로 연결
- `apps/devbox-manager/src-tauri/src/commands/manager.rs`
  - preview/remove DTO와 CAS request, manifest claim/restore, installer fail-closed boundary
- `apps/devbox-manager/src-tauri/src/core/mod.rs`, `src-tauri/src/lib.rs`
  - module 및 Tauri command 등록

### Frontend

- `apps/devbox-manager/src/api.ts`, `src/types.ts`
  - preview/request/result typed IPC boundary와 browser mock
- `apps/devbox-manager/src/App.tsx`, `src/App.css`
  - preview → confirm → remove panel, stale/unmount/busy handling, accessible status/error
- `apps/devbox-manager/src/App.test.tsx`
  - preview-only, confirm request, pending guard, stale preview invalidation, installer gate

## 테스트 계획

Rust fixture는 다음을 확인한다.

- 정확한 app tree만 삭제되고 sibling app, root 밖 user data는 유지된다.
- app ID/version traversal, registry mismatch, foreign entry와 symlink를 mutation 전에 거부한다.
- root/apps/app/version executable이 missing인 interrupted tree를 `partial`/`missing`으로
  읽고 재시도할 수 있다.
- 권한 실패는 bounded partial outcome과 remaining count를 반환한다.
- custom-root manifest snapshot은 exact missing final executable만 recovery에서 허용한다.
- entry count/depth bounds와 Windows reparse branch가 fail-closed한다.

Frontend fixture는 preview가 성공하기 전 remove/confirm을 호출하지 않고, stale token 오류
뒤 기존 target/button을 폐기하며, pending preview 동안 Refresh와 다른 mutation이 비활성화되고,
installer removal이 표시만 되는지 확인한다.

실제 Windows W2에서는 NTFS junction/reparse, ACL/lock failure, packaged custom-root
restart, installer record, runtime metadata refresh를 추가 확인한다.

## 검증 명령

전용 target directory와 `CARGO_INCREMENTAL=0`, 최대 두 Rust job을 사용한다.

```text
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-issue309 \
  CARGO_INCREMENTAL=0 cargo test -p devbox-manager --lib --offline -j2
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-issue309 \
  CARGO_INCREMENTAL=0 cargo check -p devbox-manager --lib --offline -j2
cargo fmt --manifest-path apps/devbox-manager/src-tauri/Cargo.toml -- --check
pnpm --dir apps/devbox-manager test -- --run
pnpm --dir apps/devbox-manager exec tsc --noEmit
pnpm --dir apps/devbox-manager build
git diff --check
```

전체 workspace gate와 Windows packaged 실행은 이 기능 worktree의 범위를 넘어가므로 CI/W2에서
수행한다. 이 worktree에서는 source/worktree를 삭제하거나 push/PR/merge하지 않는다.
