# API Playground Persistence Schema Sanitizer Fix

## Overview

`v0.4.2-rc1` Windows H1 검증에서 API Playground의 Collection v1→v2 migration이
안전한 Collection v2를 저장하지 못하고 fail-closed로 중단되는 문제를 수정했다.
backend persistence sanitizer가 `requiresSecretReview`라는 스키마 메타데이터까지
민감 필드로 판단해 boolean을 `[REDACTED]` 문자열로 바꾸고 있었기 때문이다. 정확한
boolean wire shape만 보존하도록 경계를 보정했으며, 이 수정이 반영된 RC2를 다시 패키징해
H1을 재검증하기 전에는 `v0.4.2` stable 승격을 진행하지 않는다.

## Context

- RC1 공식 Windows package는 DPAPI secret sealing, History migration, request/response
  redaction 및 redirect 경계를 검증하던 중 Collection migration에서 실패했다.
- Collection v2의 `requiresSecretReview`는 credential이 아니라 변환된 항목에 검토가
  필요함을 나타내는 boolean schema metadata다.
- `is_sensitive_name`은 `secret`을 포함한 이름을 민감 필드로 분류한다. 따라서
  `requiresSecretReview`를 별도 예외 없이 recursive JSON sanitizer에 통과시키면
  `true`/`false`가 문자열 `[REDACTED]`로 바뀐다.
- frontend `parseStore`는 손상된 v2 schema를 거부하므로 migration은 원자적 삭제 단계에
  도달하지 못하고 fail-closed로 재시도해야 한다. raw Collection 데이터가 논리적으로
  남은 상태이므로 stable release를 차단했다.

## Changes Made

### Boolean-only schema metadata preservation

`apps/api-playground/src-tauri/src/commands/request.rs`의 persistence JSON sanitizer에서
정확히 `requiresSecretReview` 키이면서 값이 JSON boolean인 경우에만 원래 값을 보존한다.
문자열·숫자·object 등 같은 이름의 비boolean 값은 즉시 `[REDACTED]`로 마스킹된다.
실제 secret/password/token 필드의 redaction 범위는 완화하지
않았다.

```rust
if key == "requiresSecretReview" {
    if value.is_boolean() {
        return;
    }
    *value = serde_json::Value::String(REDACTED.to_string());
    return;
}
if is_sensitive_name(key) {
    *value = serde_json::Value::String(REDACTED.to_string());
    return;
}
```

회귀 테스트는 다음을 함께 보장한다.

- top-level 및 nested `requiresSecretReview: true/false`가 boolean으로 유지된다.
- 같은 이름의 문자열·숫자 값은 `[REDACTED]`가 된다.
- History와 Collection v2 wire shape가 backend sanitization 뒤에도 parse 가능한 상태로 유지된다.
- Authorization, password, token 및 environment secret literal은 계속 제거된다.

### Documentation

- `apps/api-playground/README.md`에 `requiresSecretReview` boolean metadata의 저장
  경계와 비boolean 값의 masking 규칙을 추가했다.
- 이 workthrough에 RC1 H1 발견 경위, 수정의 fail-closed 의도, 검증 및 RC2 후속 절차를
  기록했다.

## Verification Results

수정 branch의 API Playground Rust test suite는 다음 결과를 확인한다.

```text
API Playground Rust tests    16 passed / 0 failed
```

추가된 sanitizer 회귀 테스트는 boolean-only 보존과 Collection v2 wire-shape 보존을
검증하며, 기존 secret literal redaction 테스트도 함께 통과해야 한다. RC1 H1에서
확인된 migration failure는 제품 결함으로 분류했으며, 이 문서 작성 시점의 release
판정은 다음과 같다.

- `v0.4.2-rc1`: Collection migration H1 실패
- `v0.4.2` stable: 차단
- 다음 단계: 수정사항을 포함한 `v0.4.2-rc2` 공식 Windows package를 빌드·배포하고
  Collection v1→v2, History, DPAPI, redaction 및 cleanup 시나리오를 H1에서 재검증
- RC2 H1이 통과한 증거를 확인한 뒤에만 stable tag 승격을 재개

## Next Steps

- PR 전 집중 검토에서 sanitizer 예외가 boolean wire shape에만 적용되고 실제 민감값
  redaction을 약화하지 않는지 확인한다.
- PR CI와 전체 완료 정의(`cargo test`, `cargo check`, `pnpm build`)를 통과시킨다.
- RC2 packaged H1에서 Collection migration의 v2 read-back, raw v1 삭제 및 marker
  기록을 다시 확인하고, 실패 시 stable을 계속 차단한다.
