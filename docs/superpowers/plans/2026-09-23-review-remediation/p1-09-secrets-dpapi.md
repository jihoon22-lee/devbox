# P1-09 DPAPI 봉인기 하나로 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** 세 벌(`http-client-engine`, `projects-engine`, `runtime-engine`)로 흩어진 Windows DPAPI 봉인 구현을 `crates/secrets`의 `DpapiSealer` 하나로 모으고, 짧은 비밀을 통째로 보여 주는 `mask`를 고친다(S5, 개인용 범위).

**Architecture:** `secrets::dpapi::DpapiSealer::new(entropy: &'static [u8])`가 `Sealer` trait을 구현한다(CurrentUser, UI 금지, 용도별 entropy). 구현은 가장 조심스러운 `projects-engine` 버전(DPAPI 버퍼를 0으로 지운 뒤 `LocalFree`, UTF-8 실패 시 사본까지 zeroize)을 기준으로 하고, 빈 평문도 왕복되게 한다. 세 소비자는 **지금 쓰는 entropy 문자열을 그대로** 넘기므로 이미 저장된 암호문이 계속 풀린다.

**Tech Stack:** Rust, `windows` 0.61(`Win32_Security_Cryptography`), `zeroize`

**Spec:** `review.md` §4 S5 · `00-roadmap.md` §2 보안 조정(ADR 0016)

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- entropy 문자열 불변: `devbox.api-playground.secrets.v1`, `devbox.api-playground.grpc-tls-credentials.v1`, `devbox.workbench.project-environment.v1`, `devbox.run-manager.environment.v1`. 이름에 v0.7 앱 이름이 있어도 바꾸지 않는다(바꾸면 기존 비밀을 풀 수 없다). `check-no-legacy.py` 패턴은 이 문자열에 걸리지 않는다(`_lib` 이름·migration 식별자만 대상).
- Workspace 복구 버퍼 암호화, Webhook LAN 공유 비밀 헤더는 하지 않는다(ADR 0016).

## Review Focus

1. v0.8.x가 저장한 API 환경 변수·gRPC TLS 자료·프로젝트 환경 변수·Runtime 환경 비밀 → 업데이트 후 그대로 풀린다(entropy·envelope 불변). (Task 2, 사용자 실기)
2. 빈 문자열 비밀(값을 지운 환경 변수) → 봉인·해제가 성공하고 빈 문자열로 돌아온다. (Task 1)
3. 다른 용도의 entropy로 봉인한 blob을 해제 → `CryptoFailure`(용도 간 교차 사용 불가). (Task 1)
4. 짧은 비밀(`abc`)을 `mask(…, 3)` → 전부 보이지 않는다(`a**`). (Task 1)
5. 손상된 blob(마지막 바이트 변경) → `CryptoFailure`, panic 없음. (Task 1)

## Branch · PR

- 묶음: **B5** — 브랜치 `refactor/crates/shared-platform-crates`, PR 제목 `refactor(crates): share process-tree, DPAPI, WSL helper and Markdown preview`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `refactor(crates): share one DPAPI sealer`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: `secrets::dpapi`와 `mask`

**Files:** Create `crates/secrets/src/dpapi.rs`; Modify `crates/secrets/src/lib.rs`, `crates/secrets/Cargo.toml`

**Interfaces (Produces):** `secrets::dpapi::DpapiSealer`(`#[cfg(windows)]`), `DpapiSealer::new(entropy: &'static [u8]) -> Self`, `impl Sealer for DpapiSealer`; `secrets::mask(value, visible_chars)`(보이는 글자 수를 `min(visible_chars, 글자 수 / 3)`으로 제한)

- [ ] **Step 1: 실패하는 테스트**

`crates/secrets/src/lib.rs` 테스트 모듈:

```rust
    #[test]
    fn short_secrets_are_never_fully_visible() {
        assert_eq!(mask("abc", 3), "a**");
        assert_eq!(mask("ab", 5), "**");
        assert_eq!(mask("secret-token-value", 4), format!("secr{}", "*".repeat(12)));
        assert_eq!(mask("", 4), "");
        assert_eq!(mask("abcdef", 0), "******");
    }
```

`crates/secrets/src/dpapi.rs`의 테스트(Windows에서만 돈다):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{seal_v1, unseal_v1, Sealer};

    const A: &[u8] = b"devbox.test.a.v1";
    const B: &[u8] = b"devbox.test.b.v1";

    #[test]
    fn values_round_trip_including_empty_and_unicode() {
        let sealer = DpapiSealer::new(A);
        for value in ["", "value", "비밀 값 🔐"] {
            let blob = seal_v1(&sealer, value).unwrap();
            assert_eq!(unseal_v1(&sealer, &blob).unwrap().as_str(), value);
        }
    }

    #[test]
    fn another_purpose_cannot_unseal() {
        let blob = seal_v1(&DpapiSealer::new(A), "value").unwrap();
        assert!(matches!(unseal_v1(&DpapiSealer::new(B), &blob), Err(SealError::CryptoFailure)));
    }

    #[test]
    fn tampered_blobs_fail_closed() {
        let sealer = DpapiSealer::new(A);
        let mut blob = seal_v1(&sealer, "value").unwrap();
        *blob.last_mut().unwrap() ^= 0xff;
        assert!(matches!(unseal_v1(&sealer, &blob), Err(SealError::CryptoFailure)));
        assert!(matches!(sealer.unseal(&[]), Err(SealError::InvalidInput) | Err(SealError::CryptoFailure)));
    }
}
```

- [ ] **Step 2: 실패 확인** — Run: `source ~/.cargo/env && cargo test -p secrets --lib` → `short_secrets_are_never_fully_visible` FAIL(Windows 테스트는 이 호스트에서 컴파일되지 않는다).

- [ ] **Step 3: 구현**
  - `lib.rs`의 `mask`를 바꾼다.

```rust
/// 마스킹. 앞 몇 글자만 남기고 나머지를 `*`로 가린다. 짧은 값은 대부분을
/// 가리도록 보이는 글자를 전체의 1/3 이하로 제한한다.
pub fn mask(value: &str, visible_chars: usize) -> String {
    let count = value.chars().count();
    if count == 0 {
        return String::new();
    }
    let visible_count = visible_chars.min(count / 3);
    let visible: String = value.chars().take(visible_count).collect();
    let hidden = count - visible_count;
    format!("{visible}{}", "*".repeat(hidden.min(12)))
}
```

  - `lib.rs` 머리 주석의 "crates에 Windows 전용 코드를 금지하므로 … 각 앱의 platform 레이어가 `Sealer`로 구현한다" 문단을 "Windows DPAPI 구현은 `dpapi` 모듈 하나에 둔다. 소비자는 용도별 entropy만 정한다."로 바꾸고, 파일 끝에 `#[cfg(windows)] pub mod dpapi;`를 추가한다.
  - `Cargo.toml`에 `[target.'cfg(windows)'.dependencies] windows = { workspace = true }`를 추가한다(`Win32_Security_Cryptography` feature가 workspace `windows` feature 목록에 없으면 루트 목록에 추가한다).
  - `dpapi.rs`: `crates/projects-engine/src/platform.rs`의 `windows_impl` 모듈 본문(`blob`, `copy_and_free`, `copy_and_zeroize_free`, `seal`, `unseal`)을 옮기고 아래처럼 바꾼다.

```rust
//! Current-user DPAPI with a per-purpose entropy. Every DPAPI-owned buffer is
//! zeroed before `LocalFree`, and failed UTF-8 decodes zeroize their copies.
use crate::SealError;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};
use zeroize::{Zeroize, Zeroizing};

pub struct DpapiSealer {
    entropy: &'static [u8],
}

impl DpapiSealer {
    pub fn new(entropy: &'static [u8]) -> Self {
        Self { entropy }
    }
}

impl crate::Sealer for DpapiSealer {
    fn seal(&self, plaintext: &str) -> Result<Vec<u8>, SealError> {
        // (projects-engine seal 본문을 그대로, ENTROPY 대신 self.entropy)
    }

    fn unseal(&self, ciphertext: &[u8]) -> Result<Zeroizing<String>, SealError> {
        if ciphertext.is_empty() {
            return Err(SealError::InvalidInput);
        }
        // (projects-engine unseal 본문을 그대로, ENTROPY 대신 self.entropy)
        // 단, 빈 평문을 허용한다: DPAPI 출력이 0바이트면 Ok(Zeroizing::new(String::new())).
    }
}
```

  빈 평문 허용을 위해 `copy_and_zeroize_free`를 "`cbData == 0`이면 `pbData`가 null이 아닐 때만 `LocalFree`하고 빈 `Zeroizing<Vec<u8>>`를 돌려준다"로 바꾼다(`seal` 쪽 `copy_and_free`는 빈 암호문을 계속 오류로 본다).

- [ ] **Step 4: 통과 확인** — Run: `cargo test -p secrets --lib && cargo check --target x86_64-pc-windows-msvc -p secrets` → PASS(두 번째 명령이 build script 없이 끝나므로 이 crate는 Linux에서 Windows target check가 된다. Windows 테스트 실행은 CI `Rust (Windows)`).

- [ ] **Step 5: 커밋** — `git add crates/secrets Cargo.toml && git commit -m "feat(crates): add a shared DPAPI sealer and safer masking"`

---

### Task 2: 소비자 옮기기

**Files:** `crates/http-client-engine/src/platform.rs`, `crates/projects-engine/src/platform.rs`, `crates/runtime-engine/src/platform/environment.rs`

- [ ] **Step 1: 테스트 기대값 고정** — `projects-engine/src/core/environment.rs`에서 `mask(…, 2)`를 쓰는 표시 결과를 확인하는 테스트가 있으면 새 규칙(`min(2, len/3)`)에 맞게 기대값을 고친다(`rg -n "mask\(" crates/projects-engine/src` → 테스트에서 기대 문자열 확인).
- [ ] **Step 2: 교체**
  - http-client-engine: `windows_impl` 모듈을 지우고 `DpapiSealer::environment()`/`grpc_tls()` 생성자가 있던 자리에서 `devbox_secrets::dpapi::DpapiSealer::new(ENVIRONMENT_ENTROPY)` / `new(GRPC_TLS_ENTROPY)`를 쓴다. 두 entropy 상수는 이 파일에 남긴다(`#[cfg(windows)]`).
  - projects-engine: `windows_impl` 모듈을 지우고 `platform_sealer()`의 Windows 분기를 `Box::new(devbox_secrets::dpapi::DpapiSealer::new(b"devbox.workbench.project-environment.v1"))`로 바꾼다(상수로 두고 주석은 유지).
  - runtime-engine: `DpapiProtector`의 `seal`/`unseal`과 내부 `blob`·호출 함수를 지우고 `DpapiSealer::new(OPTIONAL_ENTROPY)`에 위임한다. `DpapiProtector` 타입이 다른 곳에서 쓰이면 `pub(crate) type DpapiProtector = devbox_secrets::dpapi::DpapiSealer;`와 생성 함수로 호환을 맞춘다.
  - 각 crate `Cargo.toml`에서 DPAPI 때문에만 켜던 `windows` feature(`Win32_Security_Cryptography`)가 더 필요 없으면 목록에서 뺀다(P1-06 이후에는 workspace 합집합이므로 member 쪽 변경은 없다).
- [ ] **Step 3: 확인** — Run: `cargo test -p devbox-http-client-engine -p devbox-projects-engine -p devbox-runtime-engine --lib && ! rg -n "CryptProtectData" crates --glob '!crates/secrets/**'` → PASS(마지막 명령 0건).
- [ ] **Step 4: 커밋** — `git add -A && git commit -m "refactor(crates): use the shared DPAPI sealer everywhere"`

---

### Task 3: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9. Windows 테스트는 CI `Rust (Windows)` 결과를 근거로 적는다.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): v0.8.x에서 저장한 API Studio 환경 비밀·Workspace 프로젝트 환경 비밀·작업의 비밀 환경 변수가 P3-01 후보 설치본에서 그대로 쓰인다(요청 전송·작업 실행 성공).
