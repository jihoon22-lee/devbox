# Developer Toolbox UUID v4/v7 · ULID Generator

## Overview

Issue #286의 P2-03 단일 기능 PR 초안을 PR 전 감사하고 보강했다. Developer Toolbox에서
UUID v4, RFC 9562 UUID v7, canonical Crockford Base32 ULID를 외부 서비스나 별도 설치 없이
한 번의 bounded batch로 생성할 수 있도록 native Tauri 경로와 browser preview 경로를 같은
계약으로 맞췄다.

이번 보강의 중요한 결정은 native UUID 생성에서 `uuid::new_v4()`/`now_v7()`의 panic을
`catch_unwind`로 감싸는 데 그치지 않은 것이다. 해당 helper는 OS CSPRNG 오류를 panic
문자열로 만들고 기본 panic hook에 플랫폼 세부사항을 기록할 수 있으므로, native도
`getrandom::fill`의 명시적인 `Result`를 사용한다. UUID의 RFC 형식/파싱에는 `uuid`를
계속 사용하고, v7 monotonic 상태는 process-local mutex에 보관한다. 따라서 CSPRNG 실패와
순서 상태 고갈은 고정된 안전 오류로만 끝나며 raw panic/OS 오류는 IPC·UI·로그 경로에
반향되지 않는다.

## Contract and Scope

- 한 요청은 1–100개이며 0, 101 이상, 알 수 없는 종류와 표시 옵션은 생성 전에 거부한다.
- UUID v4는 122 random bits와 version 4/RFC variant를 사용하며 순서를 약속하지 않는다.
- UUID v7은 48-bit Unix millisecond timestamp, version 7, RFC variant와 74-bit variable
  suffix를 사용한다. 같은 process의 concurrent 호출도 mutex 상태로 직렬화한다.
- ULID는 48-bit millisecond timestamp와 80-bit random suffix를 사용하고 26자 Crockford
  Base32로 인코딩한다. 첫 문자는 canonical upper bound 때문에 `0`–`7`만 허용된다.
- UUID v7과 ULID는 한 batch 안에서 wall clock이 같은 millisecond에 머물거나 뒤로 이동해도
  이전 값의 variable suffix를 1씩 증가시켜 엄격히 사전식 증가한다. suffix가 고갈되면
  timestamp를 1ms 올리고 새 random tail을 사용하며, 48-bit timestamp 상한에서 더 이상
  진행할 수 없으면 `식별자 생성 순서를 유지할 수 없습니다.`로 중단한다.
- native UUID v7은 별도 호출에서도 process-local state를 유지하지만 process·machine 간
  전역 순서는 보장하지 않는다. browser batch도 같은 batch의 순서만 보장한다.
- UUID는 canonical hyphen/compact와 대·소문자를 선택할 수 있다. ULID 기본 표시를
  uppercase·hyphenless canonical로 하고, 선택적 hyphen은 payload를 바꾸지 않는
  `5-5-5-5-6` 표시 그룹이다.
- native는 OS CSPRNG, browser는 Web Crypto `getRandomValues`만 사용한다. 보안 난수가
  없으면 weak PRNG fallback, raw DOMException, OS error 또는 panic을 표시하지 않는다.
- 결과는 component memory에만 두고 자동 저장·전송하지 않는다. 사용자가 명시적으로
  Copy 또는 output context-menu의 Save를 선택한 경우에만 외부 action을 수행한다.
- HMAC/JWT, pipeline/storage, 외부 generator 연결은 이 PR 범위가 아니다.

## Changes Made

### 1. Native bounded generator

Files:

- `apps/developer-toolbox/src-tauri/Cargo.toml`
- `Cargo.lock`
- `apps/developer-toolbox/src-tauri/src/commands/tools.rs`
- `apps/developer-toolbox/src-tauri/src/lib.rs`

`GenerateIdsRequest`와 `generate_ids` Tauri command를 추가해 kind/count/case/hyphen 계약을
한 경계에서 검증한다. `Vec` capacity는 검증된 최대 100개로만 확보되며 kind나 사용자 입력을
오류 문자열에 다시 넣지 않는다.

`getrandom 0.4.3`을 직접 사용해 다음과 같이 고정 오류를 만든다.

```rust
fn secure_random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes)
        .map_err(|_| SECURE_RANDOM_ERROR.to_string())?;
    Ok(bytes)
}
```

UUID v4는 random bytes에 version/variant mask를 적용한 뒤 `Uuid::from_bytes`로 만든다.
UUID v7은 같은 순수 `generate_uuid_v7_from_parts`를 사용해 timestamp를 48-bit로 clamp하고
version/variant를 고정한 뒤, 반복·rollback에서는 74-bit suffix만 증가시킨다. 실제 command
호출은 `static Mutex<Option<[u8; 16]>>`에 마지막 값을 저장하여 concurrent native 호출도
서로 덮어쓰지 않는다. mutex poisoning도 고정 sequence error로 처리한다.

ULID는 10바이트를 직접 OS CSPRNG에서 받아 timestamp와 합친다. `generate_ulid_from_parts`와
UUID v7 순수 helper는 clock/random acquisition과 분리되어 deterministic boundary fixture가
가능하며, suffix overflow에서 timestamp를 1ms 전진하거나 48-bit 상한에서 fail closed 한다.
`encode_ulid`는 128-bit payload를 26개의 5-bit Crockford 문자로 변환하고 all-zero,
all-max 및 published vector를 확인한다.

기존 `generate_uuid` command도 새 bounded v4 경계를 호출해 `Result<String, String>`을
반환하도록 바꿨다. 따라서 이전 invoke API의 성공 payload는 유지하면서 legacy command에
남아 있던 raw CSPRNG panic 경로를 제거했다.

### 2. Browser fallback and API boundary

Files:

- `apps/developer-toolbox/src/tools/ids.ts`
- `apps/developer-toolbox/src/tools/ids.test.ts`
- `apps/developer-toolbox/src/api.ts`

Browser fallback은 Web Crypto를 직접 호출하고 lookup, feature detection, invocation을 모두
하나의 `try/catch` 안에 둔다. getter·없는 API·`getRandomValues` 예외 모두 fixed
`SECURE_RANDOM_ERROR`가 되며 플랫폼 exception text를 반향하지 않는다. UUID v4/v7 bit
layout과 ULID encoding/formatting은 native contract와 동일하게 유지한다.

`generateIds`는 Tauri에서는 `generate_ids`에 request DTO를 전달하고, browser에서는 동일한
순수 generator를 호출한다. 기존 `generateUuid`의 browser 경로도 `crypto.randomUUID()`를
직접 호출하지 않고 새 generator를 사용하므로 legacy API도 동일한 CSPRNG failure contract를
공유한다.

추가한 TS fixture는 다음을 결정적으로 검증한다.

- fixed Web Crypto bytes에서 UUID v4 version/variant 적용
- UUID v7과 ULID의 동일·뒤로 이동한 clock에서도 strict order 유지
- timestamp가 48-bit 상한이고 variable suffix가 all-ones일 때 fixed exhaustion error
- all-zero/all-max 및 published ULID encoding vector
- CSPRNG failure mapping, count 0/101과 잘못된 표시 옵션 거부

### 3. Async-safe and accessible UI

Files:

- `apps/developer-toolbox/src/tools/security.tsx`
- `apps/developer-toolbox/src/tools/security.test.tsx`
- `apps/developer-toolbox/src/App.css`
- `apps/developer-toolbox/src/tools/index.tsx`

`UuidTool`은 kind/count/case/hyphen control, 100개 상한, in-memory output, visible Copy와
기존 output Save/context-menu를 제공한다. 요청 중에는 모든 option을 native `disabled`로
잠그고 `aria-busy`, live `role=status`, 버튼 상태를 갱신한다.

각 request에 sequence id를 부여하고 option 변경·unmount에서 무효화한다. 늦은 success/error와
finally는 현재 request일 때만 결과·오류·busy 상태를 바꾸므로 stale IPC/browser promise가
새 선택 상태를 덮지 않는다. 동기 `runningRef`도 함께 검사해 React state commit 전의 중복
submit이 두 번째 IPC를 시작하지 못하게 한다. 수량 input의 composition 시작 중에는 submit을
막아 IME가 만드는 중간 값을 전송하지 않는다. count 오류와 생성 오류는 별도의 `role=alert`로
표시하고 native 오류 문자열은 항상 generic `IDENTIFIER_GENERATION_ERROR`로 치환한다.

UI fixture는 pending 중 option disabled/aria-busy/status, IME composition submit 방지, unmount
뒤 late response 무시, raw native/browser error 비반향과 최종 result status를 확인한다.

### 4. Documentation synchronization

Files:

- `apps/developer-toolbox/README.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
- `THIRD_PARTY_NOTICES.md`
- `workthrough/2026-08-26-developer-toolbox-uuid-ulid.md`

README에는 UUID/ULID 형식·ordering scope·CSPRNG/no-fallback·in-memory privacy 경계를
기록하고 Security 구현 목록에 `getrandom`을 반영했다. v0.5.0 native-first plan의 P2-03
계약도 실제 구현의 mutex-backed native v7 state와 direct OS CSPRNG를 명시하도록 맞췄다.
`getrandom 0.4.3`은 이미 lock/notices의 transitive package로 존재하므로 새 notice row는
필요하지 않다. Cargo lock에는 Developer Toolbox의 직접 dependency edge만 추가됐고,
generated notice inventory의 `Cargo.lock` SHA-256을 재생성해 dependency policy와 일치시켰다.

## PR-Boundary Review Findings

1. **Legacy panic path**: 기존 `generate_uuid() -> String`은 `uuid::new_v4()` panic을
   그대로 노출할 수 있었다. 새 bounded generator와 fixed `Result` 경계를 공유하게 했다.
2. **Panic hook/raw platform detail**: 단순 `catch_unwind`는 panic hook의 OS CSPRNG text를
   stderr에 남길 수 있고 전역 hook 임시 교체는 concurrent command에 안전하지 않다. 직접
   `getrandom::fill` 결과를 매핑해 panic 자체를 제거했다.
3. **Process ordering race**: batch-local previous만으로는 동시에 들어온 native 요청의
   ordering contract를 설명할 수 없었다. UUID v7만 process-local mutex state를 추가하고,
   ULID는 plan대로 batch-local 순서만 약속했다.
4. **Variable-bit corruption risk**: UUID v7 increment가 version/variant를 변경하지 않도록
   bytes 15–9, variant 하위 6bit, byte 7, version 하위 4bit 순으로 carry를 전달한다.
   overflow 후 timestamp를 올릴 때도 새 random value의 version/variant를 이미 고정한다.
5. **ULID boundary ambiguity**: timestamp와 80-bit suffix를 순수 helper로 분리해 rollback,
   timestamp 전진, max-bound exhaustion을 deterministic fixture로 만들었다.
6. **CSPRNG API lookup leak**: browser crypto getter/type check가 try 바깥이면 unusual
   platform object가 raw error를 던질 수 있었다. lookup부터 invocation까지 fixed error
   boundary 안으로 이동했다.
7. **Stale UI/unmount**: pending promise의 late success/error가 이전 view를 갱신할 수 있었다.
   request sequence와 unmount cleanup을 결합하고 options lock, generic error, live status를
   추가했다.
8. **IME/accessibility**: count control의 composition 중 submit을 차단하고 모든 option에
   help/error association, busy state, status announcement를 추가했다.
9. **불필요한 UUID feature**: 식별자는 직접 CSPRNG bytes에 RFC bit를 적용하고
   `Uuid::from_bytes`/`parse_str`만 사용한다. 사용하지 않는 `uuid`의 `v4`·`v7` Cargo
   feature를 제거해 dependency surface를 필요한 범위로 줄였다.
10. **동일 event 구간 중복 submit**: React의 disabled 렌더만으로는 상태 commit 전에 들어온
    두 번째 호출을 계약 수준에서 차단한다고 설명하기 부족했다. 동기 `runningRef` guard를
    추가해 request sequence가 배정되기 전부터 중복 IPC/browser generation을 거부하고,
    pending fixture에서 native 호출이 한 번뿐인지 검증했다.

## Verification Results

### Rust focused tests

```text
$ source ~/.cargo/env && cargo test -p developer-toolbox --lib -j 4
running 18 tests
...
test result: ok. 18 passed; 0 failed
```

The test set includes hash/regex/diff regression tests plus UUID v4/v7 shape and version/variant,
ULID canonical/grouped formatting, order within a batch, native process-call order, published and
boundary vectors, repeated/backward clock, suffix overflow and timestamp upper-bound failure.

```text
$ source ~/.cargo/env && cargo clippy -p developer-toolbox --all-targets -j 4 -- -D warnings
Finished `dev` profile [unoptimized + debuginfo]
exit 0
```

`cargo fmt --package developer-toolbox` was applied. `git diff --check` was clean at the final
focused review checkpoint.

### Frontend focused checks

`/mnt/e` worktree에서 별도 dependency install이나 watch process를 만들지 않고, 기존 Linux
native 검증 mirror에 Developer Toolbox tree만 동기화해 다음 검사를 실행했다.

```text
$ pnpm --filter developer-toolbox test
Test Files  12 passed (12)
Tests       110 passed (110)

$ pnpm --filter developer-toolbox build
tsc && vite build
✓ 135 modules transformed
✓ built
```

`security.test.tsx`의 pending option lock, IME, unmount stale response, raw error 비반향 fixture와
`ids.test.ts`의 format/order/CSPRNG/bound fixture를 포함한 전체 앱 frontend 회귀가 통과했다.

### Remaining checkpoint

- Windows W2 packaged native command/browser preview smoke is still required: UUID v4/v7 and ULID
  format, count bounds, CSPRNG/error surface, accessibility, copy/save, and batch ordering.

### Repository-wide PR gate after latest `main` rebase

The exact rebased source was synchronized into the existing disposable Linux-native frontend
mirror, preserving its cached `node_modules`, and the complete PR gate passed:

```text
cargo test --workspace -j4                         PASS
cargo check --workspace -j4                        PASS
cargo clippy --workspace --all-targets -j4 -- -D warnings  PASS
cargo fmt --all -- --check                         PASS
pnpm test  (17 frontend projects)                   PASS
pnpm build (17 frontend projects)                   PASS
git diff --check                                    PASS
dependency policy / regression / build-manifest     PASS
catalog consistency                                 PASS
```

Developer Toolbox remained at 12/12 frontend test files and 110/110 tests, including the added
duplicate-submit assertion. Repository-wide Rust tests, doctests, compilation, lint, formatting,
frontend regression tests, and production builds all completed without a failure. Windows W2
packaged smoke remains the release checkpoint because Tauri packaging is Windows-only.

## Resource Discipline

Only the requested feature worktree와 disposable native 검증 mirror가 수정됐다. Rust checks는
최대 네 cargo job과 기존 native target directory를 사용했다. 새 dependency install, full
workspace build, watch process 또는 background task는 남기지 않았다.
