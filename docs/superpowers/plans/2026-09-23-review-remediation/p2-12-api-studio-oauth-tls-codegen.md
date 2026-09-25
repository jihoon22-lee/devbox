# P2-12 API Studio 보강 3: OAuth 2.0·TLS 설정·코드 생성(PR 2개) — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4와 `CONVENTIONS.md`의 "메서드 추가 절차"(P1-11)를 읽는다.

**Goal:** HTTP 요청에 OAuth 2.0 인증(Authorization Code + PKCE, Client Credentials, 토큰 자동 갱신)을 더하고(PR A), 요청별 TLS 설정(사용자 CA, 클라이언트 인증서, 인증서 검증 끄기)과 코드 생성(curl·fetch·Python·Go·C#)을 더한다(PR B)(D25 4순위, `review.md` §8 표 5번).

**Architecture:**
- OAuth: MCP용으로 이미 있는 부품을 재사용한다 — `api_protocols::core::oauth`(`generate_state_and_pkce`, `parse_callback_request`, `parse_token_response`, `validate_client_id`, `validate_scopes`, `validate_secure_url`)와 `commands/mcp_oauth.rs`의 loopback 콜백·폼 전송·콜백 페이지 코드. 공용 부분을 `commands/oauth_common.rs`로 옮기고(`send_form`, `read_callback`, `write_callback_page`, `oauth_client`), `build_authorization_url`의 `resource`를 선택값으로 바꾼다(MCP는 계속 넘긴다).
  - 설정은 요청의 `AuthConfig`에 선택 필드 `oauth2`로 둔다: `grantType`(`authorizationCode` | `clientCredentials`), `authorizationUrl`, `tokenUrl`, `clientId`, `clientSecret`(보통 `{{변수}}` 참조), `scopes`(공백 구분). `kind: "oauth2"`.
  - 토큰 캐시: API Studio 데이터 폴더의 `oauth2-tokens.json`, access·refresh 토큰은 DPAPI로 봉인(P1-09 `secrets::dpapi::DpapiSealer`, entropy `"devbox.api-studio.oauth2-token"`). 키는 `(grantType, tokenUrl, clientId, 정렬한 scopes)`의 SHA-256(비밀은 키에 넣지 않음).
  - 전송 때: 유효한 캐시 토큰(만료 60초 전까지) → refresh 토큰으로 갱신 → client credentials면 새로 받기 → authorization code인데 토큰이 없으면 `oauth2_authorization_required`로 멈추고 화면이 "로그인"을 보인다. 받은 토큰은 redaction 대상에 넣는다.
- TLS: gRPC에 이미 있는 DPAPI TLS 자격 증명 저장소(`commands/grpc_credentials.rs`의 `GrpcCredentialState::resolve_for_connection`)를 HTTP도 쓴다(화면 이름을 "TLS 자격 증명"으로). 요청에 선택 필드 `tls: { credentialId: string | null, verify: boolean }`(기본 `null`·`true`). native는 reqwest(rustls)에 `add_root_certificate`, `identity(Identity::from_pem)`, `danger_accept_invalid_certs`를 건다. PKCS#12(`.pfx`)는 rustls에서 읽을 수 없으므로 PEM만(가이드에 `openssl pkcs12` 변환 안내).
- 코드 생성: 프런트 순수 함수 `lib/codegen.ts`. 비밀 값은 절대 넣지 않는다 — 비밀 변수와 값이 없는 변수는 `{{name}}` 그대로 두고 맨 위 주석에 채울 이름을 적는다. 비밀까지 드러낸 curl이 필요하면 기존 "비밀 포함 curl 복사"(`build_revealed_curl`)를 쓴다.

**Tech Stack:** Rust(reqwest rustls, tokio), TypeScript·React 19, Vitest

**Spec:** `review.md` §8 신규 기능 표 5번 · `00-roadmap.md` D25 · ADR 0016

## Global Constraints

- `00-roadmap.md` §3 전부 적용. 새 의존성 없음(OAuth·TLS 모두 기존 crate 기능).
- OAuth URL은 `https`만(loopback `http://127.0.0.1`·`localhost`는 허용, 기존 `validate_secure_url(…, true)`). 콜백은 `http://127.0.0.1:<임의 포트>/oauth/callback`, 흐름 제한 5분, 동시에 하나.
- 토큰 캐시 최대 64개, 파일 1MiB. 토큰·콜백 파라미터·client secret은 IPC로 나가지 않는다(상태 투영만: `valid | expired | missing`, 만료 시각, scope).
- 인증서 검증 끄기는 요청 단위이고, 화면에 "인증서 검증 꺼짐" 표시를 늘 보인다. 저장·내보내기에는 설정 그대로 남는다(비밀이 아님).
- 코드 생성 결과에는 봉인된 값·`[REDACTED]`·평문 비밀이 없어야 한다(테스트로 고정).

## Review Focus

1. refresh 토큰이 거부되면(`invalid_grant`) 캐시를 지우고 authorization code 흐름은 "다시 로그인"을 보인다(무한 재시도 없음). (PR A Task A3 테스트)
2. 브라우저를 닫아 콜백이 오지 않으면 5분 뒤 또는 "취소"로 흐름이 끝나고 포트가 닫힌다. (A3 테스트)
3. client secret을 평문으로 적은 요청은 저장 때 `[REDACTED]`와 "비밀 검토 필요"가 된다. (A5 테스트)
4. 존재하지 않거나 지워진 TLS 자격 증명 id로 보내면 "TLS 자격 증명을 찾지 못했습니다"로 멈추고 검증 없는 연결로 대신 보내지 않는다. (PR B Task B2 테스트)
5. 코드 생성은 따옴표·줄바꿈·유니코드가 든 헤더·본문도 각 언어 문법으로 올바르게 이스케이프한다. (B4 테스트)

---

## PR A — OAuth 2.0

- 묶음: **B12** — 브랜치 `feat/devbox-api-studio/imports-runner-auth`, PR 제목 `feat(devbox-api-studio): imports, file collections, runner, OAuth 2.0, TLS and code generation`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(devbox-api-studio): OAuth 2.0 authorization code and client credentials`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

### Task A1: 공용 OAuth 부품 정리

**Files:** Modify `crates/api-protocols/src/core/oauth.rs`(`AuthorizationUrlInput.resource: Option<&str>`), `crates/http-client-engine/src/commands/mcp_oauth.rs`; Create `crates/http-client-engine/src/commands/oauth_common.rs`

- [ ] **Step 1: 실패하는 테스트** — `oauth.rs` 테스트 모듈

```rust
    #[test]
    fn authorization_url_without_resource_omits_the_parameter() {
        let url = build_authorization_url(AuthorizationUrlInput {
            endpoint: "https://auth.example.com/authorize",
            client_id: "devbox",
            redirect_uri: "http://127.0.0.1:5000/oauth/callback",
            state: "s",
            challenge: "c",
            resource: None,
            scopes: &["read".to_string(), "write".to_string()],
        })
        .unwrap();
        let pairs: Vec<(String, String)> = url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        assert!(!pairs.iter().any(|(k, _)| k == "resource"));
        assert!(pairs.contains(&("scope".into(), "read write".into())));
        assert!(pairs.contains(&("code_challenge_method".into(), "S256".into())));
    }
```

  기존 MCP 테스트는 `resource: Some(...)`로 고친 뒤 그대로 통과해야 한다.
- [ ] **Step 2: 실패 확인** — Run: `source ~/.cargo/env && cargo test -p api-protocols oauth` → FAIL(컴파일 오류)
- [ ] **Step 3: 구현** — `resource`를 `Option<&'a str>`로 바꾸고 `Some`일 때만 붙인다. `mcp_oauth.rs`의 `oauth_client`, `send_form`, `send_form_allow_empty`, `read_json_response`, `read_callback`, `write_callback_page`를 `oauth_common.rs`로 옮겨 `pub(crate)`로 두고 MCP 쪽은 그것을 부른다(동작 불변).
- [ ] **Step 4: 확인·커밋** — Run: `cargo test -p api-protocols && cargo test -p devbox-http-client-engine --lib mcp_oauth` → PASS. `git add -A && git commit -m "refactor(devbox-api-studio): share OAuth plumbing between MCP and HTTP"`

---

### Task A2: 설정 검증과 토큰 캐시

**Files:** Create `crates/http-client-engine/src/commands/oauth2/{mod.rs,config.rs,cache.rs}`; Modify `commands/mod.rs`

**Interfaces (Produces):**
- `config::OAuth2Config { grant_type: GrantType, authorization_url: String, token_url: String, client_id: String, client_secret: String, scopes: String }`(serde camelCase, `#[serde(default)]` 빈 값), `GrantType { AuthorizationCode, ClientCredentials }`, `config::validate(resolved: &OAuth2Config) -> Result<ValidatedConfig, &'static str>`(변수 치환 뒤 값), `config::profile_key(config: &ValidatedConfig) -> String`
- `cache::TokenCache::open(path: PathBuf, sealer: Arc<dyn devbox_secrets::Sealer>) -> Self`, `get(&self, key: &str, now_ms: u64) -> Result<CachedToken, &'static str>`(`CachedToken::{Valid { access: Zeroizing<String>, expires_at_ms: Option<u64> }, Expired { refresh: Option<Zeroizing<String>> }, Missing}`), `put(&self, key: &str, token: &TokenResponse, now_ms: u64) -> Result<(), &'static str>`, `remove(&self, key: &str) -> Result<(), &'static str>`, `status(&self, key: &str, now_ms: u64) -> TokenStatus { state: "valid" | "expired" | "missing", expires_at_ms: Option<u64>, scope: Option<String> }`
- 오류 코드: `oauth2_config_invalid`, `oauth2_authorization_required`, `oauth2_token_failed`, `oauth2_storage_failed`, `oauth2_cancelled`, `oauth2_busy`

- [ ] **Step 1: 실패하는 테스트**

```rust
// config.rs
#[cfg(test)]
mod tests {
    use super::*;

    fn config(grant: GrantType) -> OAuth2Config {
        OAuth2Config { grant_type: grant, authorization_url: "https://auth.x.test/authorize".into(), token_url: "https://auth.x.test/token".into(),
            client_id: "devbox".into(), client_secret: "s3cret".into(), scopes: "write read".into() }
    }

    #[test]
    fn validation_requires_secure_urls_and_the_right_fields() {
        assert!(validate(&config(GrantType::AuthorizationCode)).is_ok());
        let mut insecure = config(GrantType::ClientCredentials);
        insecure.token_url = "http://auth.x.test/token".into();
        assert_eq!(validate(&insecure).unwrap_err(), "oauth2_config_invalid");
        let mut loopback = config(GrantType::ClientCredentials);
        loopback.token_url = "http://127.0.0.1:8080/token".into();
        assert!(validate(&loopback).is_ok());
        let mut no_auth_url = config(GrantType::AuthorizationCode);
        no_auth_url.authorization_url.clear();
        assert_eq!(validate(&no_auth_url).unwrap_err(), "oauth2_config_invalid");
        let mut no_secret = config(GrantType::ClientCredentials);
        no_secret.client_secret.clear();
        assert_eq!(validate(&no_secret).unwrap_err(), "oauth2_config_invalid");
    }

    #[test]
    fn profile_keys_ignore_secret_and_scope_order() {
        let a = validate(&config(GrantType::ClientCredentials)).unwrap();
        let mut other = config(GrantType::ClientCredentials);
        other.client_secret = "different".into();
        other.scopes = "read write".into();
        assert_eq!(profile_key(&a), profile_key(&validate(&other).unwrap()));
        assert_ne!(profile_key(&a), profile_key(&validate(&config(GrantType::AuthorizationCode)).unwrap()));
    }
}
```

```rust
// cache.rs
#[cfg(test)]
mod tests {
    use super::*;

    struct FakeSealer;
    impl devbox_secrets::Sealer for FakeSealer {
        fn seal(&self, plain: &[u8]) -> Result<Vec<u8>, devbox_secrets::SealError> { Ok(plain.iter().rev().copied().collect()) }
        fn unseal(&self, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, devbox_secrets::SealError> { Ok(Zeroizing::new(sealed.iter().rev().copied().collect())) }
    }
    fn token(access: &str, refresh: Option<&str>, expires_in: Option<u64>) -> TokenResponse {
        TokenResponse::for_tests(access, refresh, expires_in, Some("read"))
    }

    #[test]
    fn tokens_are_sealed_on_disk_and_expire_with_a_safety_margin() {
        let dir = tempfile::tempdir().unwrap();
        let cache = TokenCache::open(dir.path().join("oauth2-tokens.json"), Arc::new(FakeSealer));
        cache.put("k", &token("access-1", Some("refresh-1"), Some(3600)), 1_000).unwrap();
        let raw = std::fs::read_to_string(dir.path().join("oauth2-tokens.json")).unwrap();
        assert!(!raw.contains("access-1") && !raw.contains("refresh-1"));
        assert!(matches!(cache.get("k", 1_000 + 3_000_000).unwrap(), CachedToken::Valid { .. }));
        match cache.get("k", 1_000 + 3_600_000 - 59_000).unwrap() {
            CachedToken::Expired { refresh } => assert_eq!(refresh.unwrap().as_str(), "refresh-1"),
            other => panic!("{other:?}"),
        }
        assert_eq!(cache.status("k", 1_000).state, "valid");
        cache.remove("k").unwrap();
        assert!(matches!(cache.get("k", 1_000).unwrap(), CachedToken::Missing));
    }

    #[test]
    fn a_corrupt_cache_is_treated_as_empty_and_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("oauth2-tokens.json"), "{broken").unwrap();
        let cache = TokenCache::open(dir.path().join("oauth2-tokens.json"), Arc::new(FakeSealer));
        assert!(matches!(cache.get("k", 1).unwrap(), CachedToken::Missing));
        cache.put("k", &token("a", None, None), 1).unwrap();
        assert!(matches!(cache.get("k", 10_000_000_000).unwrap(), CachedToken::Valid { .. }), "no expires_in means valid until rejected");
    }
}
```

  (`devbox_secrets::Sealer`의 실제 메서드 이름·오류 타입이 다르면 그 trait에 맞춘다. `TokenResponse::for_tests`가 없으면 `parse_token_response`로 JSON을 파싱해 만든다.)
- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-http-client-engine --lib oauth2` → FAIL
- [ ] **Step 3: 구현** — `validate`: `client_id`는 `oauth::validate_client_id`, scope는 공백으로 나눠 `oauth::validate_scopes`, URL은 `validate_secure_url(…, true)`, authorization code면 `authorization_url` 필수, client credentials면 `client_secret` 필수. 캐시 문서 `{schema: "devbox.api-studio.oauth2-tokens", version: 1, tokens: [{key, access, refresh?, expiresAtMs?, scope?, obtainedAtMs}]}`(토큰 필드는 봉인 후 base64), 쓰기는 `devbox_filesystem::atomic_write`, 64개를 넘으면 가장 오래된 것부터 버린다. 만료 판정은 `expires_at_ms - 60_000 <= now`. 캐시는 `Mutex`로 한 번에 한 쓰기.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS. `git add -A && git commit -m "feat(devbox-api-studio): OAuth 2.0 config validation and sealed token cache"`

---

### Task A3: 토큰 받기·갱신·로그인 흐름

**Files:** Create `commands/oauth2/flows.rs`

**Interfaces (Produces):** `client_credentials(client: &reqwest::Client, config: &ValidatedConfig, cancel: &mut watch::Receiver<bool>) -> Result<TokenResponse, &'static str>`, `refresh(client, config, refresh_token: &str, cancel) -> Result<RefreshOutcome { Token(TokenResponse), Rejected }, &'static str>`, `authorization_code(client, config, open: &dyn Fn(&str) -> Result<(), ()>, cancel, timeout: Duration) -> Result<TokenResponse, &'static str>`, `OAuth2State { active: Mutex<Option<(String /*request id*/, watch::Sender<bool>)>>, cache: OnceCell<TokenCache> }`

- [ ] **Step 1: 실패하는 테스트** — 로컬 가짜 토큰 서버(테스트 안에서 `tokio::net::TcpListener`로 요청 하나를 읽고 정해진 응답을 쓰는 도우미 `serve_once(status, body) -> (url, JoinHandle<String /*받은 요청 원문*/>)`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn client_credentials_posts_a_form_and_parses_the_token() {
        let (url, request) = serve_once(200, r#"{"access_token":"tok","token_type":"Bearer","expires_in":60}"#).await;
        let config = test_config(GrantType::ClientCredentials, &url);
        let token = client_credentials(&test_client(), &config, &mut never_cancelled()).await.unwrap();
        assert_eq!(token.access_token.as_str(), "tok");
        let raw = request.await.unwrap();
        assert!(raw.contains("grant_type=client_credentials") && raw.contains("client_id=devbox") && raw.contains("scope=read"));
    }

    #[tokio::test]
    async fn refresh_rejection_is_reported_separately() {
        let (url, _request) = serve_once(400, r#"{"error":"invalid_grant"}"#).await;
        let outcome = refresh(&test_client(), &test_config(GrantType::AuthorizationCode, &url), "r1", &mut never_cancelled()).await.unwrap();
        assert!(matches!(outcome, RefreshOutcome::Rejected));
    }

    #[tokio::test]
    async fn authorization_code_completes_when_the_browser_hits_the_callback() {
        let (token_url, token_request) = serve_once(200, r#"{"access_token":"code-tok","token_type":"bearer"}"#).await;
        let config = test_config(GrantType::AuthorizationCode, &token_url);
        let open = |authorization_url: &str| -> Result<(), ()> {
            let url = reqwest::Url::parse(authorization_url).unwrap();
            let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
            let callback = format!("{}?code=abc&state={}", pairs["redirect_uri"], pairs["state"]);
            tokio::spawn(async move { let _ = reqwest::get(callback).await; });
            Ok(())
        };
        let token = authorization_code(&test_client(), &config, &open, &mut never_cancelled(), Duration::from_secs(10)).await.unwrap();
        assert_eq!(token.access_token.as_str(), "code-tok");
        let raw = token_request.await.unwrap();
        assert!(raw.contains("grant_type=authorization_code") && raw.contains("code=abc") && raw.contains("code_verifier="));
    }

    #[tokio::test]
    async fn authorization_code_times_out_and_can_be_cancelled() {
        let config = test_config(GrantType::AuthorizationCode, "https://auth.x.test/token");
        let error = authorization_code(&test_client(), &config, &|_| Ok(()), &mut never_cancelled(), Duration::from_millis(200)).await.unwrap_err();
        assert_eq!(error, "oauth2_cancelled");
        let (sender, mut receiver) = tokio::sync::watch::channel(false);
        let flow = authorization_code(&test_client(), &config, &|_| Ok(()), &mut receiver, Duration::from_secs(60));
        let (result, _) = tokio::join!(flow, async { sender.send(true).unwrap(); });
        assert_eq!(result.unwrap_err(), "oauth2_cancelled");
    }
}
```

  (`test_config`는 `authorization_url: "https://auth.x.test/authorize"`, 주어진 `token_url`, client id `devbox`, secret `s`, scope `read`로 `ValidatedConfig`를 만든다. 로컬 토큰 서버가 `http://127.0.0.1:<port>`라 loopback 허용 규칙으로 통과한다.)
- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-http-client-engine --lib oauth2::flows` → FAIL
- [ ] **Step 3: 구현** — client credentials: `send_form(token_url, [grant_type, client_id, client_secret, scope?])`. refresh: `[grant_type=refresh_token, refresh_token, client_id, client_secret?(있으면)]`, 400·401과 `error: invalid_grant`면 `Rejected`. authorization code: `TcpListener::bind((Ipv4Addr::LOCALHOST, 0))`, `generate_state_and_pkce`, `build_authorization_url(resource: None)`, `open(url)`, 콜백 대기(`tokio::select!` 취소·`timeout`), 피어가 loopback인지 확인, `parse_callback_request(bytes, state, "", false)`, 토큰 교환 `[grant_type=authorization_code, code, redirect_uri, client_id, code_verifier, client_secret?]`, 콜백 페이지(`write_callback_page`) 응답. 타임아웃·취소는 `oauth2_cancelled`, 토큰 응답 오류는 `oauth2_token_failed`. `OAuth2State`는 동시에 한 흐름만(`oauth2_busy`).
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS. `git add -A && git commit -m "feat(devbox-api-studio): OAuth 2.0 token flows"`

---

### Task A4: 전송 경로 연결과 native 메서드

**Files:** Modify `crates/http-client-engine/src/commands/request.rs`(`AuthConfig.oauth2`, 변수 치환, 인증 적용, redaction), `commands/oauth2/mod.rs`(명령), `api.rs`·`component.rs`(P1-13 `http_client_engine::api::ApiCall`, `METHODS`, `classify`), `apps/devbox-api-studio/src-tauri/tests/typescript.rs`(생성 TS)

**Interfaces (Produces):** `AuthConfig { …, #[serde(default)] oauth2: Option<OAuth2Config> }`; native 메서드 `oauth2_status { auth: AuthConfig, environment }` → `TokenStatus`, `authorize_oauth2 { requestId, auth, environment }` → `TokenStatus`, `cancel_oauth2 { requestId }`, `fetch_oauth2_token { auth, environment }` → `TokenStatus`, `clear_oauth2_token { auth, environment }`

- [ ] **Step 1: 실패하는 테스트** — `request.rs` 테스트 모듈
  - `referenced_variable_names`가 `oauth2.clientSecret`의 `{{clientSecret}}`을 포함하고 `resolve_template`이 치환한다.
  - 순수 함수 `oauth2_header_plan(cache_state, grant) -> Plan { UseToken, Refresh, Fetch, RequireLogin }` 표: Valid → UseToken, Expired+refresh → Refresh, Expired·Missing + clientCredentials → Fetch, Expired(refresh 없음)·Missing + authorizationCode → RequireLogin.
  - 받은 access 토큰이 redactor의 비밀 목록에 들어간다(응답 본문에 같은 토큰이 있으면 기록에서 가려짐) — `Redactor::for_request`에 토큰을 넘기는 경로를 테스트.
- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-http-client-engine --lib request::tests::oauth2` → FAIL
- [ ] **Step 3: 구현** — `send_request`에서 `kind == "oauth2"`이면 치환된 설정으로 `validate` → `profile_key` → 캐시 상태에 따라 `oauth2_header_plan` → 갱신·받기 뒤 캐시에 저장 → `Authorization: Bearer <token>`(교차 출처 리다이렉트에서는 기존 규칙대로 빼고 보낸다). `Rejected`면 캐시를 지우고 authorization code는 `oauth2_authorization_required`, client credentials는 한 번 새로 받는다. 명령 다섯 개를 `ApiCall`에 더하고(`cancel_oauth2`만 control 계열, 나머지는 일반) `METHODS` 목록 테스트와 생성 TS를 갱신한다.
- [ ] **Step 4: 확인·커밋** — Run: `cargo test -p devbox-http-client-engine --lib && cargo test -p devbox-api-studio --lib && bash .github/scripts/check-generated-bindings.sh` → PASS. `git add -A && git commit -m "feat(devbox-api-studio): send requests with OAuth 2.0 tokens"`

---

### Task A5: 화면과 저장 정화

**Files:** Modify `packages/api-studio-features/src/requests/{types.ts,lib/persistence.ts,lib/transfer.ts,api.ts}`(+tests), 인증 편집 부품(P1-15에서 나눈 요청 편집의 인증 탭); Create `OAuth2Editor.tsx`, `OAuth2Editor.test.tsx`

- [ ] **Step 1: 실패하는 테스트**
  - `persistence.test.ts`: `auth: {kind: "oauth2", oauth2: {clientSecret: "plain-secret-value", …}}`를 저장 정화하면 `clientSecret`이 `[REDACTED]`, `requiresSecretReview: true`. `{{clientSecret}}` 참조는 그대로.
  - `transfer.test.ts`: `oauth2`가 든 요청이 내보내기·가져오기로 왕복한다.
  - `OAuth2Editor.test.tsx`: 인증 종류 "OAuth 2.0" → 방식(Authorization Code(PKCE)·Client Credentials), 필드(Authorization URL은 Authorization Code일 때만), 상태 줄("토큰 없음" / "유효 · 12분 뒤 만료" / "만료됨"). Authorization Code면 "로그인"(→ `authorize_oauth2`, 진행 중 "브라우저에서 로그인을 마쳐 주세요." + "취소"(→ `cancel_oauth2`)), Client Credentials면 "토큰 받기"(→ `fetch_oauth2_token`), 공통 "토큰 지우기". 전송 결과가 `oauth2_authorization_required`면 응답 영역에 "로그인이 필요합니다"와 "로그인" 버튼. client secret 칸에는 "환경 변수의 비밀 값({{이름}})을 쓰세요." 도움말. axe 위반 0.
- [ ] **Step 2: 실패 확인·구현·확인** — `AuthConfig` 타입에 `oauth2?: OAuth2Config | null`, 정화기는 `clientSecret`을 다른 인증 비밀과 같은 규칙으로 처리하고 URL·clientId·scopes는 메타데이터 한도로 자른다. Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/requests` → PASS. 커밋: `git add -A && git commit -m "feat(devbox-api-studio): OAuth 2.0 editor"`

---

### Task A6: PR 완료

- [ ] 가이드에 "OAuth 2.0" 문단(두 방식, 리다이렉트 URI `http://127.0.0.1/oauth/callback`에 임의 포트를 쓰므로 공급자에 loopback 리다이렉트를 등록, 토큰 저장 위치와 지우기).
- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): GitHub OAuth App(Authorization Code + PKCE)으로 로그인 후 `https://api.github.com/user` 요청, client credentials를 지원하는 공급자(예: 테스트용 Keycloak·Auth0 무료 테넌트 중 쓰는 것) 하나, 토큰 만료 후 자동 갱신.

---

## PR B — TLS 설정·코드 생성

- 묶음: **B12** — 브랜치 `feat/devbox-api-studio/imports-runner-auth`, PR 제목 `feat(devbox-api-studio): imports, file collections, runner, OAuth 2.0, TLS and code generation`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(devbox-api-studio): per-request TLS settings and code generation`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).
- 순서: 같은 묶음 안에서 PR A 과제를 모두 마친 뒤(같은 인증 편집 부품을 건드림).

### Task B1: 요청의 TLS 필드

**Files:** Modify `packages/api-studio-features/src/requests/{types.ts,lib/persistence.ts,lib/transfer.ts,lib/fileCollection.ts}`(+tests), `crates/http-client-engine/src/commands/request.rs`(`RequestTemplate.tls`)

**Interfaces (Produces):** TS `interface RequestTls { credentialId: string | null; verify: boolean }`, `RequestTemplate.tls?: RequestTls | null`; Rust `RequestTls { #[serde(default)] credential_id: Option<String>, #[serde(default = "yes")] verify: bool }`, `RequestTemplate { …, #[serde(default)] tls: Option<RequestTls> }`

- [ ] **Step 1: 실패하는 테스트** — TS: 옛 요청(필드 없음)은 `tls`가 없고, `{credentialId: "c1", verify: false}`는 저장·내보내기·파일 컬렉션 왕복에서 그대로. id 형식이 grpc 자격 증명 id 규칙에 맞지 않으면 `credentialId: null`로 정리. Rust: `serde_json::from_value::<RequestTemplate>(json!({… 옛 모양 …}))`이 `tls: None`.
- [ ] **Step 2: 실패 확인·구현·확인** — Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/requests/lib && cargo test -p devbox-http-client-engine --lib request::tests::tls_field` → PASS. 커밋: `git add -A && git commit -m "feat(devbox-api-studio): store per-request TLS settings"`

---

### Task B2: TLS를 쓰는 HTTP client

**Files:** Modify `crates/http-client-engine/src/commands/request.rs`(client 생성), `commands/grpc_credentials.rs`(화면 이름만), `crates/http-client-engine/tests/fixtures/tls/`(없으면 gRPC 테스트의 PEM fixture 재사용)

**Interfaces (Produces):** `build_http_client(timeout_ms: u64, tls: Option<&PreparedTlsCredential>, verify: bool) -> Result<reqwest::Client, String>`; 오류 `tls_credential_missing`(`"TLS 자격 증명을 찾지 못했습니다"`), `tls_credential_invalid`

- [ ] **Step 1: 실패하는 테스트**

```rust
    #[test]
    fn http_clients_accept_valid_pem_material_and_reject_broken_material() {
        let ca = include_str!("../../tests/fixtures/tls/ca.pem");
        let cert = include_str!("../../tests/fixtures/tls/client.pem");
        let key = include_str!("../../tests/fixtures/tls/client.key");
        let full = PreparedTlsCredential { ca_pem: Some(ca.into()), client_certificate_pem: Some(cert.into()), client_key_pem: Some(key.into()) };
        assert!(build_http_client(1_000, Some(&full), true).is_ok());
        assert!(build_http_client(1_000, None, false).is_ok());
        let broken = PreparedTlsCredential { ca_pem: Some("-----BEGIN CERTIFICATE-----\nnope\n-----END CERTIFICATE-----\n".into()), client_certificate_pem: None, client_key_pem: None };
        assert_eq!(build_http_client(1_000, Some(&broken), true).unwrap_err(), "tls_credential_invalid");
    }
```

  `send_request` 경로 테스트: 없는 `credentialId`면 네트워크에 연결하기 전에 `tls_credential_missing`(가짜 자격 증명 저장소로).
- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-http-client-engine --lib http_clients_accept` → FAIL
- [ ] **Step 3: 구현** — 기존 `reqwest::Client::builder()…` 자리를 `build_http_client`로 바꾼다: CA PEM의 인증서마다 `Certificate::from_pem` → `add_root_certificate`(시스템 루트는 유지), 인증서+키가 있으면 `Identity::from_pem(cert + "\n" + key)` → `identity`, `verify == false`면 `danger_accept_invalid_certs(true)`. 요청에 `tls.credentialId`가 있으면 `GrpcCredentialState::resolve_for_connection`으로 읽는다(없으면 `tls_credential_missing`, 검증 없는 연결로 대신하지 않는다). 자격 증명 화면 제목 "gRPC TLS 자격 증명" → "TLS 자격 증명"(gRPC·HTTP 공용). fixture가 없으면 테스트용 CA·클라이언트 인증서·키를 `openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -days 3650 -nodes …`로 만들어 커밋한다(비밀이 아닌 테스트 전용 키라는 README 한 줄).
- [ ] **Step 4: 확인·커밋** — Run: `cargo test -p devbox-http-client-engine --lib` → PASS. `git add -A && git commit -m "feat(devbox-api-studio): custom CA, client certificates and verification switch for HTTP"`

---

### Task B3: TLS 화면과 curl `-k`

**Files:** Create `packages/api-studio-features/src/requests/TlsSettings.tsx`, `TlsSettings.test.tsx`; Modify 요청 편집 부품, `lib/importers/curl.ts`(+test)

- [ ] **Step 1: 실패하는 테스트** — `TlsSettings.test.tsx`: 자격 증명 선택(기존 `list_grpc_tls_credentials` 결과, "사용 안 함" 포함), "인증서 검증" 체크를 끄면 "인증서 검증 꺼짐" 경고 문구가 편집기 머리에도 보인다. axe 위반 0. `curl.test.ts`: `-k`가 `tls: {credentialId: null, verify: false}`가 되고 경고가 없다(P2-10의 해당 경고 기대를 바꾼다).
- [ ] **Step 2: 실패 확인·구현·확인** — Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/requests` → PASS. 커밋: `git add -A && git commit -m "feat(devbox-api-studio): TLS settings view"`

---

### Task B4: 코드 생성

**Files:** Create `packages/api-studio-features/src/requests/lib/codegen.ts`, `codegen.test.ts`

**Interfaces (Produces):** `type CodeTarget = "curl" | "fetch" | "python" | "go" | "csharp"`, `generateCode(target: CodeTarget, request: RequestTemplate, environment: EnvVariable[]): { code: string; placeholders: string[] }`

- [ ] **Step 1: 실패하는 테스트**

```ts
import { describe, expect, it } from "vitest";
import { generateCode } from "./codegen";
import { emptyRequest } from "./importers";

const env = [{ key: "baseUrl", value: "https://api.x.test", secret: false }, { key: "token", value: "sealed-blob", secret: true }];
const post = {
  ...emptyRequest(), method: "POST", url: "{{baseUrl}}/notes",
  headers: [{ key: "X-Note", value: "it's \"quoted\"\nline", enabled: true }, { key: "X-Off", value: "1", enabled: false }],
  params: [{ key: "q", value: "한글 값" }],
  body_kind: "json", body: '{"title":"안녕"}',
  auth: { kind: "bearer", username: "", password: "", token: "{{token}}", api_key: "", api_value: "" },
};

describe("code generation", () => {
  it("curl escapes single quotes and keeps secret placeholders", () => {
    const { code, placeholders } = generateCode("curl", post, env);
    expect(placeholders).toEqual(["token"]);
    expect(code).toBe([
      "# 채워 넣을 값: {{token}}",
      "curl -X POST 'https://api.x.test/notes?q=%ED%95%9C%EA%B8%80%20%EA%B0%92' \\",
      "  -H 'X-Note: it'\\''s \"quoted\" line' \\",
      "  -H 'Authorization: Bearer {{token}}' \\",
      "  -H 'Content-Type: application/json' \\",
      "  --data-raw '{\"title\":\"안녕\"}'",
    ].join("\n"));
  });

  it("each language gets valid string literals", () => {
    expect(generateCode("fetch", post, env).code).toContain('"X-Note": "it\'s \\"quoted\\" line"');
    expect(generateCode("python", post, env).code).toContain("'X-Note': 'it\\'s \"quoted\" line'");
    expect(generateCode("go", post, env).code).toContain('req.Header.Set("X-Note", "it\'s \\"quoted\\" line")');
    expect(generateCode("csharp", post, env).code).toContain('request.Headers.TryAddWithoutValidation("X-Note", "it\'s \\"quoted\\" line");');
  });

  it("never contains sealed values or redaction markers", () => {
    for (const target of ["curl", "fetch", "python", "go", "csharp"] as const) {
      const { code } = generateCode(target, { ...post, body: '{"k":"[REDACTED]"}' }, env);
      expect(code).not.toContain("sealed-blob");
      expect(code).not.toContain("[REDACTED]");
    }
  });
});
```

  (헤더 값의 줄바꿈은 HTTP에서 허용되지 않으므로 공백 하나로 바꾼다. `[REDACTED]`는 `{{REDACTED_VALUE}}` 자리표시로 바꾸고 placeholders에 넣는다.)
- [ ] **Step 2: 실패 확인·구현·확인** — 공통 단계: 비밀이 아닌 환경 값으로 `{{name}}`을 치환(비밀·없는 변수는 그대로 두고 placeholders에 이름을 모음) → URL에 활성 params를 붙임(`encodeURIComponent`) → 활성 헤더 + 인증(basic → `Authorization: Basic base64(user:pass)`은 값이 변수면 그대로 두고 표시만, bearer → `Authorization: Bearer …`, apikey → 해당 헤더, oauth2 → `Authorization: Bearer {{access_token}}`과 placeholder) + 본문 종류별 Content-Type(json·form) + 쿠키 → `Cookie` 헤더. 언어별 출력: curl(bash 작은따옴표), fetch(`await fetch(url, { method, headers, body })`, JSON 문자열 리터럴), Python(`requests.request(method, url, headers=headers, data=body)`), Go(`http.NewRequest` + `req.Header.Set` + `http.DefaultClient.Do`), C#(`HttpClient` + `HttpRequestMessage` + `StringContent`). multipart는 텍스트 파트만 각 언어 방식으로, 파일 파트는 `{{file:name}}` 자리표시. Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/requests/lib/codegen.test.ts` → PASS. 커밋: `git add -A && git commit -m "feat(devbox-api-studio): generate request code"`

---

### Task B5: 코드 화면

**Files:** Create `packages/api-studio-features/src/requests/CodePanel.tsx`, `CodePanel.test.tsx`; Modify 요청 편집 부품

- [ ] **Step 1: 실패하는 테스트** — "코드" 버튼 → 언어 탭(curl·JavaScript fetch·Python·Go·C#), 코드 블록, "복사"(클립보드 mock), placeholders가 있으면 "채워 넣을 값: token". 기존 "비밀 포함 curl 복사"는 그대로 남아 있다. axe 위반 0.
- [ ] **Step 2: 실패 확인·구현·확인** — Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/requests` → PASS. 커밋: `git add -A && git commit -m "feat(devbox-api-studio): code panel"`

---

### Task B6: PR 완료

- [ ] 가이드에 "TLS 설정"(PEM만, `.pfx` 변환 명령 `openssl pkcs12 -in client.pfx -out client.pem -nodes`)과 "코드 생성" 문단.
- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): 사내·로컬의 자체 서명 HTTPS 서버에 (1) 검증 켠 채 실패, (2) CA를 등록한 자격 증명으로 성공, (3) 검증 끄고 성공. 생성한 Python·curl 코드를 WSL에서 실행해 같은 응답.
