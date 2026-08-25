# Developer Toolbox JSON ↔ YAML Converter

## Overview

Issue #264의 P1-09 첫 범위로 Developer Toolbox에 JSON ↔ YAML 1.2 양방향 변환기를 추가했다.
기능은 앱 번들 안에서 오프라인으로 동작하며 외부 converter, sidecar, network 또는 runtime
download에 의존하지 않는다.

사용자는 변환 방향을 즉시 바꾸고, 결과를 복사하거나 방향에 맞는 `.yaml`/`.json` 파일로
저장하며, 현재 결과를 반대 방향 입력으로 넘겨 왕복 확인할 수 있다. YAML → JSON에서는 comment,
anchor/alias의 JSON 표현 한계를 항상 표시한다.

```text
strict JSON ── validate(offset) ── native JSON value ── YAML 1.2 stringify
     ▲                                                        │
     │                                                        │
     └──────────── 반대 방향 입력으로 사용 ────────────────────┘

YAML 1.2 ── strict document parse ── bounded alias expansion ── JSON stringify
                  │
                  ├─ comment 제거 고지
                  ├─ anchor 이름·공유 관계 손실 고지
                  └─ tag/unsafe number/graph fail closed
```

Windows packaged smoke는 계획의 W1 정책에 따라 P1 묶음 checkpoint에 남겼다.

## Context and Constraints

- 기존 Toolbox JSON Formatter는 native `JSON.parse` 오류 문자열만 보여 주며 런타임에 따라
  위치 정보가 없을 수 있었다.
- YAML은 comment, anchor, alias와 tag처럼 JSON에 직접 대응하지 않는 표현을 포함하므로 조용한
  손실을 만들지 않는 계약이 필요했다.
- 개발자가 인터넷이나 별도 converter 설치 없이 붙여넣기만으로 변환을 끝낼 수 있어야 한다.
- YAML merge key의 자동 확장, 여러 문서, duplicate mapping key와 과도한 alias expansion은
  편의보다 예측 가능성과 resource safety를 우선해 거부한다.
- parser가 만든 원문 포함 오류 메시지와 입력 내용은 UI 오류·log에 반영하지 않는다.
- version bump는 Wave 9 release preparation에서 수행하므로 앱 version 0.2.2는 이 기능 PR에서
  바꾸지 않는다.

## Changes Made

### 1. Bounded conversion core

Files:

- `apps/developer-toolbox/src/tools/jsonYaml.ts`
- `apps/developer-toolbox/src/tools/jsonYaml.test.ts`

`convertJsonYaml()`은 빈 입력, JSON → YAML, YAML → JSON을 하나의 구조화된 결과로 반환한다.
오류는 fixed code/message와 optional 1-based line/column만 가지며 raw parser message를 전달하지
않는다.

공통 상한은 다음과 같다.

- UTF-8 입력 최대 1,000,000바이트
- UTF-8 출력 최대 4,000,000바이트
- YAML alias expansion 최대 50개
- YAML 문서 한 개

JSON은 `jsonc-parser` visitor를 strict mode로 실행해 comment와 trailing comma를 금지하고 최초
오류 offset을 얻는다. visitor는 값을 구성하지 않으며, validation 성공 뒤 실제 값은 native
`JSON.parse`로 만든다. 따라서 `__proto__`도 prototype setter가 아니라 JSON data key로 그대로
보존한다.

visitor literal callback은 non-finite number와 safe integer 범위 밖의 정수를 위치와 함께
거부한다. YAML parser도 integer를 먼저 BigInt로 보존한 뒤 safe integer만 number로 정규화한다.
`.inf`, `.nan`, 범위 밖 정수와 plain object/array가 아닌 특수 값은 JSON으로 조용히 바꾸지 않고
`UNSUPPORTED_YAML_VALUE`로 중단한다.

YAML parser는 1.2, strict, unique/string key, merge off, known-tag 자동 해석 off로 고정한다.
errors뿐 아니라 unresolved custom tag warning도 fixed error로 처리한다. `toJS` 이후에는 visiting과
finished object를 분리해 shared alias는 허용하되 circular graph를 탐지하고, parser의 alias count
guard와 함께 `UNSUPPORTED_YAML_GRAPH`로 격리한다.

### 2. Dedicated conversion UI

Files:

- `apps/developer-toolbox/src/tools/JsonYamlTool.tsx`
- `apps/developer-toolbox/src/tools/JsonYamlTool.test.tsx`
- `apps/developer-toolbox/src/tools/index.tsx`
- `apps/developer-toolbox/src/App.css`

기존 `TransformerTool`은 단방향 text transform을 위한 공통 surface라 방향, 손실 고지, 파일명과
reverse-input action을 함께 소유하기 어렵다. 이 기능은 전용 component로 두되 입력·출력의
context-menu와 download primitive는 기존 `ToolTextArea`, `ToolOutput`, `downloadTextResult`를
재사용했다.

UI는 다음을 제공한다.

- `JSON → YAML`, `YAML → JSON` pressed-state 방향 버튼
- 변환 결과가 있을 때만 활성화되는 reverse-input action
- YAML → JSON에서 항상 보이는 comment/anchor/alias 손실 안내
- 입력 형식과 출력 형식을 표시한 좌우 editor surface
- 고정 오류와 가능한 경우 `행 열 · CODE` 위치 표시
- 눈에 보이는 복사·저장 버튼과 기존 output context-menu
- 방향별 `converted.yaml`, `converted.json` 저장 이름
- clipboard/download 실패를 결과와 분리한 복구 가능한 고정 오류

### 3. Runtime dependencies and offline contract

Files:

- `apps/developer-toolbox/package.json`
- `pnpm-lock.yaml`
- `THIRD_PARTY_NOTICES.md`

추가한 exact runtime dependency는 두 개다.

| Package | 역할 | License | Dependencies | npm unpacked |
|---|---|---|---:|---:|
| `jsonc-parser` 3.3.1 | strict JSON validation과 오류 offset | MIT | 0 | 212,821 bytes |
| `yaml` 2.9.0 | YAML 1.2 document parse/stringify | ISC | 0 | 685,953 bytes |

`JSON.parse` 단독 사용은 Node 24를 포함한 일부 런타임에서 오류 위치가 없는 경우가 확인됐다.
수제 JSON/YAML parser는 형식 정확성과 장기 유지비가 크고, 외부 converter 실행은 오프라인·
single-app workflow 목표를 충족하지 못해 선택하지 않았다.

기능과 두 package를 포함한 production JS는 main의 기존 204,020바이트에서 329,875바이트로
125,855바이트 증가했고, 같은 `gzip -c` 측정은 63,992바이트에서 103,710바이트로 39,718바이트
증가했다. one-shot Node baseline/import+small-conversion 비교의 peak RSS는 42,768KiB에서
48,260KiB로 약 5,492KiB 증가했다. 이는 packaged WebView 전체 메모리가 아니라 dependency
import 비용을 비교하기 위한 동일 환경 근사치다.

두 package는 설치물에 정적으로 번들되고 runtime network call이 없다. exact version, lockfile
integrity, allowlisted license와 generated notices를 CI에서 검증한다. Dependabot과 upstream
release/advisory를 유지보수 경로로 사용하고 native equivalent가 충분해지면 제거 가능성을
재검토한다.

### 4. Documentation synchronization

Files:

- `apps/developer-toolbox/README.md`
- `docs/product-opportunities.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

README에 사용자 동작, 손실, 상한, offline dependency를 기록했다. 상세 계획에는 #264 구현
상태와 §1.3의 목적·대안·출처·고정·license·크기·보안·offline·유지보수 검토표를 채웠다.
roadmap은 #263 merge와 P1-09 첫 작업 상태로 이동했고, product opportunity의 JSON↔YAML 후보는
구현 항목으로 전환하되 CSV↔JSON·URL parser 후보는 유지했다.

## PR-Boundary Review Findings

PR 생성 직전 전체 diff를 직접 검토하며 다음 문제를 찾아 수정했다.

1. **런타임별 JSON 위치 누락**: Node 24는 일부 malformed JSON에 입력 일부만 포함한 메시지를
   내고 offset/line을 주지 않았다. parser 문자열 정규식 대신 validator callback의 구조화된
   offset으로 바꿨다.
2. **`__proto__` key 손실**: `jsonc-parser.parse()`의 object assignment는 해당 key를 data
   property가 아닌 prototype setter로 처리할 수 있었다. visitor는 validation만 하고 native
   `JSON.parse`가 값을 만들게 해 key와 값을 보존했다.
3. **unsafe number의 조용한 변형**: `9007199254740993`과 `1e400`, YAML `.inf`가 JS number 또는
   JSON `null`로 의미가 바뀔 수 있었다. literal 위치 검사와 BigInt 보존·normalization을 넣고
   안전하게 표현할 수 없는 값을 고정 오류로 거부했다.
4. **YAML custom tag 손실**: unresolved tag는 parser error가 아니라 warning이어서 scalar로
   변환될 수 있었다. warning도 위치가 있는 `TAG_RESOLVE_FAILED`로 fail closed 처리했다.
5. **방향별 저장 검증 부족**: 최초 UI test가 `.yaml`만 확인했다. YAML → JSON에서도 visible
   저장 버튼이 `converted.json`을 지정하는 fixture를 추가했다.
6. **과도한 JSON 중첩 예외**: byte 상한 안에서도 validator recursion이 stack 한계를 넘을 수
   있었다. validator 호출 자체를 고정 오류 경계로 감싸 UI render까지 예외가 전파되지 않게 했다.

## Test Coverage

Pure conversion tests cover:

- deterministic nested JSON → YAML formatting
- comment removal과 anchor/alias value expansion
- merge key non-expansion
- JSON/YAML malformed location과 raw secret 비노출
- strict JSON comment/trailing comma rejection
- `__proto__` data key preservation
- JSON/YAML unsafe integer와 non-finite number rejection
- custom YAML tag, duplicate key와 multi-document rejection
- byte 상한 안의 과도한 JSON 중첩 예외 격리
- circular/excessive aliases
- blank input과 UTF-8 byte limit

UI tests cover:

- default JSON → YAML conversion
- visible copy and `.yaml` save
- YAML → JSON persistent loss notice and `.json` save
- parse location alert and disabled empty-result actions
- reverse-input round trip and direction pressed state

## Verification Results

### Frontend

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter developer-toolbox test -- --maxWorkers=1
Test Files  4 passed (4)
Tests       48 passed (48)
exit 0

# PR-boundary safety fixes after the full run
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter developer-toolbox exec vitest run \
      src/tools/jsonYaml.test.ts --environment=node --maxWorkers=1
Test Files  1 passed (1)
Tests       13 passed (13)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter developer-toolbox exec vitest run \
      src/tools/JsonYamlTool.test.tsx --maxWorkers=1
Test Files  1 passed (1)
Tests       4 passed (4)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter developer-toolbox exec tsc --noEmit
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 pnpm --filter developer-toolbox build
128 modules transformed
JS 329.88 kB / gzip 103.97 kB
CSS 7.10 kB / gzip 2.02 kB
exit 0
```

### Repository policy

```text
$ pnpm audit --audit-level moderate
No known vulnerabilities found

$ python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml

$ python3 .github/scripts/test-check-dependencies.py
dependency policy regression tests passed

$ python3 .github/scripts/test-build-manifest.py
build-manifest notice tests passed

$ bash .github/scripts/check-catalog.sh
exit 0

$ git diff --check
exit 0
```

### Rust acceptance

Rust source, Cargo manifest와 Cargo lockfile은 바뀌지 않았지만 issue에 명시된 acceptance를 해당
앱 패키지에 한정해 확인했다.

```text
$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p developer-toolbox --lib -j1
8 passed; 0 failed
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo check -p developer-toolbox -j1
exit 0
```

CI scope는 pnpm lockfile 때문에 전체 frontend를 원격에서 검증하고 Rust compile scope는 `none`이다.
따라서 변경과 무관한 로컬 workspace 전체 Cargo build는 수행하지 않았다.

## Resource Discipline

하나의 feature worktree만 사용했다. Vitest는 `--maxWorkers=1`, Node는 768MiB heap cap으로
frontend test, typecheck, build와 dependency gate를 순차 실행했다. `/mnt/e`의 jsdom environment
초기화가 길어도 worker를 추가하지 않았다. 마무리 시 약 10GiB available memory와 6.2GiB free
swap이 있었고 background watch/test/build process는 남지 않았다.

## Remaining Checkpoint

- Windows packaged Developer Toolbox에서 양방향 paste/copy/save와 대용량·malformed fixture를
  확인하는 W1 evidence는 P1 묶음 checkpoint에서 수행한다.
- Developer Toolbox 목표 version 0.3.0은 Wave 9 version-bump/release preparation에서 적용한다.
- Base64/Base64URL/hex, radix와 JSON → TypeScript는 #265~#268의 독립 P1-09 PR 범위다.
- CSV↔JSON, URL parser, auto-detection/pipeline/recent/favorite와 Toolbox → API handoff는 이 PR에
  포함하지 않았다.
