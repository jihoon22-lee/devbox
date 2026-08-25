# Developer Toolbox UTF-8, Base64URL and Hex Byte Codec

## Overview

Issue #265의 P1-09 두 번째 범위로 기존 Base64 Encode/Decode 두 화면을 하나의 byte codec으로
통합했다. 입력과 출력 표현을 각각 UTF-8 text, Hex raw bytes, Base64, Base64URL에서 선택하고
모든 조합을 내부 `Uint8Array`로 연결한다.

```text
UTF-8 text ─┐
Hex bytes ──┼─ strict decode ── Uint8Array ── lossless encode ─┬─ UTF-8 text
Base64 ─────┤                    (≤ 1 MB)                       ├─ Hex bytes
Base64URL ──┘                                                   ├─ Base64
                                                               └─ Base64URL
```

invalid encoded character는 원문 문자 위치로, UTF-8로 표현할 수 없는 raw byte는 byte 위치로
표시한다. 대체 문자 삽입이나 non-canonical pad bit 수용처럼 조용한 변형을 하지 않는다.

기능은 browser built-in `TextEncoder`, `TextDecoder`, `btoa`, `atob`만 사용하고 외부 converter,
runtime download, network와 새 dependency가 없다. Windows packaged smoke는 계획의 W1 P1 묶음
checkpoint에 남겼다.

## Context and Constraints

- 기존 화면은 `unescape(encodeURIComponent())`와 `escape(atob())`에 의존해 text와 raw byte
  책임이 섞여 있었고 invalid 위치를 제공하지 않았다.
- Base64와 Base64URL은 62·63번째 alphabet이 다른 별도 표현이다. Base64URL을 단순 Base64
  alias로 보지 않고 입력 alphabet과 출력 padding을 구분해야 한다.
- raw byte를 UTF-8로 열 때 U+FFFD replacement를 허용하면 원래 byte를 잃으므로 fatal 경계와
  최초 오류 위치가 필요하다.
- Base64는 암호화나 secret 보호가 아니다. 입력/결과를 자동 저장·log·전송하지 않고 사용자가
  명시적으로 선택한 clipboard/file action만 수행한다.
- file input, binary file save, UUID, HMAC과 JWT verify는 issue #265 범위가 아니다.
- 앱 version은 Wave 9에서 0.3.0으로 올리므로 이 기능 PR은 현재 0.2.2를 유지한다.

## Standards Decisions

- [RFC 4648](https://www.rfc-editor.org/rfc/rfc4648.html)의 Base64와 URL/file-name-safe alphabet,
  padding과 canonical zero pad bit 규칙을 기준으로 삼았다.
- Base64URL 출력은 `-`/`_` alphabet과 padding 없는 canonical text를 사용한다. 입력은 올바른
  terminal padding이 있거나 없어도 받되 결과는 항상 unpadded로 정규화한다.
- 일반 Base64도 paste 호환성을 위해 올바른 padding 생략을 허용하고 출력은 padded canonical
  form으로 정규화한다.
- 이 도구 자체가 MIME-like paste convenience를 정의해 space, tab, CR, LF만 무시한다. 다른
  Unicode whitespace와 alphabet 밖 문자는 위치 오류다.
- [WHATWG Encoding Standard](https://encoding.spec.whatwg.org/#utf-8-decoder)의 UTF-8 범위와
  fatal error 의미를 적용한다. BOM byte는 raw round-trip에서 사라지지 않도록
  `TextDecoder(..., { fatal: true, ignoreBOM: true })`를 사용한다.

## Changes Made

### 1. Representation-neutral conversion core

Files:

- `apps/developer-toolbox/src/tools/byteCodec.ts`
- `apps/developer-toolbox/src/tools/byteCodec.test.ts`

`convertByteEncoding(input, source, target)`은 `{ output, byteLength, error }`를 반환한다. 오류는
fixed code/message와 optional 1-based position/unit만 포함하고 원문이나 runtime exception을
반향하지 않는다.

지원 경로는 다음과 같다.

| 표현 | decode | encode |
|---|---|---|
| UTF-8 text | unpaired JS surrogate 검사 후 `TextEncoder` | strict byte validator 후 fatal `TextDecoder` |
| Hex | ASCII whitespace 제외, nibble 두 자리 검증 | lowercase 두 자리 canonical text |
| Base64 | `A-Z a-z 0-9 + /`, optional correct padding | standard alphabet, required canonical padding |
| Base64URL | `A-Z a-z 0-9 - _`, optional correct padding | URL-safe alphabet, no padding |

입력 표현은 2,100,000 UTF-16 code unit, decoded raw byte는 1,000,000바이트로 제한한다. Hex와
Base64는 decoded size를 계산한 뒤 byte buffer를 만들기 전에 상한을 거부한다. UTF-8 text는
encode 뒤 같은 byte 상한을 적용한다.

Base64 validator는 다음을 구분한다.

- standard/URL-safe alphabet mismatch와 원문 문자 위치
- data 뒤에만 올 수 있는 padding, 최대 2개, quantum과 맞는 정확한 padding 길이
- 4개 symbol quantum에서 remainder 1인 incomplete input
- decode 뒤 canonical re-encode 비교로 non-zero pad bit 탐지
- 입력 ASCII whitespace를 제거한 logical index와 원문 index mapping

UTF-8 validator는 ASCII, 2/3/4-byte sequence를 직접 걸으며 overlong sequence, standalone
continuation, UTF-16 surrogate range, U+10FFFF 초과, 잘못된 continuation과 truncated sequence의
최초 문제 byte를 찾는다. validation 성공 뒤에만 TextDecoder를 실행한다. JS input string의
unpaired high/low surrogate도 TextEncoder가 U+FFFD로 바꾸기 전에 문자 위치 오류로 막는다.

### 2. Unified byte codec UI

Files:

- `apps/developer-toolbox/src/tools/ByteCodecTool.tsx`
- `apps/developer-toolbox/src/tools/ByteCodecTool.test.tsx`
- `apps/developer-toolbox/src/tools/index.tsx`
- `apps/developer-toolbox/src/App.css`

sidebar의 `Base64 Encode`, `Base64 Decode` 두 행을 `UTF-8 / Base64 / Hex` 한 행으로 교체했다.
전용 component는 source/target select, decoded byte count, 결과로 입출력 교환, visible copy/save와
기존 input/output context-menu를 제공한다.

오류 surface는 `7번째 문자 · INVALID_HEX_CHARACTER`, `2번째 byte · INVALID_UTF8_BYTES`처럼
position unit을 분리한다. 결과가 없거나 오류가 있으면 copy/save/swap을 비활성화한다. 저장은
binary 실행 파일이 아니라 선택한 표현의 text이며 target에 따라 다음 이름을 쓴다.

- `converted.txt`
- `converted.hex.txt`
- `converted.base64.txt`
- `converted.base64url.txt`

항상 보이는 안내는 UTF-8 text와 raw byte의 차이, replacement 금지, Base64 비밀성 없음,
자동 저장·전송 없음 경계를 설명한다.

### 3. Legacy converter migration

File:

- `apps/developer-toolbox/src/tools/transformers.tsx`

기존 test/API가 참조하는 `toBase64`, `fromBase64`, `base64Encode`, `base64Decode` export는 유지하되
deprecated `escape`/`unescape` 파이프라인을 제거했다. wrapper는 새 core의 UTF-8 ↔ Base64 경로를
사용하므로 기존 ASCII/한글 동작과 새 strict 오류 계약이 같은 구현을 공유한다. JWT decode는
별도 P2 기능 경계를 건드리지 않았다.

### 4. Documentation synchronization

Files:

- `apps/developer-toolbox/README.md`
- `docs/product-opportunities.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

README와 상세 계획에 표현 matrix, whitespace/padding 정책, error position, 상한, privacy와
비범위를 기록했다. roadmap은 #264/PR #396 merge 뒤 #265 진행 상태로 이동했고 product
opportunity inventory에서도 byte codec을 구현 항목으로 전환했다.

추가 dependency가 없어 lockfile과 notices는 바뀌지 않았다. 직전 #264 main production bundle
329,875바이트/gzip 103,710바이트 대비 최종 JS는 337,982바이트/gzip 106,609바이트로 각각
8,107바이트와 2,899바이트 증가했다.

## PR-Boundary Review Findings

PR 직전 전체 diff를 직접 검토해 다음을 보강했다.

1. **최대 Hex 입력 object churn**: 최초 decoder는 nibble마다 `{ value, index }` object를 만들어
   상한 직전 1,000,000개 object가 필요했다. 두 번의 bounded scan과 pre-sized `Uint8Array`로
   바꿔 정상 경로의 per-nibble allocation을 제거했다.
2. **최대 Base64 입력 object churn**: 문자별 value/index object 대신 canonical string만 만들고,
   오류가 있을 때만 ASCII whitespace를 건너 원문 위치를 다시 찾게 했다.
3. **상한 이후 불필요한 decode**: Hex nibble count와 Base64 symbol count에서 decoded byte 길이를
   먼저 계산해 1,000,000바이트를 넘으면 `Uint8Array`/binary string을 만들지 않는다.
4. **Hex output 임시 배열**: `Array.from(bytes, hexString)`은 최대 백만 개 string 배열을 만들었다.
   16KiB byte chunk별 string을 만들어 마지막에 합치도록 변경했다.
5. **BOM round-trip**: default TextDecoder BOM 처리는 선두 EF BB BF를 text output에서 숨길 수
   있었다. `ignoreBOM: true`와 fixture로 U+FEFF를 명시적으로 보존했다.
6. **UTF-8 범위 fixture**: overlong/truncated만으로는 surrogate와 Unicode 최대 범위 초과를 놓칠
   수 있어 ED A0 80, F4 90 80 80 rejection과 valid four-byte emoji를 추가했다.

## Test Coverage

Pure codec tests cover:

- RFC 4648 `""`, `f`, `fo`, `foo`, `foob`, `fooba`, `foobar` vectors
- ASCII·한글 UTF-8/Base64 round-trip and byte count
- NUL/0xFF raw byte preservation, Hex whitespace and Base64URL alphabet
- optional valid padding and unpadded canonical Base64URL output
- BOM preservation
- invalid Hex character and odd nibble positions
- standard/Base64URL alphabet mismatch positions
- padding placement/length, incomplete quantum and non-zero pad bit
- invalid continuation, overlong, truncated, surrogate, out-of-range UTF-8 and valid emoji
- unpaired JS surrogate without replacement
- representation and decoded byte limits for UTF-8, Hex and Base64

UI tests cover:

- default UTF-8 → Base64, decoded byte count, copy and target filename save
- Hex → Base64URL and reverse-input swap
- invalid encoded character position with disabled actions
- invalid UTF-8 byte position and persistent text/raw/Base64 notice

Existing transformer tests confirm the migrated compatibility wrappers and unrelated JSON, URL, JWT,
timestamp and case tools still work.

## Verification Results

### Frontend

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter developer-toolbox test -- --maxWorkers=1
Test Files  6 passed (6)
Tests       67 passed (67)
exit 0

# PR review memory changes and boundary fixtures
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter developer-toolbox exec vitest run \
      src/tools/byteCodec.test.ts --environment=node --maxWorkers=1
Test Files  1 passed (1)
Tests       10 passed (10)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter developer-toolbox exec vitest run \
      src/tools/transformers.test.ts --environment=node --maxWorkers=1
Test Files  1 passed (1)
Tests       29 passed (29)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter developer-toolbox exec vitest run \
      src/tools/ByteCodecTool.test.tsx --maxWorkers=1
Test Files  1 passed (1)
Tests       4 passed (4)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 pnpm --filter developer-toolbox build
130 modules transformed
JS 337.98 kB / gzip 106.99 kB
CSS 7.37 kB / gzip 2.07 kB
exit 0
```

### Rust acceptance

```text
$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p developer-toolbox --lib -j1
8 passed; 0 failed
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo check -p developer-toolbox -j1
exit 0
```

### Repository hygiene

```text
$ git diff --check
exit 0
```

No package or lockfile changed, so the dependency inventory generated and verified in #264 remains
unchanged. Remote CI still runs its dependency, catalog, scope, frontend and Rust gates before merge.

## Resource Discipline

한 feature worktree만 사용하고 Vitest `--maxWorkers=1`, Node 768MiB heap, Cargo `-j1`과
Linux-native shared target을 적용했다. frontend test, typecheck/build와 Rust acceptance는
순차 실행했다. 전체 67-test run은 189.44초였고 assertion은 1.18초, jsdom environment setup이
143.26초였으므로 worker를 늘리지 않았다. review 뒤 큰-input core test는 139ms에 완료됐다.
마무리 시 약 9.7GiB available memory와 6.2GiB free swap이 있었고 watch/test/build process는
남지 않았다.

## Remaining Checkpoint

- Windows packaged WebView2에서 UTF-8/Hex/Base64URL paste, clipboard, text save와 invalid-position
  fixture를 확인하는 W1 evidence는 P1 묶음 checkpoint에서 수행한다.
- Developer Toolbox 목표 version 0.3.0은 Wave 9 version-bump/release preparation에서 적용한다.
- 진법 변환과 JSON → TypeScript는 다음 독립 P1-09 issues #266~#267에 남긴다.
- binary file input/save, UUID v7/ULID, HMAC, JWT verify, QR와 auto-detection/pipeline은 이 PR에
  포함하지 않았다.
