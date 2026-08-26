# Webhook Lab Rule Descriptions (#282)

## Overview

Webhook Lab의 response rule editor가 method/path/status/delay/body와 response headers의 의미를
값이 채워진 상태에서도 계속 보여 주도록 보강했다. backend의 실제 매칭 계약과 Rust IPC/storage
검증 경계를 문서·테스트 fixture로 고정하고, numeric/string/collection bounds, field accessibility,
invalid draft/stale/busy guard, backend 오류 비노출을 같은 PR 범위에서 정리했다. PR 전 감사에서
최신 `main`(#414/#416/#419/#421/#422/#424 포함)에 rebase한 뒤 발견한 TypeScript import, surrogate 경계,
stale stop/curl refresh, 테스트 환경 문제도 이 workthrough의 후속 보강으로 기록한다.

## Context and Constraints

- method 비교는 대소문자를 무시하며, 빈 editor 값은 `None`으로 보내 모든 method에 매치한다.
- path는 전체 문자열 exact match가 기본이고, rule path의 마지막 문자가 `*`일 때만 그 앞부분
  prefix match를 한다. 중간 `*`는 wildcard가 아니다.
- response rule의 `status`, `headers`, `body`는 반환할 HTTP response이며 `delay`는 응답 전
  대기 시간이다.
- 여러 rule이 겹칠 때 `HashMap` iteration order는 priority/determinism 계약이 아니다.
- frontend와 backend는 같은 경계를 적용한다: rule 200개, id 128자/128바이트, method ASCII
  token 16자/16바이트, path `/` 시작·control 금지·4,096자/16,384바이트, headers 100개·이름
  256자/256바이트·값 16,384자/65,536바이트·합계 64,000자/256,000바이트, body 256,000자/
  1,024,000바이트, collection 문자열 합계 2,000,000자/8,000,000바이트, status 100~599,
  delay 0~60000ms.
- char는 JS `Array.from`과 Rust Unicode scalar count, byte는 `TextEncoder`와 Rust UTF-8
  `str::len()`으로 계산한다. 빈 신규 id는 저장 전 UUID가 되며 collection 계산에서도 36자/36
  바이트를 예약한다.
- 예시 curl(#283), fixture/replay engine 등 다른 기능은 구현하지 않았다.

## Changes Made

### 1. Rule editor validation and accessibility

Files:

- `apps/webhook-lab/src/App.tsx`
- `apps/webhook-lab/src/App.css`
- `apps/webhook-lab/src/lib/ruleValidation.ts`
- `apps/webhook-lab/src/lib/ruleValidation.test.ts`

각 field에 visible label/help를 유지하고 body와 저장된 headers에도 response 의미·크기 계약을
추가했다. status는 `100~599`, delay는 `0~60000ms` 정수, path는 `/` 시작·control 금지,
method는 HTTP token, body·headers·rule collection은 동일한 char/UTF-8 byte bounds를 저장 전에
검증한다. `min`/`max`/`step`, `aria-invalid`, `aria-describedby`, `aria-busy`, field error
스타일과 ref 기반 double-action guard를 함께 사용한다. invalid raw draft는 입력창에 유지하고
duplicate도 같은 validator와 projected collection을 통과해야 저장된다. 편집 대상 id가 refresh에서
사라진 stale rule은 고정 오류로 차단한다.

`safeMessage`는 고정된 안전 오류만 허용하고 그 외 backend 원문은 일반 오류 문구로 대체한다.
전역 오류는 `role="alert"`로 노출하며, 경로·토큰·header 값·secret이 섞인 원문이 DOM에
들어가지 않는다. frontend 검증을 우회해도 Rust `upsert`가 검증 실패를 opaque error로 만들고
`set_rule`이 `규칙 입력이 유효하지 않습니다`로 변환하며, map mutation은 성공 시에만 수행한다.
규칙 삭제 버튼에도 대상별 accessible label을 추가했다.

### 2. Backend semantics fixtures and comments

Files:

- `apps/webhook-lab/src-tauri/src/core/rules.rs`
- `apps/webhook-lab/src-tauri/src/commands.rs`

`ResponseRule`의 status/headers/body가 response 구성이라는 field 문서를 추가하고, Rust
fixture로 None method/all-method, case-insensitive method, exact path, query 차이,
trailing-star prefix, 중간-star literal을 고정했다. numeric/status·delay와 method/path/header/
body/count/string-byte bounds, invalid upsert no-mutation도 고정했다. request handler에는 HashMap
순회 순서가 priority나 determinism을 보장하지 않는다는 주석을 추가했으며 매칭·id·저장 순서
semantics 자체는 변경하지 않았다.

### 3. Documentation synchronization

Files:

- `apps/webhook-lab/README.md`
- `docs/superpowers/specs/2026-08-14-webhook-lab-design.md`
- `docs/superpowers/specs/2026-08-15-ux-improvements-design.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

README와 UX·native-first 계획 문서에 실제 path/method semantics, response field 경계,
no-priority rule, 모든 저장 bounds와 char/byte 단위, backend authority/no-mutation,
accessibility/error redaction, test coverage와 explicit non-scope를 상세히 기록했다.

### 4. Rebase and PR-preflight corrections

최신 `origin/main`으로 rebase하면서 #414 example curl과 #416 token-scanning 변경을 보존하고,
UX/native-first 문서의 upstream 행·세부 계약을 충돌 없이 합쳤다. rebase 이후 집중 검증에서
다음 결함을 확인·수정했다.

- `apps/webhook-lab/src/App.tsx`: editor가 참조하는 `MAX_METHOD_CHARS` import 누락을 보완했다.
  focused Vitest에서 runtime `ReferenceError`로 재현됐고, 이후 app-level typecheck/build로 고정했다.
- `apps/webhook-lab/src/lib/ruleValidation.ts`: 문자열 끝의 lone high surrogate에서
  `charCodeAt()`이 `NaN`이 되는 경우를 명시적으로 거부했다. 이를 통해 Rust UTF-8 경계와
  JavaScript draft 검증의 fail-closed 계약을 맞췄다.
- `apps/webhook-lab/src/App.tsx`: stop 완료 후 authoritative refresh를 수행해 기존 mount refresh가
  중지된 서버 상태를 다시 덮어쓰지 않게 했다. example curl의 fresh read에도 generation guard를
  적용해 늦은 refresh가 fresh status/rule을 덮어쓰지 않게 했다.
- `apps/webhook-lab/src/App.test.tsx`: 앱별 Vitest setup이 없는 현재 webhook-lab 구성에서도
  동작하도록 새 assertion을 native `textContent` 기반으로 바꾸고, busy 중 context menu가 열리지
  않는 실제 계약을 테스트하도록 수정했다.

## Key Contract Examples

```text
method = None + PATCH + /hook       -> match
method = "post" + PoSt + /hook      -> match
rule path = /events/* + /events/123 -> match
rule path = /events/* + /eventslater -> no match
rule path = /events/*/tail + /events/123/tail -> no match
```

```tsx
<input
  aria-describedby={`rule-status-help${statusIssue ? " rule-status-error" : ""}`}
  aria-invalid={statusIssue ? "true" : undefined}
  min={MIN_RESPONSE_STATUS}
  max={MAX_RESPONSE_STATUS}
  step={1}
/>
```

## Verification

- `git diff --check`: passed.
- `cargo fmt --check`: passed.
- Focused Rust command passed with 17 tests:
  `source ~/.cargo/env && CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-webhook-rule cargo test -p webhook-lab --lib`.
- The focused validator/editor run passed 30 tests, and the complete Webhook Lab frontend suite
  passed all 43 tests across 4 files.
- `pnpm --filter webhook-lab build` passed (`42 modules transformed`). The frontend commands used
  the existing cached dependency snapshot in the Linux-native mirror; no package installation or
  lockfile change was made.
- App-level Rust Clippy passed with all targets and `-D warnings`; the collection-only fixture helper
  is test-gated so it does not leave dead production code.
- On the #422 PR-preflight tree, the repository-wide frontend gate passed all 727 tests across the
  packages/apps with test scripts, followed by `pnpm build` for all 17 participating workspace
  projects. The load-sensitive Code Pad test that failed during the first concurrent run passed all
  113 tests both in an isolated rerun and in the clean full-suite rerun. After rebasing onto #424,
  the affected Webhook Lab suite was rerun with all 43 tests and its production build passing.
- On that #422 full-gate tree, `cargo test --workspace -j2`, `cargo check --workspace -j2`,
  `cargo clippy --workspace --all-targets -j2 -- -D warnings`, and
  `cargo fmt --all -- --check` all passed. The #424 rebase was followed by the affected Webhook Lab
  Rust 17-test suite, app-level Clippy/fmt, dependency policy, catalog, and `git diff --check`.
- Dependency notice/policy checks, their regression fixtures, build-manifest notice fixtures,
  catalog consistency, and `git diff --check` passed. No dependency, lockfile, or generated source
  change was introduced by this feature.
- Windows W1 packaged smoke remains a release-checkpoint item; it is not representable in the WSL
  pre-PR environment.

## Remaining Risks

- Focused and full workspace Rust/frontend gates pass in the available Linux toolchain. Windows
  packaged smoke remains pending at the W1 checkpoint. The implementation does not rely on
  frontend-only bounds.
- The backend still stores rules in a `HashMap`; this PR documents the absence of precedence rather
  than changing matching behavior. Overlapping rules should not be relied upon until a separate
  precedence design is approved.
- A future persistence/import path must call the same `validate_rule_collection` boundary before
  loading data. This PR has no persistence/import path, so no second storage format was invented.
