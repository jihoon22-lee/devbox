# Developer Toolbox Smart Workflows fresh-base 정리 (#340–#343)

## Overview

최초 `origin/main@14716d0`에서 선별한 Developer Toolbox smart workflows 후보를
API Playground #346–#348이 병합된 `main@656ba4b`로 rebase하고, 검토를 마친 #343
handoff 후보를 같은 전용 worktree에 통합했다. Launcher·Log Lens·window-state와
무관한 stale 변경은 가져오지 않았다.

통합 범위는 #340 detection, #341 typed pipeline, #342 metadata persistence와 #343
`Toolbox→API text handoff`다. 네 이슈는 같은 결과 surface를 공유하지만, local pipeline과
cross-app one-time handoff는 별도의 explicit action·저장·수명 경계를 유지한다.

## Context and boundary decisions

- detection은 실행기가 아닌 bounded classifier다. 최대 1,000,000 UTF-8 bytes와
  2,100,000 code units 안에서 JSON/JWT/HTTP(S) URL/Base64/Base64URL/Hex 후보만
  판별한다.
- URL은 로컬 parse만 하며 열거나 fetch하지 않는다. JWT는 허용된 HS compact
  decode 후보일 뿐 서명 검증이 아니다. credential assignment, bearer/basic/token
  prefix, URL userinfo·민감 query, local/file path와 제어 문자는 fail-closed한다.
- pipeline은 static descriptor registry의 input/output type을 각 단계에서 확인하고,
  최대 8단계·1 MiB input·4 MiB intermediate/final output을 적용한다. shell,
  arbitrary process, network, API receiver는 registry에 없다. 실행은 버튼을 누른
  경우에만 일어난다.
- metadata는 tool/transformer ID, pipeline input type/ID, timestamp만 보존한다.
  Native는 app-local `smart-workflows.json`을 atomic replace하고 browser preview는
  같은 allow-listed shape만 versioned localStorage에 기록한다. 입력·출력·clipboard·
  credential·path는 저장하지 않는다.
- malformed/oversized native metadata는 원본을 자동 교체하지 않고 `writable=false`
  로 보고한다. 파일 read와 native save IPC 모두 serialized metadata 상한을 먼저
  적용하며 오류는 고정 메시지만 반환한다.
- native metadata wire schema는 unknown field를 거부하고, final symlink/reparse와
  읽기 전후 identity 교체를 차단하며 process-local I/O lock으로 직접 IPC 동시 저장도
  한 번에 하나씩 수행한다. pipeline 20개가 모두 찼을 때 다른 entry를 암묵적으로
  덮어쓰지 않는다.
- 공용 filesystem identity helper는 Windows `CreateFileW`의 Win32 오류 코드를
  `std::io::Error`로 보존한다. 존재하지 않는 metadata 파일이 `NotFound`가 아닌
  불명확한 `Other`로 축약되어 최초 저장을 막지 않으며, 다른 native 오류는 계속
  fail-closed한다.
- Smart Workflow UI는 기존 `ToolTextArea`/`ToolOutput`의 명시적 paste/copy/save
  동작을 사용한다. labelled sections, live status/error, keyboard focus-visible
  outline과 explicit run/save/favorite/open actions를 유지한다.

## Changes made

### #340 Smart detection

- `apps/developer-toolbox/src/workflows/smartDetection.ts`
  - bounded JSON shape/node scan, strict JWT parser reuse, local URL parser,
    canonical Base64/Base64URL/Hex validation을 추가했다.
  - binary byte는 text로 추정하지 않고 lossless byte transformer를 추천한다.
  - candidate DTO/reason/error에는 입력 원문·secret·path·parser exception을 넣지
    않는다.
- `apps/developer-toolbox/src/workflows/smartDetection.test.ts`
  - JSON/JWT/URL/ambiguous Base64/binary Hex, invalid/path/credential/oversized,
    credential-shaped JSON과 JSON node bound fixture를 고정했다.

### #341 Typed transformer pipeline

- `apps/developer-toolbox/src/workflows/transformPipeline.ts`
  - JSON/YAML/TypeScript, unverified JWT decode, URL component,
    Base64/Base64URL/Hex와 case 변환을 static descriptor로 등록했다.
  - unknown/type mismatch를 실행 전에 차단하고 stage failure/overflow도 fixed
    non-reflective error로 반환한다.
- `apps/developer-toolbox/src/workflows/transformPipeline.test.ts`
  - compatible JSON chain, Base64 text decode, binary lossless conversion,
    mismatch/unknown/step/input bound와 type-only output 계산을 검증한다.

### #342 Metadata and UI

- `apps/developer-toolbox/src/workflows/workflowStore.ts`
  - schema v1 allow-list, deterministic ordering, recent/favorite/pipeline/step/
    serialized-size bounds, malformed preservation과 serial save queue를 구현했다.
  - native IPC에는 이미 64 KiB로 직렬화한 metadata string만 전달한다.
- `apps/developer-toolbox/src-tauri/src/core/workflows.rs`
  - native wire model, current tool/transformer allow-list, type transition,
    timestamp/size validation, bounded file read와 atomic save를 구현했다.
- `apps/developer-toolbox/src-tauri/src/commands/workflows.rs`,
  `commands/mod.rs`, `core/mod.rs`, `lib.rs`
  - app-local path는 command 경계에서만 결정하고 metadata load/save 명령을
    등록했다. save command는 serialized payload size를 확인한 뒤 metadata를
    deserialize한다.
- `apps/developer-toolbox/src/workflows/SmartWorkflowPanel.tsx`,
  `apps/developer-toolbox/src/App.tsx`, `apps/developer-toolbox/src/App.css`
  - detection, typed pipeline, explicit output actions, recent/favorite/pipeline
    library를 하나의 labelled offline surface로 연결했다.
  - restart 시 metadata만 복원하고 draft text는 복원하지 않는다.
- `apps/developer-toolbox/src/workflows/workflowStore.test.ts`,
  `SmartWorkflowPanel.test.tsx`
  - redaction/order/restart, malformed preservation, typed pipeline persistence,
    ambiguous no-auto-selection과 explicit UI actions를 검증한다.

### Documentation

- `apps/developer-toolbox/README.md`
  - #340–#343의 범위, bounds, metadata schema, fixture와 local/cross-app 경계를
    앱 문서에 기록했다.
- 이 workthrough
  - fresh-base 선별 기준, 통합 rebase, 보안·자원 경계와 검증 상태를 기록한다.

## Verification

- 최신 `main@656ba4b` 통합 후 Developer Toolbox 전체 frontend는 29 files/232
  tests, 마지막 TypeScript compile 보정 뒤 handoff focused file은 6 tests가 통과했다.
- combined focused Rust는 API Playground 100, AppLink 60, Catalog 11, Developer
  Toolbox 51 tests가 통과했다. 이어 `cargo test --workspace -j1` 전체 suite와
  doc-tests가 모두 통과했다.
- `cargo check --workspace -j1`, strict Clippy(`developer-toolbox`, `api-playground`,
  `applink`, `catalog`, all targets, `-D warnings`)와 `cargo fmt --all -- --check`가
  통과했다.
- `pnpm --workspace-concurrency=2 -r build`는 19개 frontend project를 모두
  성공적으로 빌드했다. `git diff --check`도 통과했다.
- 첫 PR CI의 전체 frontend 병렬 실행에서 Knowledge Quick Capture clipboard
  fixture가 mock 호출만 기다린 뒤 React state 반영 전 값을 단언하는 race로 한 번
  실패했다. 제품 코드는 이 PR에서 바뀌지 않았고, fixture를 동일 `waitFor` 안에서
  호출 횟수와 textarea 값까지 기다리도록 보정했다. focused Quick Capture 11 tests가
  통과했으며 갱신 CI에서 전체 suite를 다시 확인한다.
- 다음 CI에서 Windows 최초 metadata save 3개 fixture가 모두 같은 고정 오류로
  실패했다. 원인은 공용 `filesystem_identity`의 Windows 오류 변환이 Win32
  `ERROR_FILE_NOT_FOUND`를 `ErrorKind::Other`로 잃어버려, 정상적인 신규 파일 slot을
  unsafe storage처럼 거부한 것이었다. Win32 코드를 raw OS error로 보존하고 공용
  identity test에 missing-path `NotFound` 회귀 검증을 추가했다. Linux 공용 identity,
  Developer Toolbox는 각각 17/51 tests 및 strict Clippy가 통과했고, 공용 filesystem의
  `x86_64-pc-windows-msvc` check도 통과했다. Tauri app 전체의 local Windows check는
  host에 `llvm-rc`가 없어 resource build 단계에서 중단되므로 Windows CI로 최종
  실행 검증한다.

## Remaining risks and follow-up

1. Native Windows packaged W3에서 candidate 선택, typed mismatch, explicit run,
   metadata-only restart, handoff preview/edit/cancel/apply와 no-auto-send를 확인해야 한다.
2. Linux workspace compile에서 Tauri command registration과 serialized IPC
   argument naming을 확인했다. 최종 Windows packaged smoke는 계속 필요하다.
3. 기존 tool/transformer catalog가 변경되면 TypeScript registry, Rust allow-list와
   metadata migration을 함께 갱신해야 한다.
