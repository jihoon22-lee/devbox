# Developer Toolbox HMAC 생성·검증 (#288)

## Overview

Developer Toolbox에 HMAC 생성·검증 도구를 추가하는 P2-03 단일 기능 초안을 구현했다.
기능은 인터넷, 외부 generator, sidecar, secret storage 없이 동작하며, native 경로와
브라우저 미리보기가 같은 algorithm/encoding/bounds 계약을 사용한다. 키와 message는 한
작업의 메모리와 현재 화면에만 존재하고, verify 명령은 tag를 다시 반환하지 않고
`boolean`만 반환한다.

이 작업은 issue #288의 범위만 다룬다. JWT signature verify, secret persistence,
pipeline/handoff, telemetry, 자동 clipboard/save는 추가하지 않았다. 준비 branch는
`feat/developer-toolbox/hmac`이고 PR 전 최종 기준은 `main` `4df7321`이다.

## Context and decisions

기존 Toolbox에는 일반 hash와 UUID/ULID만 있었고, HMAC를 별도 설치·온라인 서비스 없이
반복적으로 사용할 수 있는 native-first 도구가 없었다. HMAC는 직접 암호 primitive를
만들면 안 되는 기능이므로 RustCrypto의 검증된 `hmac` 구현과 브라우저 Web Crypto를
선택했다. Base64/hex는 암호 primitive가 아닌 wire codec으로만 처리한다.

### Exact wire contract

- `algorithm`: exact lower-case `sha256`, `sha384`, `sha512`만 허용한다.
- Generate request:

  ```json
  {
    "algorithm": "sha256",
    "key": "...",
    "keyEncoding": "utf8",
    "message": "...",
    "messageEncoding": "utf8",
    "outputEncoding": "hex"
  }
  ```

- Verify request는 위 필드에 `expectedTag`를 더한다. Tauri 명령은
  `hmac_generate`와 `hmac_verify`로 분리하며 각각 encoded tag 문자열과 `boolean`을
  반환한다. request DTO는 `deny_unknown_fields`로 고정한다.
- key/message input encoding은 `utf8`, `hex`, padded RFC 4648 `base64`, unpadded
  RFC 4648 `base64url`이다. 결과 encoding은 lowercase hex, padded Base64,
  unpadded Base64URL이다.
- Hex 입력은 대·소문자를 허용한다. Base64는 alphabet, padding, unused pad bit까지
  재인코딩 결과와 일치하는 canonical 표현만 허용한다. Base64URL에는 `=` padding을
  허용하지 않는다.
- decoded key/message는 각각 1,000,000바이트, encoded text는 2,100,000바이트,
  expected/result tag는 128자까지다. key는 non-empty이고 empty message는 허용한다.
  올바른 encoding이지만 tag 길이 또는 값이 다르면 `false`; malformed encoding은
  고정 오류다.
- Rust와 TypeScript 모두 malformed/unknown/oversized input 및 primitive/DOM 예외를
  `HMAC 입력을 처리할 수 없습니다.` 하나로 매핑한다. 오류에 raw input, key,
  credential, path, platform detail을 포함하지 않는다.

## Changes made

### 1. Bounded native core and Tauri commands

Files:

- `apps/developer-toolbox/src-tauri/src/core/mod.rs`
- `apps/developer-toolbox/src-tauri/src/core/hmac.rs`
- `apps/developer-toolbox/src-tauri/src/commands/tools.rs`
- `apps/developer-toolbox/src-tauri/src/lib.rs`

`core/hmac.rs`는 parsing/decoding/encoding, 상한, algorithm dispatch를 순수 로직으로
소유한다. `Hmac<Sha256>`, `Hmac<Sha384>`, `Hmac<Sha512>`는 RustCrypto `hmac`의
표준 구현이고, verify는 `Mac::verify_slice`를 호출해 문자열의 일반 비교를 하지 않는다.

```rust
fn verify_with_digest<D: EagerHash>(
    key: &[u8],
    message: &[u8],
    expected: &[u8],
) -> Result<bool, String> {
    let mut mac = Hmac::<D>::new_from_slice(key).map_err(|_| fixed_error())?;
    mac.update(message);
    Ok(mac.verify_slice(expected).is_ok())
}
```

`commands/tools.rs`는 `hmac_generate(HmacRequest)`와
`hmac_verify(HmacVerifyRequest)`만 공개하고, `lib.rs`의 invoke handler에 두 명령을
등록했다. request는 `Debug`/`Serialize`를 derive하지 않아 key가 일반 debug/log
경로로 포맷될 표면도 줄였다. 명령과 core에는 filesystem, localStorage, network,
telemetry, clipboard handle을 전달하지 않는다.

### 2. Browser parity and API

Files:

- `apps/developer-toolbox/src/tools/hmac.ts`
- `apps/developer-toolbox/src/api.ts`

`hmac.ts`는 native contract와 같은 strict decoder/encoder와 bounds를 제공한다. Web
Crypto의 `subtle.importKey`, `sign`, `verify`가 실제 HMAC를 수행하고, codec만 앱 코드에
있다. 브라우저 경로에서 `crypto` 예외도 고정 오류로 변환한다.

`api.ts`는 Tauri가 없을 때 browser helper를 사용하고, Tauri 환경에서는 위 두 명령에
`{ request }`를 전달한다. verify API의 반환 타입은 `boolean`이며 계산된 tag는 IPC
응답에 포함하지 않는다.

### 3. UI, accessibility, and async boundaries

Files:

- `apps/developer-toolbox/src/tools/HmacTool.tsx`
- `apps/developer-toolbox/src/tools/HmacTool.test.tsx`
- `apps/developer-toolbox/src/tools/hmac.test.ts`
- `apps/developer-toolbox/src/tools/index.tsx`
- `apps/developer-toolbox/src/App.css`

`HmacTool`은 Generate/Verify, 알고리즘, key/message input encoding, output encoding을
명시적으로 선택한다. key/message/expected tag는 공통 Toolbox context menu가 있는
controlled input이고, 결과 Copy/Save는 기존 output surface에서 사용자 명시 action일
때만 수행한다. key·message는 history/localStorage/state persistence로 옮기지 않는다.

실행 중 모든 selector/input과 action button을 잠그고 `runningRef`로 double action을
차단한다. request sequence와 unmount cleanup으로 늦은 native/browser 응답이 새
화면을 덮지 못하게 하며, key/message/tag의 IME composition 중에는 submit하지 않는다.
`aria-busy`, labeled controls, `role=status`, `role=alert`, help note를 제공한다.

Fixture는 다음을 포함한다.

- explicit wire request와 output의 generate 결과
- pending 중 field lock과 double action 무시
- IME composition 중 submit 억제
- unmount/remount 뒤 stale result 억제
- verify에서 validity message만 표시하고 Copy 버튼을 추가하지 않음
- native/browser 오류 원문을 노출하지 않는 fixed error
- UTF-8/hex/Base64/Base64URL parity, canonical padding/pad bit rejection
- RFC 4231 SHA-256 known vector와 browser Web Crypto adapter 호출

PR 전 전체 검토에서 native DTO의 `deny_unknown_fields`와 browser preview의 runtime 계약을
맞추기 위해 generate/verify 요청의 exact own-key 집합을 검사하도록 보강했다. decoded 입력의
정확한 1,000,000바이트 경계와 초과값, Web Crypto `verify` 호출·boolean-only 반환 fixture도
추가했다. 이 과정에서 현재 stable Clippy가 지적한 수동 짝수 검사도
`usize::is_multiple_of`로 바꿔 `-D warnings`를 만족시켰다.
첫 수동 full CI의 Rust 1.98 Clippy가 고정 길이 `chunks_exact(2)`에 새 lint를 적용한 뒤에는
이미 검증한 짝수 길이 byte slice를 `as_chunks::<2>()`로 순회하도록 맞췄다. decoding
결과·상한·오류 계약은 바뀌지 않으며 local stable과 CI stable 모두 경고 없이 컴파일한다.

### 4. Documentation and dependency inventory

Files:

- `apps/developer-toolbox/README.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
- `THIRD_PARTY_NOTICES.md`

README와 roadmap에는 supported algorithms, exact encodings, bounds, offline/privacy,
constant-time verify, explicit output action, 비범위와 W2 남은 검증을 기록했다. native-first
계획의 §1.3 및 P2-03에도 같은 DTO/return/bounds/error 계약과 fixture/packaged 검증
항목을 반영했다.

이번 기능의 direct dependency는 다음과 같다.

| 항목 | 결정 및 근거 |
|---|---|
| Purpose | Rust native에서 SHA-256/384/512 HMAC 생성 및 constant-time verify 제공 |
| Alternatives | 자체 HMAC/비교 구현은 cryptographic risk 때문에 금지. 외부 CLI/온라인 generator는 오프라인·설치 경계를 깨므로 사용하지 않음. Web Crypto만으로 native command를 대체할 수 없어 RustCrypto를 병행 |
| Source | [`hmac` official repository](https://github.com/RustCrypto/MACs), [`hmac 0.13.0 registry`](https://crates.io/crates/hmac/0.13.0), [`base64` official repository](https://github.com/marshallpierce/rust-base64) |
| Pin | `hmac = "0.13.0"`, `base64 = "0.22.1"`; lock에는 `hmac 0.13.0`, `cmov 0.5.4`, `ctutils 0.4.2`와 checksum이 기록됨 |
| License | `hmac` MIT OR Apache-2.0, `base64` MIT OR Apache-2.0; generated notices에 direct/transitive rows를 추가함 |
| Size | 별도 실행 파일·network runtime·sidecar를 추가하지 않음. exact packaged binary/bundle 증분은 Windows W2 release checkpoint에서 측정할 항목으로 남김 |
| Security | strict alphabet/padding/pad-bit, input/result bounds, no secret Debug/log/persistence, fixed error, RustCrypto/Web Crypto verify 경계. `cargo deny`/advisory full gate는 merge 전 CI에서 재확인 |
| Offline | 모든 계산은 설치물에 번들된 Rust/Web Crypto primitive로 수행하며 runtime download나 외부 API 없음 |
| Maintenance | RustCrypto MACs release/advisory와 Cargo lock/notices를 Dependabot/dependency gate에서 추적. 호환 primitive가 제공되면 고정 버전을 재검토하고, 문제 시 HMAC PR 전체를 rollback |

## Verification results

### Rust focused test

```text
source ~/.cargo/env && cargo test --manifest-path apps/developer-toolbox/src-tauri/Cargo.toml --lib core::hmac -j1
running 6 tests
test core::hmac::tests::rejects_malformed_wire_values_with_one_fixed_error ... ok
test core::hmac::tests::serde_contract_rejects_unknown_fields ... ok
test core::hmac::tests::matches_rfc_4231_vectors_for_all_supported_algorithms ... ok
test core::hmac::tests::supports_key_message_and_output_encodings ... ok
test core::hmac::tests::verification_is_true_for_matching_tag_and_false_for_mismatch ... ok
test core::hmac::tests::enforces_non_empty_key_and_input_limits_without_reflecting_input ... ok
test result: ok. 6 passed; 0 failed
```

`cargo fmt`와 app-level `cargo clippy --all-targets -- -D warnings`도 통과했다.
`cargo metadata --locked --no-deps`와
`python3 .github/scripts/check-dependencies.py generate`도 성공해 lockfile 및 generated
notice가 일치한다.

### Frontend focused checks

작업 worktree에 별도 install을 하지 않고 Linux native temporary mirror에 기존
workspace package를 연결해 실행했다. mirror는 각 실행 뒤 파일·symlink·빈 디렉터리를
삭제했다.

```text
vitest run src/tools/hmac.test.ts --environment node --pool=threads --maxWorkers=1
Test Files  1 passed (1)
Tests       5 passed (5)

vitest run src/tools/HmacTool.test.tsx --pool=threads --maxWorkers=1
Test Files  1 passed (1)
Tests       6 passed (6)

tsc --strict ... HmacTool.tsx hmac.ts common.tsx ids.ts api.ts types.ts ...
Exit code: 0
```

최종 `main` rebase와 위 보강 뒤 Linux-native mirror에서 다음 PR 전 전체 gate가 통과했다.

- `pnpm test`: 724 tests passed. Developer Toolbox는 16 files, 136 tests다.
- `pnpm build`: 17개 참여 workspace project가 모두 통과했다.
- `cargo test --workspace -j2`
- `cargo check --workspace -j2`
- `cargo clippy --workspace --all-targets -j2 -- -D warnings`
- `cargo fmt --all -- --check`
- dependency policy/notices, dependency regression, build-manifest notice regression
- catalog consistency와 `git diff --check`

Windows Tauri packaged build/W2 smoke와 CI의 `pnpm audit`·`cargo deny`는 플랫폼/PR
checkpoint로 남긴다.

## Remaining risks and follow-up

- Native/browser codec 구현은 동일한 contract를 의도적으로 각각 보유하므로 향후 공통
  fixture를 더 확장해 parity drift를 방지해야 한다.
- key/message를 persistence/log에서 제외했지만, 일반 프로세스 메모리에서 작업 후
  zeroization을 보장하는 기능은 이 PR에 추가하지 않았다. 필요하면 별도 보안 설계와
  측정으로 다룬다.
- Windows W2에서 packaged native/browser 결과, output encoding, error redaction,
  accessibility와 명시적 copy/save 외 filesystem 불변을 확인해야 한다.
- `hmac`의 license/notice와 전체 workspace 로컬 gate는 통과했다. advisory audit과 Windows
  compile/test는 PR CI에서, packaged 동작은 W2 release checkpoint에서 확인해야 한다.
