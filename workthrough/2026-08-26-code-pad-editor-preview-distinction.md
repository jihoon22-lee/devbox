# Code Pad Editor and Preview Distinction Workthrough

- Date: 2026-08-26
- Issue: #279 `feat(code-pad): editor·preview 구분`
- Branch: `feat/code-pad/editor-preview-distinction`
- Base: `fc649045070a1f648226c157300455dca6901668`
- Target: Code Pad 0.4.0 / v0.5.0 P1-09-16
- Status: implementation, direct review and local PR-wide gates complete; GitHub Actions pending

## Outcome

Markdown/Mermaid preview를 열면 editor와 preview가 배경, 경계와 header tone으로 즉시 구분된다.
현재 editor focus는 얇은 active border로 나타나고 선택된 document tab은 기존 active underline에
keyboard `focus-visible` ring을 더한다. 색만으로 pane을 추측하지 않아도 `편집 뷰` section과
`프리뷰` complementary landmark가 기존 semantic boundary를 유지한다.

Overflow 정책도 pane별로 고정했다. Preview body 하나만 vertical scroll을 소유하며 prose, 긴 path와
inline code는 panel 안에서 줄바꿈한다. Whitespace가 의미 있는 fenced code는 줄바꿈하지 않고 해당
block 안에서만 horizontal scroll한다. Image, SVG, video와 canvas는 panel width를 넘지 않는다.

이번 PR은 renderer 또는 editor 기능 PR이 아니다. 기존 `previewOpen` boolean, native markdown result,
Mermaid strict renderer, CodeMirror lifecycle, file path와 source 전달을 그대로 사용하고 CSS와 그
구조 계약을 검증하는 fixture만 추가한다.

## Scope

### Included

- preview-open layout의 editor/preview surface 배경 차이
- editor와 preview 사이 하나의 명확한 2px boundary
- preview label/path header의 별도 tone과 bounded path 표시
- active CodeMirror host의 `focus-within` border
- document tab/button의 keyboard `focus-visible` ring
- preview prose/inline code/path wrapping
- code block의 bounded horizontal scroll
- image/SVG/video/canvas width bound
- preview body single vertical scroll contract
- preview toggle, landmark, active tab과 renderer invocation regression fixture
- Code Pad README, roadmap, native-first plan과 이 workthrough

### Excluded

- Markdown sanitizer, renderer HTML 또는 Mermaid source/rendering 변경
- Mermaid security level, SVG application 또는 last-good state 변경
- CodeMirror extension, editor command, document state 또는 focus routing 변경
- preview resize handle, layout persistence, split ratio 또는 new pane state
- new native command, IPC DTO, capability, storage, filesystem, network 또는 process
- source/path masking 정책 변경 또는 raw value reveal 추가
- LSP management UX (#278)와 Workbench services/ports (#280)

## Existing Boundary

Code Pad는 이미 다음 구조와 동작을 제공했다.

- `App`이 `previewOpen`을 소유하고 previewable workspace document에만 toggle을 표시
- `content-area` 안에 editor section과 조건부 `PreviewPane` 배치
- `PreviewPane`은 `aside[aria-label="프리뷰"]`, label/path header와 하나의 body로 구성
- native markdown crate가 sanitize한 HTML만 preview body에 할당
- Mermaid는 `securityLevel: strict`이며 실패한 block은 last-good SVG를 보존
- document tab은 `aria-selected`, roving tab index와 editor panel relation을 제공
- CodeMirror instance는 tab/view 이동 중에도 stable parent 아래 유지

시각적으로는 editor, view와 preview가 유사한 dark surface를 사용했고 1px 저대비 border만 있어 pane
경계가 흐렸다. Editor focus는 CodeMirror caret 외에 pane-level active state가 없었고 tab keyboard
focus ring도 명시하지 않았다. Preview body는 vertical scroll을 소유했지만 긴 pre/media의 panel 폭
정책은 고정되지 않았다.

## Design Decisions

### 1. Keep the change visual-only

Preview가 열릴 때 App은 이미 `.content-area.with-preview`를 부여한다. 새 styling은 이 selector 아래에
scope해 preview가 닫힌 일반 editor layout을 바꾸지 않는다. React state, effect dependency와 native
invoke는 수정하지 않는다.

Regression fixture는 preview toggle 전후 다음 사실만 확인한다.

```text
preview toggle aria-pressed false -> true -> false
content-area gains and removes with-preview
editor region remains mounted
preview complementary landmark appears and disappears
active document tab remains selected
existing renderPreview(path, source, workspace) call is unchanged
```

### 2. Use one divider

Editor의 right border와 preview의 left border를 동시에 두면 3px 이상 겹치며 split view 내부 divider와
혼동된다. Preview 쪽 2px left border 하나만 editor/preview boundary로 사용하고 inset shadow는 surface
depth만 보조한다. Split editor의 기존 1px internal divider는 그대로 둔다.

### 3. Distinguish surface, header and active editor

Preview-open layout은 세 단계 contrast를 사용한다.

```text
layout backdrop       #0c121a
editor/view surface   #101820 / #182432
preview surface       #111a25 with #1b2938 header
boundary              #4c78a5
active editor border  #4d83b9
```

Active editor border는 `.code-editor`에 항상 transparent 1px 공간을 예약하고 `:focus-within`에서만 색과
inset shadow를 바꾼다. Focus 이동 때 content size가 흔들리지 않는다. Preview는 read-only render
surface라 새 tab stop을 만들지 않으며 active를 가장하지 않는다.

### 4. Preserve keyboard semantics

Tab selection은 기존 `aria-selected`와 active underline을 그대로 사용한다. `.document-tab-select`와
`.tab-action`에 `:focus-visible` outline을 추가해 keyboard focus만 명시한다. Roving tab index, arrow,
Delete, Menu/Shift+F10과 focus restoration behavior는 기존 `TabBar` fixture가 계속 검증한다.

### 5. Give preview one vertical scroll owner

`.preview-pane`은 overflow를 숨기고 `.preview-body`만 `overflow: auto`를 유지한다. Header, error/empty
surface와 outer content area에는 vertical scroll owner를 추가하지 않는다. Long prose와 inline code는
`overflow-wrap: anywhere`로 panel 안에 남는다.

Whitespace가 의미 있는 `pre`는 `white-space: pre`를 유지하며 `overflow-x: auto`를 적용한다. 이를
`pre-wrap`과 함께 사용하면 horizontal scroll이 사실상 사라지고 code indentation/line fidelity가
달라지므로 두 정책을 섞지 않았다. Nested vertical scroll은 만들지 않는다.

### 6. Bound rendered media without touching renderer output

Sanitized markdown 또는 Mermaid가 만든 `img`, `svg`, `video`, `canvas`에 `max-width: 100%`와
`height: auto`만 적용한다. DOM을 다시 쓰거나 source/attribute를 변환하지 않는다. 따라서 sanitizer와
Mermaid strict security boundary는 그대로 유지된다.

### 7. Do not widen the privacy boundary

Preview header의 `docPath`와 renderer input은 기존 값이다. 이 PR은 새 path/source field, logging,
clipboard, persistence 또는 error echo를 추가하지 않는다. Path label은 ellipsis로 layout만 제한하며
title/tooltip처럼 숨겨진 full-value reveal도 추가하지 않는다. 외부 process, URL 또는 network action도
없다.

## File Changes

### Frontend

- `apps/code-pad/src/App.css`
  - scoped editor/preview surface and boundary
  - active editor and keyboard focus styles
  - preview overflow/media/error/empty states
- `apps/code-pad/src/App.test.tsx`
  - preview toggle, semantic region, selected tab and unchanged renderer call fixture

### Documentation

- `apps/code-pad/README.md`
  - visual/overflow contract and explicit behavior non-change
- `docs/roadmap.md`
  - #279 implementation record and next feature boundary
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
  - detailed implementation and W1 acceptance
- this workthrough

## Failure and Security Fixtures

The focused App fixture covers:

- preview is absent and toggle is not pressed initially
- preview open adds the scoped styling hook and accessible complementary landmark
- editor region remains present while preview is open
- active document remains selected
- exact existing path/source/workspace renderer arguments
- preview close removes the landmark and styling hook without closing the document

Existing Code Pad fixtures continue to cover:

- tab roving focus, arrow navigation and `aria-selected`
- Delete and keyboard context-menu actions with focus restoration
- stable CodeMirror parents across tab/view changes
- preview last-good Mermaid SVG state
- sanitized native Markdown output boundary

No raw error, credential, secret or path is introduced by the fixture or new UI behavior. The test path is a fixed
non-secret fixture value.

## Validation

Completed from an exact Linux-native mirror with at most two frontend workers:

```text
pnpm --dir apps/code-pad test -- --maxWorkers=2
  14 files, 113 tests passed
pnpm --dir apps/code-pad build
  TypeScript and Vite production build passed
cargo fmt --all -- --check
  passed
cargo test --workspace
  all workspace unit/integration/doc tests passed
cargo check --workspace
  passed
cargo clippy --workspace --all-targets -- -D warnings
  passed
pnpm --workspace-concurrency=2 -r test
  all 17 frontend/package projects passed
pnpm --workspace-concurrency=2 -r build
  all 17 frontend/package projects passed
catalog consistency, dependency-policy and regression scripts
  passed
pnpm audit --audit-level moderate
  no known vulnerabilities
cargo deny --locked check
  advisories, bans, licenses and sources passed; allowlisted duplicate warnings only
```

Running the same frontend suite directly from the `/mnt/e` worktree caused an existing LSP panel async test to
exceed its timeout while workers waited on 9p I/O. The run was stopped without using its result. The exact mirror
run completed the whole Code Pad suite in 11.68 seconds with all 113 tests passing, which confirms the failure was
the mount latency rather than a product regression.

WSL cannot execute the packaged Windows Tauri runtime, so the visual/keyboard packaged evidence remains in the
release W1 checkpoint rather than being represented as locally complete.

## Manual W1 Checkpoint

On a Windows packaged Code Pad build, verify:

- preview closed editor appearance remains unchanged
- preview open editor and preview backgrounds are visibly distinct
- exactly one strong divider separates editor and preview, including split editor mode
- clicking/keyboard-focusing each editor shows its active border without layout shift
- active tab underline and keyboard focus ring remain distinguishable
- long preview path does not widen the header or expose an added tooltip
- long prose and inline code wrap inside the preview
- fenced code preserves whitespace and scrolls horizontally inside its block
- large image, Mermaid SVG, video and canvas stay within preview width
- only preview body scrolls vertically; header and outer layout remain fixed
- preview error and loading states remain readable at narrow width
- opening/closing preview does not remount or discard the editor document
- no new process, network request, storage write, renderer capability or console window appears

## Follow-up

#280 `feat(workbench): services·ports 입력` is the next independent P1-09 feature. It owns service CRUD,
stable edit buffering and validated persistence; none of that scope is included here.
