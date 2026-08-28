# Developer Toolbox Smart Workflows fresh-base 정리 (#340–#343)

## Overview

`origin/main@14716d0`를 기준으로 Developer Toolbox smart workflows 작업을
전용 worktree에 선별 통합했다. 기존 후보 worktree의 기능 전용 코드·테스트·앱
README hunk만 가져왔고, Launcher·Log Lens·window-state·다른 앱 변경과 stale
root 계획 문서는 가져오지 않았다.

현재 통합 범위는 Developer Toolbox 안에서 완결되는 #340 detection, #341 typed
pipeline, #342 metadata persistence다. #343 `Toolbox→API text handoff`는
Developer Toolbox 외 integration/API Playground와 one-time secret handoff를
포함하는 별도 보안 경계이므로 이 worktree에는 구현·의존성·다른 앱 변경을 추가하지
않았다.

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
  - #340/#341/#342의 범위, bounds, metadata schema, fixture와 #343 exclusion을
    앱 문서에 기록했다.
- 이 workthrough
  - fresh-base 선별 기준, 보안·자원 경계, 검증 제한과 후속 #343 작업을 기록한다.

## Verification

- `git diff --check` — 통과.
- 13개 앱이나 window-state/Launcher/Log Lens를 이 작업에 포함하지 않았는지 변경
  목록을 정적으로 확인했다.
- 지시대로 `cargo test`, `cargo check`, `pnpm build`, commit, push, PR은 이
  worktree에서 수행하지 않았다. 통합 후 parent가 fresh-base 전체 gate와 Windows
  packaged W3 smoke를 실행해야 한다.

## Remaining risks and follow-up

1. #343은 아직 남아 있다. #284/#341 선행 조건을 확인한 뒤 integration crate의
   `toolbox-text/v1` one-time handoff, API Playground receiver, preview/claim/
   expiry/no-clipboard fixture를 별도 보안 경계로 구현해야 한다.
2. Native Windows packaged W3에서 candidate 선택, typed mismatch, explicit run,
   metadata-only restart와 외부 action 부재를 확인해야 한다.
3. Tauri command registration과 serialized IPC argument naming은 Windows
   `cargo check` 및 packaged smoke에서 확인해야 한다.
4. 기존 tool/transformer catalog가 변경되면 TypeScript registry, Rust allow-list와
   metadata migration을 함께 갱신해야 한다.
