# API Playground Secret Persistence Hardening

## Overview

API Playground 0.3.2에서 secret의 평문이 프론트엔드 상태, History·Collection
localStorage, cURL, redirect, 응답 또는 오류를 통해 남을 수 있는 경로를 닫았다. PR
#225는 secret 해석을 Rust 전송 경계로 옮기고 저장 형식을 v2로 이관했으며, 모든
필수 Linux·Windows CI가 통과한 뒤 2026-08-24에 merge됐다.

## Context

- 기존 프론트엔드는 봉인된 environment secret을 다시 해제해 요청을 구성할 수
  있었으므로 React 상태와 브라우저 저장 경계에 평문이 나타날 수 있었다.
- v1 History·Collection에는 direct credential이 저장될 수 있었고, 실패 시 raw
  payload를 다시 UI에 노출하지 않는 원자적 이관 규칙이 필요했다.
- HTTP redirect, 응답 header/body, URL, cURL 및 네트워크 오류도 secret을 되돌려
  줄 수 있어 요청 전송부터 표시·저장까지 하나의 redaction 경계가 필요했다.
- 보안 핫픽스는 API Playground patch version `0.3.2`로 배포하고, 공식 Windows
  패키지 H1 검증 뒤 `v0.4.2` 안정판으로 승격하도록 계획했다.

## Architecture Decisions

### Backend-only secret resolution

편집·저장 가능한 `RequestTemplate`, 직렬화되지 않는 `ResolvedRequest`, 저장 wire
형식인 `PersistedHistoryRequest`를 분리했다. `{{NAME}}`와 `${NAME}` 참조는 URL,
params, header key/value, body 및 모든 auth field에서 Rust가 전송 직전에만
해석한다. unseal 실패는 ciphertext fallback 없이 안전한 오류로 끝난다.

```rust
pub async fn send_request(
    req: RequestTemplate,
    environment: Vec<EnvironmentVariable>,
) -> Result<ApiResponse, String> {
    let sealer = platform_sealer();
    let (resolved, environment_secrets) =
        resolve_template(&req, &environment, sealer.as_ref())
            .map_err(|_| safe_secret_error())?;
    let redactor = Redactor::for_request(&resolved, environment_secrets);
    execute_request(resolved, &redactor).await
}
```

브라우저 preview에서는 secret seal/send/revealed cURL을 거부한다. 원문 cURL은
사용자 확인 뒤 backend가 한 번만 생성하고 React state나 persistence에 넣지 않은
채 clipboard로 전달한다.

### Redirect and response redaction

- reqwest 자동 redirect를 끄고 최대 10 hop을 직접 처리한다.
- cross-origin redirect에서는 Authorization, Cookie, API key 계열 header와 auth,
  body를 다음 요청에 전달하지 않는다.
- 301/302의 POST 및 303은 GET으로 전환한다.
- final URL과 hop location, 민감 응답 header, JSON 민감 key, 알려진 token 패턴,
  form assignment 및 environment secret의 정확한 값은 `[REDACTED]`로 반환한다.
- timeout과 transport failure는 요청 원문을 포함하지 않는 고정된 안전 오류로
  변환한다.

### Fail-closed persistence migration

History는 `apip-history-v2`, Collection은 `apip-collections-v2`를 사용한다.
History v1은 평문 포함 여부를 증명할 수 없으므로 읽거나 변환하거나 backup하지
않는다. 빈 v2를 write/read-back한 뒤 raw v1을 delete/read-back하고 marker를
기록한다. 어느 단계든 실패하면 UI에는 빈 in-memory store만 반환하고 marker를
남기지 않아 다음 시작에서 재시도한다.

Collection v1은 요청을 먼저 locally sanitize한 뒤 backend scanner를 통과한 v2만
저장한다. Authorization, Cookie, API key, auth username/password/token/value,
민감 JSON/form/query 값은 마스킹하고 `requiresSecretReview`를 기록한다. 안전한 v2
read-back 전에는 v1을 삭제하지 않으며 raw backup·quarantine을 만들지 않는다.

앱 시작 시 History와 Collection 검증·이관이 모두 끝날 때까지 send/save를
비활성화해 startup race로 검증 전 데이터가 렌더링되거나 다시 저장되지 않게 했다.

## Changes Made

### Version and metadata

- `Cargo.lock` — `api-playground` package version을 0.3.2로 고정했다.
- `apps/api-playground/package.json` — frontend package version을 0.3.2로 올렸다.
- `apps/api-playground/src-tauri/Cargo.toml` — Rust package version을 0.3.2로
  올렸다.
- `apps/api-playground/src-tauri/tauri.conf.json` — packaged app version을
  0.3.2로 맞췄다.

### Rust request boundary

- `apps/api-playground/src-tauri/src/commands/request.rs` — backend-only resolve,
  manual redirect, cross-origin credential stripping, cURL generation, persistence
  scanner, response/error/URL/body/header redaction 및 로컬 two-server 회귀 테스트를
  구현했다.
- `apps/api-playground/src-tauri/src/commands/secrets.rs` — frontend에서 호출하던
  secret unseal command를 제거했다.
- `apps/api-playground/src-tauri/src/lib.rs` — 제거된 unseal command 대신 안전한
  request/persistence command만 Tauri handler에 등록했다.

### Frontend behavior

- `apps/api-playground/src/App.tsx` — persistence readiness gate, 안전한 startup
  migration, masked/revealed cURL 확인 흐름, 저장 실패 격리 및 보안 상태 안내를
  적용했다.
- `apps/api-playground/src/App.css` — migration·secret review·원문 복사 확인 UI
  상태의 스타일을 추가했다.
- `apps/api-playground/src/api.ts` — browser preview fail-closed 동작과 backend
  request/persistence/revealed-cURL API 경계를 반영했다.
- `apps/api-playground/src/types.ts` — template, persisted request, redirect hop 및
  response wire type을 분리했다.

### Persistence modules

- `apps/api-playground/src/lib/persistence.ts` — History v2 migration, safe save,
  direct credential·invalid JSON·form·URL userinfo/query·known token sanitizer를
  구현했다.
- `apps/api-playground/src/lib/collections.ts` — Collection v2 conversion,
  `requiresSecretReview`, backend validation 및 원자적 v1 제거 순서를 구현했다.
- `apps/api-playground/src/lib/environments.ts` — browser pseudo-sealing을 제거하고
  secret 환경 저장의 fail-closed 경계를 적용했다.

### Tests and documentation

- `apps/api-playground/src/App.test.ts` — startup readiness, migration isolation,
  masked/revealed cURL 및 실패 상태 회귀를 검증한다.
- `apps/api-playground/src/lib/persistence.test.ts` — History v2 write/delete/marker
  실패, 재시도 및 payload sanitizer를 검증한다.
- `apps/api-playground/src/lib/collections.test.ts` — v1 conversion과 v2/marker
  failure, direct credential redaction을 검증한다.
- `apps/api-playground/src/lib/environments.test.ts` — browser secret operation이
  거부되고 평문 fallback이 생기지 않음을 검증한다.
- `apps/api-playground/README.md` — backend-only 해석, 저장·redirect·cURL·응답
  보안 경계와 v1 migration 동작을 문서화했다.

## Verification Results

집중 코드 검토에서 다음을 추가로 찾아 수정했다.

- persistence startup race
- Rust의 camelCase `apiKey` 민감 key 인식
- malformed JSON, URL query/userinfo 및 browser response redaction 누락
- v2 write/read-back과 marker failure 회귀 공백

로컬 검증:

```text
API Playground frontend tests       55 passed / 0 failed
API Playground Rust tests           11 passed / 0 failed
cargo fmt --all -- --check          PASS
cargo clippy -p api-playground ...  PASS
cargo test --workspace --all-targets PASS
cargo check --workspace             PASS
API Playground production build     PASS
13-app sequential frontend build    PASS
catalog consistency                 PASS
git diff --check                    PASS
```

기본 병렬 frontend build는 Run Manager에서 로컬 `ENOMEM`이 발생했지만 같은 전체
workspace를 순차 실행했을 때 모두 통과해 source failure가 아닌 host memory pressure로
판정했다.

GitHub Actions PR #225 최종 실행:

```text
Catalog consistency          PASS
Detect changed scope         PASS
Frontend (pnpm)              PASS
Rust (Cargo workspace)       PASS
Rust (Windows compile check) PASS
```

첫 CI 실행은 stable Rust 1.98의 신규 lint가 기존 WSL/Code Pad 코드에서 발생해
실패했다. 별도 PR #226으로 저장소 전체 해당 lint를 수정하고 PR #225를 최신 main에
rebase한 뒤 최종 CI가 통과했다. API 보안 코드 자체의 CI 실패는 없었다.

## Windows Evidence and Next Steps

- H0/W0: v0.4.1 공식 portable 중 격리 가능한 10개 앱의 cold start가 통과했고,
  API Playground도 expected title, Responding, 생존, stderr 0을 직접 확인했다.
- H1: `v0.4.2-rc1` 공식 API Playground 0.3.2 package에서 DPAPI ciphertext,
  History raw v1 삭제, Collection 안전 변환, cURL/response/error redaction,
  sanitized localStorage와 cross-origin credential stripping을 격리 검증한다.
- H1 성공 뒤 stable release 문서를 merge하고 annotated `v0.4.2` tag의 build,
  publish, manifest verification을 완료한다.
