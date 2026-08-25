# Devbox Manager App Row Context Menu

## Overview

Issue #255의 P1-06-DM 범위로 Devbox Manager의 앱 목록 행에 `@devbox/context-menu`를 적용했다.
mouse right-click, Shift+F10, Menu key가 같은 메뉴와 action 경로를 사용하고, 선택되지 않은 행에서
메뉴를 열면 그 행을 먼저 선택하며 닫힌 뒤 원래 행으로 focus를 복구한다.

정확한 메뉴는 다음과 같다.

```text
설치/업데이트 ▸
  휴대용
  설치 패키지
실행
이전 버전 롤백
────────
설치 폴더 열기
────────
제거 (danger)
```

메뉴를 붙이는 과정에서 실행·폴더 열기·제거가 frontend로 전달된 raw executable path에 의존하지
않도록 Manager의 portable 설치 경계를 재정의했다. frontend에는 catalog app ID와 mode/version 상태만
전달하며, backend가 catalog, registry, 고정 layout, canonical filesystem identity를 action마다 다시
검증한다. 제거는 Manager 기본 root 아래 해당 app tree만 대상으로 하고 별도 앱 사용자 데이터는
보존한다.

batch, install path 표시, Related Tools, custom install root, installer uninstall은 이 PR 범위가 아니다.
특히 installer의 실제 설치 위치나 uninstaller는 Manager가 현재 소유한 증거가 없으므로 추측하지 않고
관련 메뉴를 비활성화한다. custom root와 안전한 제거 확장은 P2-11의 locator/app-owned manifest 계약이
소유한다.

## Context

- 앱 목록은 설치·업데이트·Launch·Rollback inline button만 제공했고, pointer와 keyboard에서 사용할
  수 있는 행 단위 메뉴가 없었다.
- 우클릭한 행과 이전 selection이 다를 수 있어, 메뉴가 기존 selection을 사용하면 다른 앱을 실행하거나
  제거할 수 있었다.
- 설치/업데이트는 portable과 setup이라는 서로 다른 기존 경로가 있어 단일 menu item으로 합치면 선택을
  잃는다. 공용 package의 submenu로 두 방식을 그대로 보존해야 했다.
- installer registry entry는 installer 실행 사실만 기록하고 실제 설치 완료 여부, executable 위치,
  uninstaller를 소유하지 않는다. 실행·폴더 열기·제거를 제공하면 임의 경로 추측이나 의도하지 않은
  외부 상태 변경이 된다.
- 기존 `installed`와 `current` IPC DTO는 Manager app-local registry의 raw executable path를 frontend에
  반환했다. context action에는 app ID만 필요하므로 이 경로 노출은 불필요했다.
- portable registry도 local state이므로 신뢰할 수 있는 command 경계가 아니다. path traversal,
  registry/path 불일치, symlink 또는 Windows reparse point가 있으면 action이 fail-closed해야 한다.
- basic remove는 현재 Manager가 명확히 소유하는 기본 root app tree만 지울 수 있다. binary와 앱별
  user data를 한 번에 지우거나 custom root를 추측하는 것은 P2 기능을 중복 구현하게 된다.

## Changes Made

### 1. 공용 context menu와 대상 우선 선택

Files:

- `apps/devbox-manager/package.json`
- `pnpm-lock.yaml`
- `apps/devbox-manager/src/App.tsx`
- `apps/devbox-manager/src/App.css`

기존 workspace package `@devbox/context-menu`를 direct dependency로 추가했다. 새 registry package,
sidecar, network service, Tauri capability는 없다.

각 app row는 focusable하고 `data-app-id`에 catalog ID만 둔다. 공용 hook의 `onBeforeOpen`은 현재 catalog
collection에서 ID를 다시 찾은 뒤 `selectedAppId`와 `contextApp`을 함께 갱신한다. pointer와 keyboard
모두 이 경로를 사용한다. refresh로 대상이 사라지거나 환경 진단 tab으로 이동하면 stale menu와
snapshot을 닫는다.

공용 package가 root/submenu viewport flip, 바깥 클릭·Esc·scroll close, 화살표 navigation, disabled
skip, focus restore를 담당한다. Manager는 항목 정의, 상태 gate, action dispatch만 소유한다.

### 2. 정확한 메뉴 topology와 상태 gate

Files:

- `apps/devbox-manager/src/App.tsx`
- `apps/devbox-manager/src/App.test.tsx`

`설치/업데이트`는 submenu로 portable과 installer 선택을 유지한다. release manifest에 해당 앱이 없거나,
동일 앱 action이 진행 중이거나, 설치 버전과 최신 버전이 같으면 비활성화한다.

상태 표:

| 현재 상태 | 설치/업데이트 | 실행 | 롤백 | 폴더 열기 | 제거 |
|---|---:|---:|---:|---:|---:|
| 미설치 + manifest 있음 | 가능 | 불가 | 불가 | 불가 | 불가 |
| portable, 이전 버전 있음 | 필요 시 가능 | 가능 | 가능 | 가능 | 확인 후 가능 |
| portable, 이전 버전 없음 | 필요 시 가능 | 가능 | 불가 | 가능 | 확인 후 가능 |
| installer | 필요 시 가능 | 불가 | 불가 | 불가 | 불가 |
| 상태 확인 중/손상/manifest 없음 | 불가 또는 확인된 항목만 | 불가 | 불가 | 불가 | 불가 |
| 같은 앱 action 진행 중 | 불가 | 불가 | 불가 | 불가 | 불가 |

installer에는 context와 기존 inline Launch를 모두 노출하지 않는다. portable/installer 설치 button도 같은
app busy gate를 공유해 한 앱에 두 다운로드를 동시에 시작하지 않는다.

제거는 danger style이며 exact display name과 “Manager가 관리하는 실행 파일과 보존 버전만 삭제하고 앱
사용자 데이터는 유지한다”는 범위를 확인 문구에 명시한다. cancel이면 IPC를 호출하지 않는다.

### 3. Path-free frontend IPC contract

Files:

- `apps/devbox-manager/src-tauri/src/commands/manager.rs`
- `apps/devbox-manager/src/types.ts`
- `apps/devbox-manager/src/api.ts`

내부 `InstalledApp`과 `Current`는 기존 registry/current JSON 호환을 위해 `exe_path`를 유지한다. Tauri
command 반환에는 별도 `InstalledAppView`와 `CurrentView`를 사용한다.

```text
InstalledAppView = app + version + mode
CurrentView      = version + installedAt + previousVersion
```

serialization tests는 sentinel secret path가 JSON에 없고 `exePath`/`exe_path` key도 생기지 않는지
검증한다. launch, folder, remove command는 raw path 대신 `appId`만 받는다.

`installed`는 현재 선택 catalog의 manager-visible/non-self-managed target, portable/installer mode,
안전한 version component가 모두 맞는 registry entry만 반환한다. `current`도 portable registry version과
일치하고 current/previous version이 bounded component일 때만 path-free view를 반환한다.

### 4. Manifest와 download coordinate 검증

Files:

- `apps/devbox-manager/src-tauri/src/core/asset.rs`
- `apps/devbox-manager/src-tauri/src/commands/manager.rs`

공식 release API와 allowlist URL 정책은 그대로 유지한다. 그 뒤 release manifest의 다음 값을 filesystem
path나 URL에 넣기 전에 single bounded ASCII component로 검증한다.

- release tag: 최대 128 bytes
- app ID: 최대 64 bytes, manifest 안에서 unique
- app version: 최대 128 bytes
- portable/setup asset name: 최대 255 bytes, `.exe`
- size: 음수 아님
- SHA-256: 정확한 64자리 hex

slash, backslash, colon, 공백, control, `.`/`..`, 허용되지 않은 문자를 거부한다. 오류에는 검증 실패한
manifest 원문을 되돌려 넣지 않는다. manifest app ID는 선택된 catalog에도 존재해야 하며 실제 install
command는 manager-visible/non-self-managed target을 다시 요구한다.

### 5. Manager-owned portable resolver

Files:

- `apps/devbox-manager/src-tauri/src/core/managed_install.rs`
- `apps/devbox-manager/src-tauri/src/core/mod.rs`
- `apps/devbox-manager/src-tauri/src/commands/manager.rs`

registry의 `exe_path`는 증거로만 사용한다. resolver는 검증된 identity로 다음 expected path를 직접 만든다.

```text
<canonical-manager-root>/apps/<app-id>/versions/<version>/<app-id>.exe
```

Manager root, apps root, app root, version directory, executable의 각 path component를
`symlink_metadata`로 검사한다. Unix symlink와 Windows reparse point를 거부하고, canonical apps/app/exe가
각 상위 root 안에 남는지 확인한다. registry executable도 canonicalize한 뒤 derived executable과 정확히
같아야 한다. command는 registry raw path가 아니라 이 resolver가 반환한 derived canonical executable을
사용한다.

적용 command:

- `launch`: 검증된 executable만 새 process로 실행
- `open_install_folder`: 검증된 executable의 exact version directory만 opener로 전달
- `rollback`: previous version target도 고정 layout resolver를 통과한 뒤 current/registry 갱신
- `remove_portable_app`: 검증된 app root만 제거

registry version이 잘못되거나 mode가 installer이거나 catalog target이 아니면 portable command 진입
단계에서 고정 오류로 중단한다.

### 6. Bounded portable removal

Files:

- `apps/devbox-manager/src-tauri/src/core/managed_install.rs`
- `apps/devbox-manager/src-tauri/src/commands/manager.rs`

제거는 resolved app root 전체를 먼저 read-only 순회한다. 최대 depth 16, 최대 entry 10,000이며 일반
file/directory만 허용한다. symlink, Windows reparse point, 특수 entry, root 경계 불일치, 필수 executable
부재가 하나라도 있으면 mutation 전에 중단한다.

검증 뒤 registry에서 해당 app을 atomic write로 먼저 제거해 다른 discovery consumer가 삭제 중인 binary를
새로 실행하지 않게 한다. filesystem 제거가 실패하면 원래 registry를 다시 atomic write하고 runtime
metadata sync를 시도한다. 성공하면 locator가 가리키는 동일 registry를 다시 sync한다.

삭제 대상은 `<manager-root>/apps/<app-id>`뿐이다. 각 앱의 Tauri app-local data, 문서, 프로젝트,
workspace, home, custom root는 대상이 아니다. confirmation과 성공 메시지 모두 user data 보존을
명시한다.

### 7. Documentation and dependency boundary

Files:

- `apps/devbox-manager/README.md`
- `docs/architecture.md`
- `workthrough/2026-08-26-devbox-manager-context-menu.md`

README에는 menu topology, portable-only state, path-free DTO와 removal 범위를 기록했다. architecture에는
Manager row selection/action 흐름, canonical portable identity, bounded deletion과 P2 ownership boundary를
추가했다.

새 의존성은 locked internal workspace package 하나뿐이다. dependency notice generator로 lockfile
provenance를 갱신하며 registry dependency, license surface, CSP, filesystem/network capability에는 변화가
없다.

## Test Coverage

Frontend tests cover:

- catalog의 manager-visible/non-self-managed 12개 target만 표시
- right-click한 exact row가 먼저 선택됨
- exact root menu topology와 danger style
- portable state의 launch/rollback/folder/remove enable gate
- Shift+F10으로 submenu를 열어 portable install 실행
- installer submenu 항목 존재, Menu key setup 실행과 close 후 row focus restore
- launch, rollback, open-folder가 exact catalog app ID를 전달
- removal cancel은 IPC 미호출, confirm은 exact app만 제거하고 user-data 보존 notice 표시
- installer lifecycle/folder/removal fail-closed
- 이미 최신인 target의 install/update 비활성화

Rust tests cover:

- installed/current path-free DTO serialization
- valid release/prerelease/build artifact coordinates
- traversal asset name, duplicate app ID, invalid digest 거부와 raw value 비노출
- exact derived portable executable resolution
- traversal identity와 registry mismatch 거부
- removal이 해당 app tree만 지우고 sibling app 및 user data fixture를 보존
- symlink entry가 있으면 mutation 전에 제거 거부하고 outside fixture 보존
- 기존 manifest, download, layout, URL policy, catalog, runtime metadata 회귀

## Verification Results

아래 결과는 PR 직전 최종 검증에서 갱신한다.

### Rust library tests

```text
$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p devbox-manager --lib -j 1
45 passed; 0 failed
exit 0
```

### Frontend focused tests

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter devbox-manager test -- --maxWorkers=1 src/App.test.tsx
8 passed; 0 failed
exit 0
```

### Frontend build

```text
$ NODE_OPTIONS=--max-old-space-size=768 pnpm --filter devbox-manager build
vite production build passed
exit 0
```

## Key Decisions

1. **설치 방식은 submenu로 보존한다.** “설치/업데이트”라는 설계 항목이 portable/setup 선택을 없애는
   의미는 아니며, 둘은 저장·실행 의미가 다르다.
2. **installer lifecycle은 fail-closed다.** setup process를 실행했다는 registry record만으로 설치
   위치나 uninstall command를 추측할 수 없다.
3. **raw registry path를 frontend로 보내지 않는다.** app ID만 넘기고 action 순간에 backend가 현재
   catalog와 filesystem을 재검증한다.
4. **P1 basic remove는 default portable app tree만 소유한다.** app user data, custom root, installer
   제거는 P2-11의 app-owned manifest와 recovery UX 없이는 안전하게 구현할 수 없다.
5. **검증 실패는 고정 오류로 반환한다.** untrusted registry/manifest path나 원문 값을 UI error에
   포함하지 않는다.
6. **UI에서 한 앱의 중복 lifecycle action을 막는다.** menu와 inline button이 같은 busy gate를 사용해
   진행 중인 앱의 다른 action을 비활성화한다. backend의 canonical 검증은 직접 IPC 호출과 race에 대한
   최종 방어선으로 남는다.

## Follow-up Work

- P1-09: Devbox Manager batch와 read-only install path 표시를 각각 독립 PR로 구현한다.
- P2-11: versioned locator를 기반으로 custom root 사전 검사·atomic 변경·app manifest 기반 safe removal과
  부분 실패 recovery 안내를 구현한다.
- P3-11: Data Inspector, redacted support bundle, Related Tools를 별도 PR로 구현한다.
- Windows W1 checkpoint에서 packaged app의 pointer/Shift+F10/Menu key, WebView2 focus restore, portable
  install/launch/folder/rollback/removal과 user data 보존 evidence를 수집한다.
