# Developer Toolbox Lorem Generator and Markdown Table Formatter (#290/#291)

## Overview

Developer Toolbox Text 그룹에 오프라인 Lorem Generator와 Markdown Table Formatter를 추가했다.
두 issue는 같은 paste → bounded local transform → explicit copy/save 사용자 흐름과 공용 입력·출력
surface를 공유하므로 하나의 cohesive 기능 경계로 구현했다. 기능별 corpus와 parser semantics는
분리하고, 현재 main(c10d205)의 HMAC·HTML/URL 도구와 기존 common API는 유지했다.

## Context

- #290은 외부 text generator 없이 반복 가능한 Lorem 문단·문장·단어와 분량 상한, copy/save를
  요구한다.
- #291은 불균일·빈 pipe table, alignment separator, deterministic padding과 실패 경로를
  요구한다. 전체 Markdown parser/editor와 document export는 비범위다.
- 두 도구는 secret, credential, path, network, process, persistence를 필요로 하지 않으므로
  별도 Rust command·IPC·sidecar·runtime dependency를 추가하지 않고 bundled TypeScript로
  처리했다.
- 입력 변경 또는 새 요청이 이전 결과를 덮어쓰지 않도록 sequence/AbortController/mounted
  guard와 한 번에 하나의 copy/save action만 허용하는 busy 계약을 공용 surface에 반영했다.

## Changes Made

### 1. Deterministic Lorem core

File: `apps/developer-toolbox/src/tools/lorem.ts`

- 5개 classic Lorem 문장을 고정 corpus로 번들하고 `paragraphs`, `sentences`, `words`를
  deterministic하게 생성한다.
- count는 안전한 정수 1–100만 허용하고, UI decimal token은 지수·부호·소수·Infinity를
  거부한다. count paste는 UTF-8 3바이트/3자리 상한을 함께 적용한다.
- 결과는 UTF-8 65,536바이트 이하로 확인하며 invalid unit/count와 overflow는 empty output 및
  고정 code/message를 반환한다. random, clock, locale, network, filesystem, user corpus는
  사용하지 않는다.

### 2. Bounded Markdown table core

File: `apps/developer-toolbox/src/tools/markdownTable.ts`

- pipe-delimited 행을 파싱하고 CRLF/CR을 LF로 정규화한다. leading/trailing blank만 무시하고
  내부 blank 또는 delimiter 없는 행은 고정 `MALFORMED_ROW` 오류로 거부한다.
- 선택적 두 번째 separator 행의 `---`, `:---`, `---:`, `:---:` alignment를 소비한다. 누락
  셀은 빈 값으로 채우고 extra column을 포함하되 source row/column 순서는 변경하지 않는다.
- `\\|`와 matched backtick code span 내부 pipe를 cell data로 해석하고 backslash parity를
  보존한다. unmatched backtick 뒤 delimiter는 숨기지 않으며 tag-like cell은 React text data로
  남는다. HTML parser/renderer, row sorting, external formatter는 호출하지 않는다.
- UTF-8 input 1,000,000바이트, row 1,000개, column 100개, cell 4,096 code point, output
  4,000,000바이트를 fail-closed로 제한한다. control/lone-surrogate·malformed row/separator와
  runtime type misuse는 입력을 오류에 반향하지 않는다. output은 line append 단계에서도
  bounded하게 확인해 padding 폭이 큰 표를 먼저 중단한다.

### 3. Shared interaction boundary

File: `apps/developer-toolbox/src/tools/common.tsx`

- 기존 `useAsyncTransform(input, run, options)` 호출 형태와 기본 오류 동작을 유지하면서
  run의 선택적 `AbortSignal`을 전달하고 effect cleanup 시 이전 request를 abort한다.
- `ToolTextArea`/`ToolTextField`에 선택적 fixed clipboard error, UTF-8 bounded paste, 이전
  value·unmount 뒤의 stale paste 방지 guard를 추가했다. 기존 callers는 새 props 없이 같은
  context menu를 사용한다. 이전 구현의 `actionErrorMessage` input alias도 유지한다.
- `ToolOutput`에 선택적 fixed action error, parent busy state, busy callback을 추가했다.
  output snapshot/value/mounted/revision을 확인하고 duplicate copy/save와 stale error update를
  차단한다. context-menu action도 parent direct action과 하나의 in-flight guard를 공유한다.

### 4. React tools and registration

Files: `apps/developer-toolbox/src/tools/LoremTool.tsx`,
`apps/developer-toolbox/src/tools/MarkdownTableTool.tsx`,
`apps/developer-toolbox/src/tools/index.tsx`, `apps/developer-toolbox/src/App.css`

- Lorem은 문단·문장·단어 선택, 수량 입력, 명시적 생성, 복사, `lorem-ipsum.txt` 저장을
  제공한다. 생성은 synchronous bounded core라 겹치지 않으며 copy/save는 busy ref와 revision으로
  single-flight 처리한다.
- Markdown formatter는 다음 event-loop task로 bounded synchronous core를 예약한다. 새 입력은
  아직 시작하지 않은 task를 `clearTimeout`/AbortController로 취소하고, 시작한 core는 입력 상한
  안에서 완료한 뒤 sequence/unmount guard로 stale 결과를 버린다. 결과가 최신 입력에 속할 때만
  output action을 활성화하고 오류/변환 중에는 output을 빈 값으로 유지한다.
- 두 도구 모두 accessible Korean Input/Output label, `aria-busy`, live running/status,
  fixed `role=alert`를 제공한다. 일반 cut/copy/paste와 IME keyboard event를 가로채지 않는다.
- Text 그룹에 `lorem`과 `markdown-table`을 등록하고 responsive toolbar/output scroll 스타일을
  추가했다. HTML injection 없이 output을 React text로 렌더링한다.

### 5. Unit and integration fixtures

Files: `apps/developer-toolbox/src/tools/lorem.test.ts`,
`apps/developer-toolbox/src/tools/LoremTool.test.tsx`,
`apps/developer-toolbox/src/tools/MarkdownTableTool.test.tsx`,
`apps/developer-toolbox/src/tools/markdownTable.test.ts`,
`apps/developer-toolbox/src/tools/common.test.tsx`

- Lorem exact sentence/word fixture, paragraph boundary/count, repeat equality, max result bound,
  invalid count/unit와 decimal parser rejection을 고정했다.
- Markdown empty/uneven/alignment/escaped pipe/backslash parity/matched·unmatched backtick code span/
  line-ending/text safety, malformed
  row/separator, controls/lone surrogate, input/row/column/cell/output bounds와 deterministic repeat를
  고정했다.
- UI fixture에서 explicit copy/save, fixed direct/context/paste errors, output action gating,
  newest-result guard, IME-neutral key handling, accessible output, bounded 1 MiB paste를
  검증한다.

### 6. Documentation

Files: `apps/developer-toolbox/README.md`, `docs/roadmap.md`,
`docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`, this workthrough

- 앱 도구 목록과 Lorem/table semantics·bounds·privacy를 갱신했다.
- roadmap와 v0.5.0 plan에 #290/#291을 같은 cohesive text-transform boundary로 묶은 이유,
  shared single-flight/stale/cancel/a11y/fixture 계약, 비범위와 W2 follow-up을 기록했다.

## Code Examples

### Fixed formatter failure

```typescript
const result = formatMarkdownTable(input);
// malformed or over-bound input:
// { output: "", error: { code: "MALFORMED_SEPARATOR", message: "..." } }
// The pasted row is never included in the message.
```

### Latest-request guard

```tsx
const current = ++requestSequence.current;
const controller = new AbortController();

const timer = setTimeout(() => {
  if (controller.signal.aborted || requestSequence.current !== current) return;
  const result = formatMarkdownTable(input);
  if (controller.signal.aborted || requestSequence.current !== current) return;
  setState({ ...result, input, running: false });
}, 0);

return () => {
  clearTimeout(timer);
  controller.abort();
};
```

## Verification Results

The root agent runs the repository-wide gate. Focused verification in this worktree uses the existing
offline pnpm store with two workers at most:

```text
pnpm install --offline --frozen-lockfile --child-concurrency=2           passed
pnpm --filter developer-toolbox exec vitest run --maxWorkers=2 \
  src/tools/{common,lorem,LoremTool,markdownTable,MarkdownTableTool}...   37 passed
pnpm --filter developer-toolbox exec vitest run --maxWorkers=1 \
  src/tools/LoremTool.test.tsx                                           6 passed
pnpm --filter developer-toolbox build                                   passed
git diff --check                                                         passed
```

The new unit/integration fixtures are included for the root/CI gate; no dependency, Cargo, IPC, or
runtime storage change was made. Windows W2 packaged offline smoke remains a release checkpoint.

## Next Steps

- Complete the grouped focused test/build and repository Cargo/frontend gates before the PR.
- On Windows, verify offline packaged cold start, exact Lorem copy/save, malformed/oversized table
  behavior, keyboard/IME/focus, narrow output scrolling, and absence of automatic clipboard/storage/
  network side effects.
- #289 JWT와 #292 QR은 사용자 결정에 따라 #290/#291과 같은 Developer Toolbox 0.3.0 offline
  tool release PR에서 통합 검증한다. 각 parser/crypto/renderer acceptance와 workthrough는 분리한다.
