# Developer Toolbox Bounded Radix Converter

## Overview

Issue #266의 P1-09 세 번째 범위로 Developer Toolbox에 2·8·10·16진수 변환기를 추가했다.
입력 진법을 자동 또는 명시적으로 선택하고 네 진법의 canonical signed-magnitude 결과를 동시에
보여 준다.

```text
input text
   │
   ├─ trim outer whitespace
   ├─ optional sign (+ / -)
   ├─ optional 0b / 0o / 0x prefix
   ├─ exact digit + original-position validation
   └─ bounded magnitude accumulation (≤ 256 bits)
              │
              ├─ BIN  -0b...
              ├─ OCT  -0o...
              ├─ DEC  -...
              └─ HEX  -0x...
```

JavaScript `BigInt`는 Number precision 손실을 피하기 위한 내부 계산 수단으로만 사용한다. 입력
512자와 절댓값 256bit 상한을 강제해 무제한 arbitrary-precision 계산기로 확장하지 않았다.
새 dependency, network, storage와 외부 도구가 없으며 Windows packaged smoke는 W1 P1 묶음
checkpoint에 남겼다.

## Context and Constraints

- 개발자는 protocol flag, permission mask, RGB/UUID 일부, 64/128/256bit identifier를 여러
  진법으로 빠르게 확인해야 하지만 JavaScript Number는 53bit를 넘으면 정수 정밀도를 잃는다.
- issue acceptance는 Hex/Dec/Bin/Oct, sign/prefix와 invalid digit 위치다. floating point,
  fraction, arbitrary precision policy, bitwise/two's-complement width 선택은 범위가 아니다.
- 자동 모드가 legacy leading-zero octal을 추측하면 `08` 같은 입력이 혼란스러우므로, 명시적
  `0b`, `0o`, `0x`만 감지하고 prefix 없는 값은 10진수로 고정한다.
- sign은 unary sign처럼 prefix 앞에만 허용한다. output도 `-0x2a` 순서의 signed magnitude다.
- 오류에는 parser input, 숫자 원문이나 clipboard exception 내용을 넣지 않는다.
- 앱 version 0.3.0 bump는 Wave 9 release preparation에서 별도로 수행한다.

## Input and Output Contract

### Input modes

| Mode | Prefix behavior | Prefixless value |
|---|---|---|
| Auto | `0b`→2, `0o`→8, `0x`→16 | decimal |
| Binary | optional matching `0b` | base 2 |
| Octal | optional matching `0o` | base 8 |
| Decimal | known non-decimal prefix rejected | base 10 |
| Hexadecimal | optional matching `0x`; known other prefix rejected | base 16 |

Prefix letter는 대소문자를 구분하지 않는다. decimal `0d`, legacy octal zero, suffix `n`, decimal
point, exponent, underscore와 내부 whitespace는 지원하지 않는다. 외부 whitespace만 trim한다.

### Canonical output

- Binary: optional `-`, `0b`, lowercase digits
- Octal: optional `-`, `0o`, digits
- Decimal: optional `-`, no prefix
- Hexadecimal: optional `-`, `0x`, lowercase digits
- positive `+` is omitted
- negative zero is normalized to `0`, `0b0`, `0o0`, `0x0`

출력은 two's complement가 아닌 signed magnitude다. 최대 magnitude는 `2^256 - 1`이고 negative도
같은 절댓값 상한을 적용한다.

## Changes Made

### 1. Position-aware bounded parser

Files:

- `apps/developer-toolbox/src/tools/radix.ts`
- `apps/developer-toolbox/src/tools/radix.test.ts`

`convertRadix()`는 outputs, input base/digit/bit metadata와 structured error를 반환한다. 값은 digit를
한 글자씩 누적하고 매 단계에서 256bit maximum을 비교하므로 overflow가 처음 발생한 원문 digit
위치를 정확히 보고한다. 전체 input을 먼저 BigInt parser에 넘기지 않는다.

오류 code는 다음 경계를 구분한다.

- `INPUT_TOO_LONG`
- `SIGN_WITHOUT_DIGITS`
- `PREFIX_WITHOUT_DIGITS`
- `BASE_PREFIX_MISMATCH`
- `INVALID_DIGIT`
- `VALUE_OUT_OF_RANGE`

known prefix는 digit scan보다 먼저 처리한다. 따라서 explicit hex에서 `0b10`을 hexadecimal digit
`b10`으로 오해하지 않고 prefix mismatch로 중단한다. 모든 메시지는 selected/detected base와
고정 문구만 포함한다.

64bit maximum `18446744073709551615`, 256bit maximum과 그 다음 digit을 fixture로 고정해 Number
coercion이나 overflow 후 truncation이 생기지 않게 했다.

### 2. Multi-output UI and actions

Files:

- `apps/developer-toolbox/src/tools/RadixTool.tsx`
- `apps/developer-toolbox/src/tools/RadixTool.test.tsx`
- `apps/developer-toolbox/src/tools/index.tsx`
- `apps/developer-toolbox/src/App.css`

Encoding group에 `Radix Converter`를 추가했다. UI는 input mode select, app-owned context-menu가 있는
single-line input, parse 위치 alert, detected base/digit/bit metadata와 네 output row를 제공한다.

각 output은 공용 `ToolOutput`을 사용해 context-menu copy/select/save를 제공하고 눈에 보이는 개별
copy button도 가진다. 전체 결과는 다음 deterministic text로 copy/save한다.

```text
BIN 0b101010
OCT 0o52
DEC 42
HEX 0x2a
```

save filename은 `radix-conversion.txt`이며 개별 context save는 `.bin.txt`, `.oct.txt`, `.dec.txt`,
`.hex.txt`를 쓴다. clipboard/download 실패는 result를 유지하고 고정 오류만 표시한다.

notice는 sign-before-prefix, auto detection, signed magnitude, no two's complement, 256bit bound,
separator 비지원과 자동 저장·전송 없음 계약을 항상 표시한다. 760px 이하에서는 input/output grid를
한 column으로 내려 긴 binary output과 좁은 창을 함께 처리한다.

### 3. Documentation synchronization

Files:

- `apps/developer-toolbox/README.md`
- `docs/product-opportunities.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

README와 상세 계획에 input/output contract, 512-character/256-bit bounds, invalid position,
signed-magnitude와 explicit non-scope를 기록했다. roadmap은 #264/PR #396과 #265/PR #397 merge
뒤 #266 진행 상태로 이동했고 product opportunity inventory도 구현 상태로 갱신했다.

새 dependency와 lockfile 변화는 없다. 직전 #265 main production JS 337,982바이트/gzip
106,609바이트에서 342,822바이트/gzip 108,119바이트로 4,840/1,510바이트 증가했다. CSS는
7,366/2,084바이트에서 8,376/2,267바이트로 1,010/183바이트 증가했다.

## PR-Boundary Review Findings

전체 diff와 acceptance를 직접 검토하며 다음을 확인·보강했다.

1. **arbitrary precision 경계**: BigInt 사용 자체가 무제한 숫자 기능으로 번지지 않게 input 512자,
   magnitude 256bit를 public constants와 overflow-position fixture로 고정했다.
2. **known-prefix ambiguity**: explicit hex의 `0b10`에서 `b`가 valid hex digit이므로 prefix를 먼저
   인식하지 않으면 0xB10으로 오판할 수 있다. known prefix mismatch가 digit scan보다 앞서도록
   하고 sign 포함 위치를 테스트했다.
3. **overflow 위치 fixture 계산**: `0x1` + 64 zero의 마지막 digit은 prefix 포함 67번째 문자다.
   최초 test expectation의 66을 실제 원문 위치 67로 바로잡아 UI와 parser의 1-based 계약을 맞췄다.
4. **raw input/error reflection**: invalid value에 marker를 넣고 structured error 전체에 원문이 없는지
   검증했다. clipboard rejection reason도 fixed UI error로 격리하고 output을 유지한다.
5. **privacy wording**: numeric input도 token/identifier일 수 있으므로 자동 저장·전송 없음 안내를
   README/spec뿐 아니라 사용 화면에 계속 표시했다.
6. **narrow-window layout**: 256bit binary output과 sidebar를 고려해 760px 이하 single-column
   fallback과 horizontal output scroll을 추가했다.

## Test Coverage

Pure tests cover:

- auto `0b`/`0o`/`0x` and prefixless decimal detection
- explicit 2/8/10/16 parsing without prefix
- sign-before-prefix, plus canonicalization and negative zero
- selected-base/prefix mismatch positions
- invalid digit positions with leading whitespace/sign
- sign/prefix without digits and misplaced sign
- internal whitespace and underscore rejection
- 256bit max accept and first-overflow position
- exact 64bit value beyond Number safe integer
- blank input and 512-character limit
- raw marker absence from structured errors

UI tests cover:

- all four auto-detected outputs and metadata
- individual result copy
- explicit binary negative signed-magnitude output
- persistent two's-complement/privacy notice
- invalid digit position with hidden/disabled actions
- deterministic all-output copy/save
- clipboard failure isolation and output preservation

## Verification Results

### Frontend

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter developer-toolbox test -- --maxWorkers=1
Test Files  8 passed (8)
Tests       82 passed (82)
exit 0

# PR review fixtures
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter developer-toolbox exec vitest run \
      src/tools/radix.test.ts --environment=node --maxWorkers=1
Test Files  1 passed (1)
Tests       10 passed (10)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter developer-toolbox exec vitest run \
      src/tools/RadixTool.test.tsx --maxWorkers=1
Test Files  1 passed (1)
Tests       5 passed (5)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 pnpm --filter developer-toolbox build
132 modules transformed
JS 342.82 kB / gzip 108.51 kB
CSS 8.38 kB / gzip 2.25 kB
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
$ bash .github/scripts/check-catalog.sh
exit 0

$ git diff --check
exit 0
```

No dependency/lockfile changed, so generated notices and dependency inventory remain byte-identical.
Remote CI still validates dependency policy, catalog, scoped frontend and Rust gates before merge.

## Resource Discipline

한 feature worktree에서만 작업하고 Vitest `--maxWorkers=1`, Node 768MiB heap, Cargo `-j1`과
Linux-native shared target을 사용했다. 전체 82-test 회귀는 259.21초였고 assertion 1.34초,
jsdom environment setup 198.55초였다. 긴 setup 중에도 worker를 늘리거나 중복 test를 실행하지
않았다. 마무리 시 available memory 약 9.5GiB, free swap 6.1GiB였고 background watch/test/build
process는 남지 않았다.

## Remaining Checkpoint

- Windows packaged WebView2에서 input context-menu, 256bit binary horizontal scroll, copy/save와
  invalid-position fixture를 확인하는 W1 evidence는 P1 묶음 checkpoint에서 수행한다.
- Developer Toolbox 목표 version 0.3.0은 Wave 9 version-bump/release preparation에서 적용한다.
- JSON → TypeScript는 다음 독립 P1-09 issue #267에 남긴다.
- floating point, fractions, arbitrary precision, two's-complement width interpretation, bitwise
  calculator와 digit grouping 옵션은 이 PR에 포함하지 않았다.
