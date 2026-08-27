# Developer Toolbox JWT Decode·Verify

## Overview

Issue #289의 JWT 기능을 Developer Toolbox에 추가했다. compact JWT를 오프라인에서
bounded/fail-closed 방식으로 decode해 header와 payload를 `unverified` 상태로 표시하고,
사용자가 별도로 Verify를 눌렀을 때만 HS256/HS384/HS512 HMAC 서명을 확인한다. 브라우저
미리보기는 Web Crypto, packaged Tauri 앱은 RustCrypto HMAC을 사용하며 두 경로 모두
secret/token을 저장하거나 로그·network·telemetry로 보내지 않는다.

작업 중 `origin/main`이 HMAC #424(`2c20eb5`) 병합으로 이동해 초안을 안전하게 stash한 뒤
최신 main에 rebase했다. HMAC의 `core/mod.rs`, command/API/UI, Cargo lock과 third-party
notice를 보존하고 JWT 모듈을 추가하는 방식으로 충돌을 해결했다.

## Context and decisions

- JWT decoding은 signature 검증과 분리한다. decode 성공은 인증 성공을 의미하지 않으며,
  출력 상태는 항상 `unverified`로 시작한다.
- 이 PR의 알고리즘은 exact `HS256`, `HS384`, `HS512`뿐이다. `none`, casing 변형,
  RS/ES와 기타 미래 알고리즘은 거부한다.
- key encoding은 raw UTF-8, hex, padded standard Base64, unpadded Base64URL로 고정한다.
  decoded key는 각 SHA digest 길이(32/48/64 bytes) 이상이어야 한다.
- PEM/JWK parser, RSA/EC public-key primitive, token persistence/history, automatic
  clipboard, network/external JWT tool 연결은 범위에 넣지 않았다. asymmetric key가
  HMAC secret으로 묵시적으로 fallback되지 않도록 unknown key encoding을 fail-closed한다.
- 수제 HMAC/RSA/EC/DER 구현은 하지 않고, packaged 경로는 RustCrypto `hmac`의
  `Mac::verify_slice`, browser 경로는 Web Crypto `subtle.verify`를 사용한다. verify
  command의 반환값은 `boolean` 또는 fixed error뿐이다.

## Changes made

### 1. Bounded native verification boundary

Files:

- `apps/developer-toolbox/src-tauri/src/core/jwt.rs`
- `apps/developer-toolbox/src-tauri/src/core/mod.rs`
- `apps/developer-toolbox/src-tauri/src/commands/tools.rs`
- `apps/developer-toolbox/src-tauri/src/lib.rs`

`JwtVerifyRequest`는 `serde(rename_all = "camelCase", deny_unknown_fields)`를 사용하는
strict DTO다. `key`, `signature`, `signing_input`에는 Debug/Serialize를 구현하지 않아
command 경계에서 secret-bearing 값을 실수로 formatting/serialization할 수 없게 했다.

native boundary는 다음을 다시 검사한다.

- compact signing input 최대 256 KiB, header/payload segment 각각 96 KiB
- 각 segment와 signature의 unpadded canonical Base64URL, non-zero pad bit 및 `=` 거부
- header의 JSON UTF-8/구문, duplicate key, `alg` exact match, `typ`/`kid`/`cty` 타입
- payload의 JSON UTF-8/구문 및 64 KiB, depth 32, 10,000 value/key node, 문자열 16 KiB
  상한
- `crit` 최대 8개, duplicate/missing/unknown critical name 거부
- HS256/384/512 정확한 signature 길이 및 encoded key 2,100,000 bytes / decoded key
  1,000,000 bytes / digest별 최소 key 길이

Rust custom serde visitor는 JSON을 보존하거나 반환하지 않고 bounds/duplicate만 검사한다.
오류는 모두 `JWT_VERIFY_ERROR` 하나로 매핑되고 token/key/signature/platform parser
세부사항을 반영하지 않는다. primitive 호출 결과 외에는 key나 calculated tag가 command
밖으로 나오지 않는다.

핵심 wire contract:

```text
JwtVerifyRequest {
  algorithm: "HS256" | "HS384" | "HS512",
  signingInput: string,
  signature: string,
  key: string,
  keyEncoding: "utf8" | "hex" | "base64" | "base64url"
}
-> Result<boolean, fixed_error>
```

### 2. Strict browser parser and crypto fallback

Files:

- `apps/developer-toolbox/src/tools/jwt.ts`
- `apps/developer-toolbox/src/api.ts`
- `apps/developer-toolbox/src/tools/transformers.tsx`
- `apps/developer-toolbox/src/tools/transformers.test.ts`
- `apps/developer-toolbox/src/tools/jwt.test.ts`

`jwt.ts`는 `jsonc-parser` tree를 사용하되 comments/trailing comma를 허용하지 않고,
null-prototype object와 `Object.defineProperty`로 값을 재구성해 `__proto__` 같은 key가
prototype pollution 경로가 되지 않도록 했다. duplicate JSON key, invalid UTF-8,
unsafe integer/non-finite number, depth/node/string/output bounds는 고정 `JwtError`로
중단한다. Base64URL decoder는 alphabet, length modulo, padding, unused pad bit와
canonical re-encoding을 확인한다.

`parseJwt`는 정확히 세 segment와 allow-listed protected-header `alg`를 요구하고,
signature를 검증하지 않은 상태에서 표시할 수 있는 구조화 결과만 만든다. `formatJwtDisplay`
는 compact signature와 key를 결과 JSON에 넣지 않으며, optional verification timestamp도
유한한 bounded UTC epoch일 때만 ISO-8601로 formatting한다.

시간 claim은 payload object의 `exp`/`nbf`/`iat`를 raw NumericDate와 UTC ISO-8601로
표시한다. Verify 시작 때 epoch seconds를 한 번 캡처하고 ±60초 고정 skew를 적용한다.
malformed/out-of-range claim은 crypto 호출 전에 `invalid_claims`가 되고, signature와
시간 claim을 모두 통과한 경우에만 `verified`가 된다.

`browserVerifyJwt`는 native request와 같은 key/segment/signature bounds를 적용한 뒤
Web Crypto HMAC `importKey`/`verify`를 호출하고 boolean만 반환한다. Tauri API는 같은
DTO를 `jwt_verify` command로 전달한다.

### 3. Explicit, accessible verification UI

Files:

- `apps/developer-toolbox/src/tools/JwtTool.tsx`
- `apps/developer-toolbox/src/tools/JwtTool.test.tsx`
- `apps/developer-toolbox/src/tools/index.tsx`
- `apps/developer-toolbox/src/tools/common.tsx`
- `apps/developer-toolbox/src/App.css`

새 `JwtDecoder`는 Decode와 Verify 버튼을 분리하고, password-type memory-only key field와
key encoding selector를 제공한다. Decode는 unverified output만 만들며, Verify는 explicit
action 이후에만 실행한다. 입력/결과에는 기존 app-owned context menu를 연결해 Paste,
select/clear, output copy/select/save를 사용자 선택으로만 제공한다.

`sequence`와 `runningRef`가 duplicate click, 늦은 browser/native promise, unmount 뒤 state
반영을 차단한다. decode 완료 후 verify가 영구적으로 막히던 상태 flag도 수정했고, custom
paste가 HTML `maxLength`를 우회하지 않도록 token/key code-unit bound를 handler에서 다시
확인했다. 실행 중에는 입력과 selector를 잠그며 IME composition을 실행 동작으로 취급하지
않는다.

`aria-label`, `aria-describedby`, `aria-busy`, `role=status`, `role=alert`와 keyboard
focus 가능한 output surface를 유지한다. primitive/OS/secret-bearing 오류는 화면에 raw
내용을 반향하지 않고 fixed message로 표시한다.

### 4. Documentation and integration

Files:

- `apps/developer-toolbox/README.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-15-ux-improvements-design.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
- this workthrough

Auth 도구의 HS algorithm/key format, unverified/verified 상태, temporal claim/skew,
privacy, bounds, native/browser parity, non-scope를 문서화했다. 계획 문서에는 strict wire
DTO, critical header, duplicate/JSON rules, exact PR boundary와 W2 검증 항목을 축약 없이
반영했다.

HMAC #424가 이미 main에 제공하는 `hmac 0.13.0`, `base64 0.22.1`, `cmov`, `ctutils`와
third-party notice/lock hash는 그대로 재사용했다. JWT 자체로 새 runtime dependency를
추가하지 않았다.

## Code examples

### Verification is explicit and status-only

```typescript
const valid = await verifyJwt({
  algorithm: parsed.algorithm,
  signingInput: parsed.signingInput,
  signature: parsed.signature,
  key,
  keyEncoding,
});

setStatus(valid ? "verified" : "invalid_signature");
```

The returned value is only a boolean. `formatJwtDisplay` includes verification status/time,
header, payload and temporal metadata; it never includes the compact signature or key.

### Native constant-time primitive

```rust
match algorithm {
    Algorithm::Hs256 => verify_with_digest::<Sha256>(key, signing_input, signature),
    Algorithm::Hs384 => verify_with_digest::<Sha384>(key, signing_input, signature),
    Algorithm::Hs512 => verify_with_digest::<Sha512>(key, signing_input, signature),
}
```

Each branch ends at RustCrypto `Mac::verify_slice`; malformed requests return the same fixed
error and a validly shaped but wrong tag returns `false`.

## Verification results

2026-08-27 grouped-PR root review에서 native/browser direct verification 경계에도 `exp`/`nbf`/
`iat` ±60초 검증을 추가했다. UI 입력은 UTF-16 code-unit가 아니라 UTF-8 byte 상한으로 제한하고
explicit paste·output context action에 고정 오류를 적용했다. #289 acceptance는 #290/#291/#292와
같은 Developer Toolbox PR에서 독립 fixture로 검증한다.

All commands were run in `/mnt/e/projects/devbox-worktrees/developer-toolbox-jwt` with the
Rust target cache on the Linux filesystem and bounded worker counts.

### Rust/native

```text
cargo fmt --manifest-path apps/developer-toolbox/src-tauri/Cargo.toml -- --check   PASS
cargo test --locked -p developer-toolbox --lib core::jwt                         PASS (8 tests)
cargo check --locked -p developer-toolbox                                         PASS
cargo clippy --locked -p developer-toolbox --lib -- -D warnings                  PASS
```

### Frontend

```text
node .../typescript/bin/tsc --noEmit                                               PASS
Vitest: 3 files, 49 tests                                                         PASS
Vite production build: 151 modules transformed                                      PASS
```

The Vitest run was serialized with one worker and no file parallelism. It covers known
HS256/384/512 vectors, algorithm confusion, canonical Base64URL, key formats/lengths, JSON
and UTF-8/critical/duplicate bounds, temporal skew, browser verification, explicit UI actions,
fixed error mapping, IME, duplicate action, stale/unmount, and input bound behavior.

### Dependency and diff checks

```text
python3 .github/scripts/check-dependencies.py check                               PASS
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml
git diff --check                                                                  PASS
```

## Remaining verification and risks

- Windows W2 packaged Tauri smoke is intentionally left to the root agent/CI. It must invoke
  the packaged `jwt_verify` command, confirm browser/native result parity, verify no token/key
  persistence or automatic clipboard activity, and exercise offline preview/a11y.
- Full workspace `cargo test`/`cargo check`, root `pnpm build`, CI and PR gate remain root-level
  integration work. This worktree used app-focused checks to conserve shared resources.
- PEM/JWK/RSA/EC support remains a deliberate future scope decision; adding it later requires a
  separate algorithm/key-type security review and must not widen this HMAC request implicitly.
- HMAC files are inherited from the already merged #424 main commit. If another local branch
  also contains the pre-merge HMAC patch, cherry-pick/rebase it with care to avoid duplicate
  Cargo/README/core registration rather than dropping either feature.
