// v0.9.0 persistence·oauth2Persistence·tlsPersistence·requestChecksPersistence 시험의 적응(07b A 표).
// v1은 localStorage 대신 컬렉션 파일에 저장하므로 입력은 파일 모양 `StoredRequest`, 결과에 가린 위치 `masked`가 있다.
import { describe, expect, it } from "vitest";
import { emptyDraft, setAuthKind, toSnapshot, updateAuth } from "../model/draft";
import { parseRequestFile, serializeRequestFile } from "../model/requestFile";
import { normalizeStoredRequest, type StoredPart, type StoredRequest } from "../model/stored";
import { variableUses } from "../runner/plan";
import { isExactVariableReference } from "./references";
import { REDACTED, sanitizeForStorage, stillMasked } from "./storage";

const personal = { kind: "personal" } as const;
const project = { kind: "project", projectId: "p1" } as const;

function stored(overrides: Partial<StoredRequest> = {}): StoredRequest {
  return {
    method: "POST",
    url: "https://api.example.com/x",
    query: [],
    headers: [],
    cookies: [],
    body: { kind: "none" },
    auth: { kind: "none" },
    tls: { verify: true, caPath: null, clientCertPath: null, clientKeyPath: null },
    timeoutMs: 30_000,
    followRedirects: true,
    ...overrides,
  };
}
const on = (name: string, value: string, enabled = true) => ({ name, value, enabled });
const text = (name: string, value: string): StoredPart => ({
  kind: "text",
  name,
  value,
  contentType: null,
  enabled: true,
});

describe("file storage sanitizer", () => {
  it("direct_secrets_in_json_form_url_auth_are_masked_and_references_kept", () => {
    const { stored: safe, masked } = sanitizeForStorage(
      stored({
        url: "https://url-user:url-password@example.com/x?token=url-token&api_key=url-api-key&name=alice",
        query: [on("access_token", "param-token"), on("q", "safe-query")],
        headers: [
          on("Authorization", "Bearer header-secret"),
          on("Cookie", "session=cookie-secret"),
          on("X-Trace", "${TRACE_ID}"),
        ],
        cookies: [
          on("session", "direct-cookie"),
          on("token", "${COOKIE_TOKEN}"),
          on("mixed", "prefix-${COOKIE_TOKEN}"),
          on("disabled", "disabled-secret", false),
        ],
        body: {
          kind: "json",
          text: JSON.stringify({
            password: "json-password",
            token: "${BODY_TOKEN}",
            nested: { apiKey: "json-api-key" },
            safe: "safe-body",
          }),
        },
        auth: { kind: "basic", username: "auth-user", password: "auth-password" },
      }),
      personal,
    );
    for (const secret of ["url-user", "url-password", "url-token", "url-api-key"]) {
      expect(safe.url).not.toContain(secret);
    }
    expect(decodeURIComponent(safe.url)).toContain("token=[REDACTED]");
    expect(safe.query).toEqual([on("access_token", REDACTED), on("q", "safe-query")]);
    expect(safe.headers).toEqual([on("Authorization", REDACTED), on("Cookie", REDACTED), on("X-Trace", "${TRACE_ID}")]);
    expect(safe.cookies).toEqual([
      on("session", REDACTED),
      on("token", "${COOKIE_TOKEN}"),
      on("mixed", REDACTED),
      on("disabled", REDACTED, false),
    ]);
    const body = safe.body.kind === "json" ? safe.body.text : "";
    expect(body).toContain('"password":"[REDACTED]"');
    expect(body).toContain('"token":"${BODY_TOKEN}"');
    expect(body).toContain('"apiKey":"[REDACTED]"');
    expect(body).toContain('"safe":"safe-body"');
    expect(safe.auth).toEqual({ kind: "basic", username: REDACTED, password: REDACTED });
    expect(JSON.stringify(safe)).not.toMatch(/direct-cookie|disabled-secret|json-password|json-api-key|auth-password/);
    expect(masked).toEqual([
      "url",
      "query.0.value",
      "headers.0.value",
      "headers.1.value",
      "cookies.0.value",
      "cookies.2.value",
      "cookies.3.value",
      "body.text",
      "auth.username",
      "auth.password",
    ]);
    for (const [auth, expected] of [
      [
        { kind: "bearer", token: "auth-token" },
        { kind: "bearer", token: REDACTED },
      ],
      [
        { kind: "apiKey", name: "X-API-Key", value: "auth-api-value", placement: "header" },
        { kind: "apiKey", name: "X-API-Key", value: REDACTED, placement: "header" },
      ],
    ] as const) {
      expect(sanitizeForStorage(stored({ auth }), personal).stored.auth).toEqual(expected);
    }
  });

  it("partial_reference_values_are_still_masked", () => {
    const { stored: safe, masked } = sanitizeForStorage(
      stored({
        url: "https://prefix-${USER}:prefix-${PASS}@example.test/x?token=prefix-${TOKEN}&keep=${SAFE}",
        headers: [on("Authorization", "Bearer prefix-${HEADER_TOKEN}"), on("X-Exact", "${EXACT}")],
        query: [on("access_token", "prefix-${PARAM_TOKEN}"), on("token", "${PARAM_REF}")],
        auth: { kind: "basic", username: "prefix-${USER}", password: "${PASS}" },
      }),
      personal,
    );
    expect(safe.headers).toEqual([on("Authorization", REDACTED), on("X-Exact", "${EXACT}")]);
    expect(safe.query).toEqual([on("access_token", REDACTED), on("token", "${PARAM_REF}")]);
    expect(safe.auth).toEqual({ kind: "basic", username: REDACTED, password: "${PASS}" });
    expect(safe.url).not.toContain("prefix-${");
    expect(decodeURIComponent(safe.url)).toContain("token=[REDACTED]");
    expect(safe.url).toContain("keep=%24%7BSAFE%7D");
    expect(masked).toContain("auth.username");
    expect(masked).not.toContain("auth.password");
  });

  it("scheme_and_reference_header_values_are_kept", () => {
    const headers = [on("Authorization", "Bearer {{secret:GH}}"), on("Proxy-Authorization", "Basic ${PROXY}")];
    expect(sanitizeForStorage(stored({ headers }), project)).toEqual({ stored: stored({ headers }), masked: [] });
    const typed = sanitizeForStorage(stored({ headers: [on("Authorization", "Bearer {{secret:GH}} extra")] }), project);
    expect(typed.stored.headers).toEqual([on("Authorization", REDACTED)]);
  });

  it("values_typed_next_to_an_old_mask_are_masked_again", () => {
    const { stored: safe, masked } = sanitizeForStorage(
      stored({
        headers: [on("Authorization", "[REDACTED]typed-secret")],
        auth: { kind: "basic", username: "{{USER}}", password: "[REDACTED]2" },
        body: { kind: "json", text: '{"password":"[REDACTED]x","ok":1}' },
      }),
      personal,
    );
    expect(JSON.stringify(safe)).not.toMatch(/typed-secret|\[REDACTED\]2|\[REDACTED\]x/);
    expect(masked).toEqual(["headers.0.value", "body.text", "auth.password"]);
    // 형식만 다른 값(공백 있는 JSON·정규화되는 URL)은 사용자가 쓴 글자 그대로 둔다.
    const plain = stored({ url: "https://api.test", body: { kind: "json", text: '{ "a": 1 }' } });
    expect(sanitizeForStorage(plain, personal)).toEqual({ stored: plain, masked: [] });
  });

  it("form_token_and_password_fields_are_masked", () => {
    const { stored: safe, masked } = sanitizeForStorage(
      stored({
        body: {
          kind: "form",
          fields: [
            on("token", "form-token"),
            on("password", "form-password"),
            on("name", "alice"),
            on("api_key", "${FORM_KEY}"),
          ],
        },
      }),
      personal,
    );
    expect(safe.body).toEqual({
      kind: "form",
      fields: [on("token", REDACTED), on("password", REDACTED), on("name", "alice"), on("api_key", "${FORM_KEY}")],
    });
    expect(masked).toEqual(["body.fields.0.value", "body.fields.1.value"]);
  });

  it("known_token_patterns_are_masked_without_sensitive_names", () => {
    const githubToken = "ghp_1234567890abcdef";
    const { stored: safe, masked } = sanitizeForStorage(
      stored({ body: { kind: "raw", text: `trace=${githubToken}`, contentType: null } }),
      personal,
    );
    expect(safe.body).toEqual({ kind: "raw", text: "trace=[REDACTED]", contentType: null });
    expect(masked).toEqual(["body.text"]);
  });

  it("project_root_drops_multipart_file_paths_and_stale_body", () => {
    const parts: StoredPart[] = [
      {
        kind: "file",
        name: "upload",
        path: "/home/u/private/artifact.zip",
        fileName: null,
        contentType: "application/zip",
        enabled: false,
      },
      text("token", "direct-token"),
      text("token", "${UPLOAD_TOKEN}"),
    ];
    const request = stored({ body: { kind: "multipart", parts } });
    const { stored: shared, masked } = sanitizeForStorage(request, project);
    const sharedParts = shared.body.kind === "multipart" ? shared.body.parts : [];
    expect(sharedParts[0]).toMatchObject({ kind: "file", path: "", fileName: "artifact.zip", enabled: false });
    expect(sharedParts[1]).toMatchObject({ value: REDACTED });
    expect(sharedParts[2]).toMatchObject({ value: "${UPLOAD_TOKEN}" });
    expect(JSON.stringify(shared)).not.toMatch(/\/home\/u|direct-token/);
    // 파일 경로를 지운 것은 비밀 가림이 아니다(가린 수에 넣지 않음).
    expect(masked).toEqual(["body.parts.1.value"]);
    // 개인 루트는 WSL 경로를 유지한다.
    const own = sanitizeForStorage(request, personal).stored.body;
    expect(own.kind === "multipart" && own.parts[0]).toMatchObject({ path: "/home/u/private/artifact.zip" });
    // binary 본문 경로도 같은 규칙.
    const binary = stored({ body: { kind: "binary", path: "/home/u/private/blob.bin" } });
    expect(sanitizeForStorage(binary, project).stored.body).toEqual({ kind: "binary", path: "" });
    expect(sanitizeForStorage(binary, personal).stored.body).toEqual({
      kind: "binary",
      path: "/home/u/private/blob.bin",
    });
    // 비활성 본문 초안(stale body)은 파일 모양에 자리가 없다.
    expect(Object.keys(shared.body)).toEqual(["kind", "parts"]);
  });

  it("template_urls_and_broken_json_fail_closed", () => {
    const { stored: safe, masked } = sanitizeForStorage(
      stored({
        url: "https://direct-user:direct-pass@${HOST}/x?access_token=direct-token&q=safe",
        body: { kind: "json", text: '{"password":"broken-secret", trailing}' },
      }),
      personal,
    );
    expect(safe.url).not.toMatch(/direct-user|direct-pass|direct-token/);
    expect(JSON.stringify(safe.body)).not.toContain("broken-secret");
    expect(masked).toEqual(["url", "body.text"]);
  });

  it("graphql_query_literals_and_variable_secrets_are_masked", () => {
    const { stored: safe, masked } = sanitizeForStorage(
      stored({
        body: {
          kind: "graphql",
          query: 'query Viewer { viewer(token: "query-secret", id: "42", ref: "{{ID}}") { id } }',
          variables:
            '{"token":"variable-secret","mixed_token":"prefix-${TOKEN}","reference_token":"${TOKEN}","id":"42"}',
          operationName: "Viewer",
          method: "post",
        },
      }),
      personal,
    );
    expect(safe.body).toEqual({
      kind: "graphql",
      query: 'query Viewer { viewer(token: "[REDACTED]", id: "[REDACTED]", ref: "{{ID}}") { id } }',
      variables: '{"token":"[REDACTED]","mixed_token":"[REDACTED]","reference_token":"${TOKEN}","id":"42"}',
      operationName: "Viewer",
      method: "post",
    });
    expect(masked).toEqual(["body.query", "body.variables"]);
  });

  it("non_graphql_request_does_not_store_stale_graphql_state", () => {
    // 이전에 GraphQL이던 파일을 JSON 요청으로 다시 저장: 원본의 알려진 키는 되살아나지 않는다.
    const original = {
      version: 1,
      name: "Viewer",
      request: {
        ...stored({
          body: {
            kind: "graphql",
            query: 'query Viewer { viewer(token: "stale-secret") { id } }',
            variables: "{}",
            operationName: "Viewer",
            method: "post",
          },
        }),
      },
      assertions: [],
      captures: [],
    };
    const parsed = parseRequestFile(JSON.stringify(original));
    const { text: written } = serializeRequestFile(
      { ...parsed.file, request: stored({ body: { kind: "json", text: '{"safe":true}' } }) },
      personal,
      parsed.raw,
    );
    expect(JSON.parse(written).request.body).toEqual({ kind: "json", text: '{"safe":true}' });
    expect(written).not.toContain("stale-secret");
  });

  it("oauth_client_secret_literal_is_masked", () => {
    const credentials = (clientSecret: string) =>
      stored({
        auth: {
          kind: "oauth2ClientCredentials",
          tokenUrl: "https://auth.test/token",
          clientId: "client",
          clientSecret,
          scope: "read write",
        },
      });
    const literal = sanitizeForStorage(credentials("literal-private-secret"), personal);
    expect(literal.stored.auth).toMatchObject({ clientSecret: REDACTED, clientId: "client", scope: "read write" });
    expect(literal.masked).toEqual(["auth.clientSecret"]);
    expect(sanitizeForStorage(credentials("{{CLIENT_SECRET}}"), personal)).toEqual({
      stored: credentials("{{CLIENT_SECRET}}"),
      masked: [],
    });
    const code = stored({
      auth: {
        kind: "oauth2AuthCode",
        authorizationUrl: "https://auth.test/authorize",
        tokenUrl: "https://auth.test/token",
        clientId: "client",
        clientSecret: null,
        scope: null,
      },
    });
    expect(sanitizeForStorage(code, personal)).toEqual({ stored: code, masked: [] });
  });

  it("inactive_authorization_url_is_ignored_for_client_credentials", () => {
    let draft = updateAuth(setAuthKind(emptyDraft(), "oauth2AuthCode"), "oauth2AuthCode", {
      authorizationUrl: "{{UNUSED_AUTH}}",
      tokenUrl: "https://auth.test/token",
      clientId: "client",
      clientSecret: "{{CLIENT_SECRET}}",
    });
    draft = updateAuth(setAuthKind(draft, "oauth2ClientCredentials"), "oauth2ClientCredentials", {
      tokenUrl: "https://auth.test/token",
      clientId: "client",
      clientSecret: "{{CLIENT_SECRET}}",
    });
    expect(variableUses(toSnapshot(draft))).toEqual(["CLIENT_SECRET"]);
    expect(variableUses(toSnapshot(setAuthKind(draft, "oauth2AuthCode")))).toEqual(["UNUSED_AUTH", "CLIENT_SECRET"]);
  });

  it("invalid_tls_references_are_cleared_and_verify_defaults_on", () => {
    const base = stored();
    expect(normalizeStoredRequest({ ...base, tls: { verify: false, caPath: "../private.pem" } }).tls).toEqual({
      verify: false,
      caPath: null,
      clientCertPath: null,
      clientKeyPath: null,
    });
    expect(
      normalizeStoredRequest({ ...base, tls: { caPath: "/etc/ssl/ca.pem", clientKeyPath: "/home/u/k\u0000.pem" } }).tls,
    ).toEqual({ verify: true, caPath: "/etc/ssl/ca.pem", clientCertPath: null, clientKeyPath: null });
    expect(normalizeStoredRequest({ ...base, tls: undefined }).tls.verify).toBe(true);
  });
});

describe("file checks", () => {
  const assertion = { id: "a", enabled: true, source: "status", target: "", operator: "equals", expected: "200" };
  const capture = { id: "c", enabled: true, variable: "token", source: "jsonPath", target: "$.token" };
  const file = (extra: Record<string, unknown>) =>
    JSON.stringify({ version: 1, name: "R", request: stored(), assertions: [], captures: [], ...extra });

  it("invalid_checks_are_dropped_and_expected_tokens_masked", () => {
    const parsed = parseRequestFile(
      file({
        assertions: [
          ...Array.from({ length: 51 }, (_, index) => ({ ...assertion, id: String(index) })),
          { ...assertion, id: "x", operator: "unknown" },
        ],
        captures: [{ ...capture, variable: "bad name" }, capture],
      }),
    );
    expect(parsed.file.assertions).toHaveLength(50);
    expect(parsed.file.captures).toEqual([capture]);
    expect(parsed.dropped).toBe(3);
    const token = parseRequestFile(
      file({
        assertions: [
          { ...assertion, expected: "ghp_abcdefghijklmnopqrstuvwxyz0123" },
          { ...assertion, source: "x" },
        ],
      }),
    );
    expect(token.file.assertions).toEqual([{ ...assertion, expected: REDACTED }]);
    const written = serializeRequestFile(
      { ...token.file, assertions: [{ ...assertion, expected: "ghp_abcdefghijklmnopqrstuvwxyz0123" }] } as never,
      personal,
    );
    expect(written.text).not.toContain("ghp_");
    expect(written.masked).toEqual(["assertions.0.expected"]);
  });

  it("invalid_expressions_are_kept_for_reporting", () => {
    const invalidRegex = { ...assertion, operator: "matches", expected: "(unclosed" };
    const invalidPath = { ...capture, target: "$[" };
    const parsed = parseRequestFile(file({ assertions: [invalidRegex], captures: [invalidPath] }));
    expect(parsed.file.assertions).toEqual([invalidRegex]);
    expect(parsed.file.captures).toEqual([invalidPath]);
    expect(parsed.dropped).toBe(0);
  });
});

describe("3c follow-ups (PROGRESS S5-3a·3b 후속)", () => {
  it("exact_references_follow_the_daemon_rules", () => {
    expect(isExactVariableReference("{{TOKEN}}")).toBe(true);
    expect(isExactVariableReference("{{ TOKEN\t}}")).toBe(true);
    expect(isExactVariableReference("${TOKEN}")).toBe(true);
    expect(isExactVariableReference("{{secret:GH}}")).toBe(true);
    // 개행 공백은 데몬 `variable_name`이 자르지 않으므로 참조가 아니다.
    expect(isExactVariableReference("{{\nTOKEN}}")).toBe(false);
    expect(isExactVariableReference("{{TOKEN\r\n}}")).toBe(false);
    // 토큰 모양 이름은 참조처럼 감싼 직접 입력 비밀로 본다.
    expect(isExactVariableReference("{{ghp_1234567890abcdef}}")).toBe(false);
    const { stored: safe } = sanitizeForStorage(
      stored({ headers: [on("Authorization", "{{ghp_1234567890abcdef}}"), on("X-Api-Key", "{{\nKEY}}")] }),
      personal,
    );
    expect(safe.headers).toEqual([on("Authorization", REDACTED), on("X-Api-Key", REDACTED)]);
  });

  it("graphql_comment_tokens_are_masked_by_the_token_pattern_pass", () => {
    const { stored: safe } = sanitizeForStorage(
      stored({
        body: {
          kind: "graphql",
          query: "# ghp_1234567890abcdef\nquery Viewer { viewer { id } }",
          variables: "",
          operationName: null,
          method: "post",
        },
      }),
      personal,
    );
    expect(JSON.stringify(safe.body)).not.toContain("ghp_");
  });

  it("only_recorded_masked_positions_block_sending", () => {
    const { stored: safe, masked } = sanitizeForStorage(
      stored({ headers: [on("Authorization", "Bearer direct"), on("X-Note", "[REDACTED] is fine")] }),
      personal,
    );
    expect(masked).toEqual(["headers.0.value"]);
    expect(stillMasked(safe, masked)).toEqual(["headers.0.value"]);
    const retyped = { ...safe, headers: [on("Authorization", "Bearer {{secret:TOKEN}}"), safe.headers[1]!] };
    expect(stillMasked(retyped, masked)).toEqual([]);
    // 사용자가 직접 쓴 `[REDACTED]` 글자는 막지 않는다(user_typed_redacted_text_is_not_blocked).
    expect(stillMasked(safe, [])).toEqual([]);
  });
});
