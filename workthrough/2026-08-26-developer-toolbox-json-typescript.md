# Developer Toolbox JSON to TypeScript Generator

## Overview

Issue #267의 P1-09 네 번째 범위로 Developer Toolbox에 strict JSON 표본을 TypeScript type
선언으로 바꾸는 generator를 추가했다. 사용자가 root type 이름을 지정하고 결과를 즉시 복사하거나
`.ts` 파일로 저장할 수 있다.

```text
strict JSON + root type name
          │
          ├─ bounded parse + fixed position error
          ├─ recursive primitive/null/array/object inference
          ├─ batch merge object samples in arrays
          ├─ optional property + canonical union inference
          └─ code-point sort + deterministic renderer
                         │
                         └─ export interface / export type
```

새 runtime dependency, sidecar, network와 storage는 없다. 기존 `jsonc-parser` 3.3.1을 strict JSON
구문 위치에 재사용하고, type inference와 renderer는 TypeScript 순수 로직으로 구현했다. Windows
packaged smoke는 W1 P1 묶음 checkpoint에 남겼다.

## Context and Constraints

- acceptance는 root type 이름, optional/null/array/object 추론과 deterministic 출력이다.
- JSON 하나만으로 도메인 전체 schema를 알 수 없으므로 literal value, enum, date, branded type을
  추측하지 않는다. primitive 종류와 관찰된 구조만 반영한다.
- optional은 object가 단독으로 등장할 때 임의 생성하지 않고, 같은 배열의 object 표본 중 속성이
  누락된 경우에만 생성한다.
- array sample 순서와 object key 원문 순서는 결과에 영향을 주지 않아야 한다.
- typed pipeline, schema import, 다른 언어 generator와 파일 input은 별도 범위다.
- JSON 원본은 credential이나 개인 데이터를 포함할 수 있어 생성 코드·오류·로그에 값 자체를
  반영하지 않고 자동 저장하거나 전송하지 않는다.
- 앱 version 0.3.0 bump는 Wave 9 release preparation에서 별도로 수행한다.

## Input and Output Contract

### Root declaration

root 이름은 1~80자의 ASCII TypeScript identifier다. 첫 글자는 영문자, `_`, `$`이고 나머지는
숫자를 추가로 허용한다. JavaScript/TypeScript 예약어와 contextual type keyword는 고정 오류로
거부한다.

| JSON root | Generated declaration |
|---|---|
| non-empty object | `export interface <Root> { ... }` |
| empty object | `export type <Root> = Record<string, never>;` |
| array | `export type <Root> = Array<...>;` |
| string/number/boolean/null | `export type <Root> = ...;` |

ASCII identifier가 아닌 JSON property key는 JSON string literal로 quote한다. `__proto__`도
prototype mutation 없이 data property로 유지한다.

### Inference

- string, number, boolean, null은 각각 TypeScript primitive/null로 바꾼다.
- 빈 배열은 표본이 없음을 나타내는 `Array<unknown>`으로 만든다.
- 같은 배열의 primitive 종류는 union으로 합친다.
- 같은 배열의 object는 구조를 병합하고 하나 이상의 표본에서 빠진 속성을 `?`로 표시한다.
- 같은 속성의 nested object와 array도 재귀적으로 병합한다.
- union member와 object property는 locale-dependent `localeCompare`가 아니라 code-point 비교로
  정렬한다.
- literal value, numeric range, tuple, enum, Date, undefined와 discriminated union은 추론하지
  않는다.

### Bounds and errors

| Bound | Limit | Error |
|---|---:|---|
| UTF-8 input | 1,000,000 bytes | `INPUT_TOO_LARGE` |
| nesting | 64 levels | `INPUT_TOO_DEEP` |
| visited JSON values | 100,000 | `INPUT_TOO_COMPLEX` |
| generated UTF-8 output | 4,000,000 bytes | `OUTPUT_TOO_LARGE` |
| root name | 80 characters | `ROOT_TYPE_NAME_TOO_LONG` |

Strict JSON comment와 trailing comma를 허용하지 않는다. parse failure는 1-based line/column과
`INVALID_JSON` 같은 고정 code/message만 반환한다. inference limit과 unexpected failure도 원문을
반향하지 않는 고정 오류로 격리한다.

## Changes Made

### 1. Deterministic inference core

Files:

- `apps/developer-toolbox/src/tools/jsonTypescript.ts`
- `apps/developer-toolbox/src/tools/jsonTypescript.test.ts`

`convertJsonToTypescript()`는 `{ output, error }` 구조를 반환한다. 내부 `TypeNode`는 primitive,
array, object, union과 empty-array evidence를 위한 `never` sentinel을 구분한다. sentinel은 다른
표본이 생기면 그 type으로 흡수되고 끝까지 비어 있을 때만 `unknown`으로 렌더링된다.

object-array merge는 표본을 pairwise로 누적하지 않는다. 모든 표본을 한 번 순회해 property별
등장 횟수, 이미 optional인지와 관찰 type을 모은 뒤 한 번만 정렬·병합한다. 이 방식은 서로 다른
key가 계속 등장할 때 매 단계마다 커진 object를 복사하는 최악의 제곱 비용을 피한다. 2,000개
sparse object의 정방향·역방향 출력이 같은 fixture로 이 경계를 고정했다.

structural `typeKey`와 explicit code-point comparator로 union을 deduplicate/sort한다. renderer는
2-space indentation, semicolon과 마지막 newline을 고정한다. 따라서 key 순서와 sample 순서가
다른 동등 입력은 byte-identical output을 만든다.

### 2. Root-name and parser safety

기존 bundled `jsonc-parser` visitor를 strict mode로 호출해 runtime별 `JSON.parse` 오류 문구 대신
offset을 얻는다. JSON.parse는 visitor가 성공한 뒤에만 실행하고 parser exception, inference limit,
unexpected exception을 각각 고정 오류로 바꾼다.

root 이름은 코드에 직접 들어가므로 free-form text를 interpolate하지 않는다. identifier와
reserved keyword validation을 통과한 값만 declaration과 download filename에 사용한다. property
key는 ASCII identifier가 아니면 `JSON.stringify` string literal로 출력한다.

### 3. UI and explicit actions

Files:

- `apps/developer-toolbox/src/tools/JsonTypescriptTool.tsx`
- `apps/developer-toolbox/src/tools/JsonTypescriptTool.test.tsx`
- `apps/developer-toolbox/src/tools/index.tsx`
- `apps/developer-toolbox/src/App.css`

JSON group에 `JSON → TypeScript`를 추가했다. 상단에는 app-owned context menu를 쓰는 root-name
field와 visible copy/save button이 있고, 아래에는 strict JSON input과 read-only TypeScript output을
나란히 둔다. parse 오류는 line/column과 code를 표시하며 어떤 오류든 output action을 비활성화한다.

notice는 object-array optional 병합, null union, empty-array unknown, 값 비포함과 자동 저장·전송
없음을 계속 표시한다. `ToolOutput` context menu의 copy/select/save도 유지한다. clipboard/download
exception 원문은 UI에 반향하지 않고 고정 action error만 표시하며 기존 결과를 지우지 않는다.
760px 이하에서는 입·출력 영역을 한 column으로 내려 좁은 창에서도 생성 코드를 읽을 수 있게 했다.

### 4. Documentation synchronization

Files:

- `apps/developer-toolbox/README.md`
- `docs/product-opportunities.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

README와 상세 계획에 inference contract, deterministic ordering, safety bounds와 explicit non-scope를
기록했다. roadmap은 #266/PR #398 merge 뒤 #267 범위로 이동했고 product opportunity inventory도
구현 상태로 갱신했다.

새 dependency와 lockfile 변화는 없다. 직전 #266 main production JS 342,822바이트/gzip
108,119바이트에서 350,471바이트/gzip 110,314바이트로 7,649/2,195바이트 증가했다. CSS는
8,376/2,267바이트에서 8,998/2,341바이트로 622/74바이트 증가했다.

## PR-Boundary Review Findings

전체 diff와 acceptance를 직접 검토하며 다음을 확인·보강했다.

1. **optional 의미**: 단일 object의 모든 property를 required로 두고 같은 array 표본의 누락만
   optional로 처리해, 한 JSON snapshot에서 근거 없이 optional을 확대하지 않았다.
2. **empty-array evidence**: 빈 배열을 즉시 `unknown` top type으로 합치면 `[[], [1]]`도 unknown에
   흡수된다. 내부 `never` sentinel을 사용해 다른 표본이 있으면 `Array<Array<number>>`처럼 실제 evidence를
   우선하고, 정말 빈 배열뿐일 때만 unknown을 출력한다.
3. **batch merge complexity**: pairwise object merge는 sparse key가 늘 때 제곱 비용이 될 수 있다.
   property occurrence를 batch 집계하도록 바꾸고 2,000-sample reverse-order fixture를 추가했다.
4. **determinism**: object key, union member와 array object sample 순서를 모두 바꾸는 fixture에서
   byte-identical output을 검증했다. host locale에 의존하는 비교 함수는 사용하지 않는다.
5. **code injection boundary**: root 이름을 ASCII identifier/keyword allow boundary로 제한하고
   비-identifier property key를 JSON string literal로 quote했다. `__proto__`는 Map과 own-key 순회로
   다뤄 prototype write가 일어나지 않는다.
6. **resource/error boundary**: input/output byte, depth와 node count를 독립 제한하고 deeply nested,
   oversized, parser failure가 raw input 없이 고정 오류로 끝나는지 확인했다.
7. **privacy/action boundary**: 값은 type inference에만 쓰고 output에 포함하지 않는다. clipboard
   rejection reason도 고정 UI 오류로 격리하고 명시적 copy/save 전에는 외부 action이 없다.

## Test Coverage

Pure tests cover:

- sorted root interface and nested object/array rendering
- object-array optional property merge and nullable union
- key/sample order-independent byte-identical output
- 2,000 sparse object batch merge regression
- empty/mixed arrays and primitive root aliases
- empty object and quoted non-identifier property key
- `__proto__` data property preservation
- strict JSON comment/trailing-comma rejection
- 1-based parse position and raw marker non-reflection
- empty/invalid/reserved root type names
- UTF-8 byte, nesting depth and visited-value limits

UI tests cover:

- custom root name and optional generated output
- persistent inference/privacy notice
- visible clipboard copy and root-name `.ts` save
- parse-position/root-name error with disabled actions
- clipboard failure isolation and output preservation

## Verification Results

### Frontend

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter developer-toolbox exec vitest run --maxWorkers=1 \
      src/tools/jsonTypescript.test.ts src/tools/JsonTypescriptTool.test.tsx
Test Files  2 passed (2)
Tests       14 passed (14)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter developer-toolbox exec tsc --noEmit
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter developer-toolbox test -- --maxWorkers=1
Test Files  10 passed (10)
Tests       96 passed (96)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 pnpm --filter developer-toolbox build
134 modules transformed
JS 350.47 kB / gzip 110.76 kB
CSS 9.00 kB / gzip 2.33 kB
exit 0
```

### Rust acceptance

```text
$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p developer-toolbox --jobs 1
8 passed; 0 failed
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo check -p developer-toolbox --jobs 1
exit 0
```

### Repository hygiene

```text
$ bash .github/scripts/check-catalog.sh
exit 0

$ git diff --check
exit 0
```

No dependency/lockfile changed, so generated notices and dependency inventory remain unchanged. Remote CI
still validates dependency policy, catalog, scoped frontend and Rust gates before merge.

## Resource Discipline

한 feature worktree에서만 작업하고 Vitest `--maxWorkers=1`, Node 768MiB heap, Cargo job 1과
Linux-native shared target을 사용했다. 전체 96-test 회귀는 318.69초였고 assertion은 1.56초,
jsdom environment setup은 242.50초였다. worker PID가 test file마다 교체되는 동안에도 추가 worker나
중복 suite를 띄우지 않았다. 검증 전 available memory는 약 9.7GiB, free swap은 6.1GiB였고 Cargo
작업도 순차로 실행했다.

## Remaining Checkpoint

- Windows packaged WebView2에서 root-name input context menu, long generated code scroll, copy/save와
  invalid-position fixture를 확인하는 W1 evidence는 P1 묶음 checkpoint에서 수행한다.
- Developer Toolbox 목표 version 0.3.0은 Wave 9 version-bump/release preparation에서 적용한다.
- API Playground header table은 다음 독립 P1-09 issue #268에 남긴다.
- literal/enum/date/tuple/discriminated-union inference, schema import, typed pipeline, file input과 다른
  language generator는 이 PR에 포함하지 않았다.
