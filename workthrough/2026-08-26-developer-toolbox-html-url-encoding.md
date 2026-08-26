# Developer Toolbox HTML Entity·URL Component Encoding

## Overview

Issue #287의 Encoding 확장을 구현했다. 기존 URL 변환은 bounded URL component codec으로
강화하고, HTML parser나 network 없이 동작하는 HTML entity encode/decode 도구를 추가했다. 모든
변환은 현재 WebView 안에서만 실행되며 자동 저장·history·외부 전송은 없다.

## Context

기존 URL 도구는 `encodeURIComponent`와 `decodeURIComponent`를 직접 호출해 malformed percent와
invalid Unicode의 runtime 오류를 UI 경계에서 안전하게 정규화하지 않았고, input/output 상한도
없었다. #287은 HTML entity와 URL component text tool을 요구하지만 HTML parser와 network fetch는
명시적으로 제외한다. 따라서 parser가 암묵적으로 복구하는 동작 대신 deterministic text codec과
고정 오류를 선택했다.

## Changes Made

### 1. Bounded native-first text codec

File: `apps/developer-toolbox/src/tools/textEncoding.ts`

- URL component encode/decode를 `encodeURIComponent`/`decodeURIComponent` semantics로 제공한다.
- percent escape의 두 hexadecimal digit과 percent-decoded UTF-8을 먼저 검증하고, invalid UTF-8,
  lone surrogate, malformed percent는 `malformed_url` 또는 `invalid_unicode` 고정 오류로 거부한다.
- HTML encode는 `&`, `<`, `>`, `"`, `'`를 canonical named/numeric entity로 바꾼다.
- HTML decode는 `entities@8.0.0`의 전체 표준 HTML named entity와 직접 검증한 decimal/hex numeric
  code point를 semicolon-terminated strict mode로만 해석한다.
  unknown·unterminated·surrogate·범위 밖 numeric entity는 부분 결과 없이 실패하며, entity가 될 수
  없는 literal ampersand는 보존한다. DOMParser, sanitizer, tag/attribute 해석은 사용하지 않는다.
- 공통 1,000,000 UTF-8 input bytes, 4,000,000 output bytes, 16배 expansion 상한과 HTML
  token/entity-count 상한을 적용한다. encode는 사전 크기 추정, decode는 누적 크기 검사를 사용한다.
- 오류에는 input text, credential, path, platform/parser diagnostic을 넣지 않는다. 알 수 없는 예외도
  `Text transformation failed.`로 정규화한다.

핵심 계약:

```ts
export const TEXT_ENCODING_LIMITS = {
  maxInputBytes: 1_000_000,
  maxOutputBytes: 4_000_000,
  maxExpansionRatio: 16,
  maxEntityCount: 100_000,
  maxEntityTokenLength: 32,
  maxNumericEntityDigits: 7,
};
```

### 2. Transformer UI integration

Files: `apps/developer-toolbox/src/tools/transformers.tsx`,
`apps/developer-toolbox/src/tools/index.tsx`, `apps/developer-toolbox/src/tools/common.tsx`

- `HTML Entity Encode`, `HTML Entity Decode`, `URL Component Encode`, `URL Component Decode`를
  Encoding group에 등록했다. 기존 `url-encode`/`url-decode` IDs는 유지했다.
- codec 결과를 `TransformerTool`에 연결하는 고정 runner를 사용해 render마다 effect가 새로
  생성되지 않게 했다.
- bounded text tool은 새 입력이 시작될 때 이전 output/error를 지우고, 기존 sequence guard로
  stale Promise 결과를 폐기한다. effect cleanup은 unmount 뒤 state 갱신도 차단한다. input의
  `aria-busy`와 live running status를 표시한다.
- 공용 `ToolTextArea`/`ToolOutput`를 재사용해 explicit paste/select/clear, native keyboard와 IME,
  명시적 copy/select/save 및 accessible Input/Output label을 보존했다.

### 3. Fixtures and documentation

Files: `apps/developer-toolbox/src/tools/textEncoding.test.ts`,
`apps/developer-toolbox/src/tools/TextEncodingTool.test.tsx`,
`apps/developer-toolbox/src/tools/transformers.test.ts`,
`apps/developer-toolbox/README.md`, `docs/roadmap.md`,
`docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

- standard named/multi-code-point entity와 numeric Unicode round-trip, canonical encoding, literal ampersand, malformed entity/percent,
  invalid UTF-8/surrogate, prototype-looking name, input/output/entity bounds와 fixed error를
  순수 fixture로 고정했다.
- frontend fixture는 accessible surface, clear-on-input, busy/live state, stale completion discard,
  malformed input의 non-reflective error를 검증한다. 기존 URL 회귀 fixture도 새 오류 계약에 맞춰
  갱신했다.
- 앱 README와 v0.5.0 roadmap/native-first plan에 parser/network/dependency 비범위, bounds,
  privacy, UI 및 W2 packaged smoke 계획을 상세히 기록했다.
- 이미 workspace lock에 있던 BSD-2-Clause `entities@8.0.0`을 직접 dependency로 승격했으며 새
  package 다운로드는 필요하지 않다. lock digest가 달라지므로 notice는 generator로 갱신한다.

## Verification Results

### TypeScript

```text
node_modules/typescript/bin/tsc --project apps/developer-toolbox/tsconfig.json --noEmit
Exit code: 0
```

The check used the repository's cached dependency snapshot; no install was performed.

### Focused, app, and repository-wide tests

```text
Vitest v4.1.10
Developer Toolbox: 14 test files passed, 125 tests passed
All 17 frontend projects: tests passed
All 17 frontend projects: production builds passed

cargo test --workspace -j4
cargo check --workspace -j4
cargo clippy --workspace --all-targets -j4 -- -D warnings
cargo fmt --all -- --check
Exit code: 0
```

The focused codec and UI fixtures include the explicit unmount-while-pending case. The full Developer
Toolbox suite passed all 125 tests, then every frontend project completed its test and production-build
gate. The complete Rust workspace test/check, Clippy, and formatting gates also passed.

Dependency policy and notice generation/regression fixtures, build-manifest fixtures, catalog
consistency, CI scope detection, and `git diff --check` passed. The dependency review confirmed that the
direct `entities@8.0.0` declaration reuses the already pinned BSD-2-Clause package and does not add a
second implementation or network/runtime dependency.

## Known Limitations / Next Steps

- This is a text codec, not an HTML parser or sanitizer; it does not interpret document structure or
  assemble full URLs.
- The named HTML decoder uses the complete standard table from the pinned codec plus validated numeric
  entities; unsupported names fail closed rather than being silently repaired.
- Windows packaged W2 smoke remains a post-CI acceptance check because the Linux host cannot execute the
  Tauri package. The feature itself is WebView-only and has no target-specific native branch.
