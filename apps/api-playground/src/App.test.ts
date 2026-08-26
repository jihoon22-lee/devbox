import { describe, expect, it } from "vitest";
import { buildCurl, shellQuote, statusClass, tryPretty } from "./App";
import type { RequestTemplate } from "./types";

describe("statusClass", () => {
  it("2xx는 status-2xx", () => {
    expect(statusClass(200)).toBe("status-2xx");
    expect(statusClass(299)).toBe("status-2xx");
  });

  it("199나 300은 2xx 범위 밖", () => {
    expect(statusClass(199)).toBe("status-other");
    expect(statusClass(300)).toBe("status-other");
  });

  it("399는 status-other, 400은 status-4xx (경계값)", () => {
    expect(statusClass(399)).toBe("status-other");
    expect(statusClass(400)).toBe("status-4xx");
  });

  it("500번대도 status-4xx 클래스로 분류된다 (기능상 버그 아님 — CSS에 .status-2xx/.status-4xx 둘뿐이라 " +
    "'에러는 빨간색'이라는 의도대로 동작한다. 다만 클래스 '이름'이 500에는 안 맞는다 — 오타 아님, 후속 정리 후보)", () => {
    expect(statusClass(500)).toBe("status-4xx");
    expect(statusClass(503)).toBe("status-4xx");
  });
});

describe("tryPretty", () => {
  it("유효한 JSON은 2칸 들여쓰기로 포맷한다", () => {
    expect(tryPretty('{"a":1}')).toBe('{\n  "a": 1\n}');
  });

  it("깨진 JSON은 원본 문자열을 그대로 반환한다", () => {
    expect(tryPretty("not json")).toBe("not json");
  });

  it("빈 문자열도 원본 그대로 반환한다", () => {
    expect(tryPretty("")).toBe("");
  });
});

describe("shellQuote", () => {
  it("일반 문자열은 작은따옴표로만 감싼다", () => {
    expect(shellQuote("hello")).toBe("'hello'");
  });

  it("작은따옴표가 포함된 문자열은 이스케이프한다", () => {
    expect(shellQuote("it's")).toBe(`'it'\\''s'`);
  });
});

function baseReq(overrides: Partial<RequestTemplate> = {}): RequestTemplate {
  return {
    method: "GET",
    url: "",
    headers: [],
    cookies: [],
    params: [],
    body_kind: "none",
    body: "",
    auth: { kind: "none", username: "", password: "", token: "", api_key: "", api_value: "" },
    timeout_ms: 10000,
    ...overrides,
  };
}

describe("buildCurl", () => {
  it("url이 비어 있으면 빈 문자열", () => {
    expect(buildCurl(baseReq({ url: "" }))).toBe("");
  });

  it("기본 GET 요청은 curl --request 한 줄", () => {
    expect(buildCurl(baseReq({ url: "https://api.example.com/users" }))).toBe(
      "curl --request GET 'https://api.example.com/users'",
    );
  });

  it("쿼리 파라미터가 없으면 ?를 붙이고, 이미 ?가 있으면 &로 이어붙인다", () => {
    const withoutQuery = buildCurl(
      baseReq({ url: "https://api.example.com/users", params: [{ key: "page", value: "2" }] }),
    );
    expect(withoutQuery).toContain("?page=2");

    const withQuery = buildCurl(
      baseReq({ url: "https://api.example.com/users?sort=name", params: [{ key: "page", value: "2" }] }),
    );
    expect(withQuery).toContain("users?sort=name&page=2");
  });

  it("key가 빈 파라미터/헤더는 무시한다", () => {
    const curl = buildCurl(
      baseReq({ url: "https://api.example.com", params: [{ key: "", value: "ignored" }], headers: [{ key: "", value: "ignored" }] }),
    );
    expect(curl).not.toContain("ignored");
  });

  it("중복 header 순서를 유지하고 disabled header는 기본 cURL에서 제외한다", () => {
    const curl = buildCurl(baseReq({
      url: "https://api.example.com",
      headers: [
        { key: "X-Trace", value: "one", enabled: true },
        { key: "X-Trace", value: "two", enabled: true },
        { key: "X-Skip", value: "not-sent", enabled: false },
      ],
    }));

    expect(curl.match(/X-Trace:/g)).toHaveLength(2);
    expect(curl.indexOf("X-Trace: one")).toBeLessThan(curl.indexOf("X-Trace: two"));
    expect(curl).not.toContain("X-Skip");
    expect(curl).not.toContain("not-sent");
  });

  it("basic auth는 기본 cURL에서 평문 대신 [REDACTED]를 사용한다", () => {
    const curl = buildCurl(
      baseReq({
        url: "https://api.example.com",
        auth: { kind: "basic", username: "user", password: "pass", token: "", api_key: "", api_value: "" },
      }),
    );
    expect(curl).toContain("Authorization: Basic [REDACTED]");
    expect(curl).not.toContain("user");
    expect(curl).not.toContain("pass");
  });

  it("bearer auth는 기본 cURL에서 token 평문을 남기지 않는다", () => {
    const curl = buildCurl(
      baseReq({
        url: "https://api.example.com",
        auth: { kind: "bearer", username: "", password: "", token: "tok123", api_key: "", api_value: "" },
      }),
    );
    expect(curl).toContain("Authorization: Bearer [REDACTED]");
    expect(curl).not.toContain("tok123");
  });

  it("bearer auth의 environment reference는 기본 cURL에 보존한다", () => {
    const curl = buildCurl(
      baseReq({
        url: "https://api.example.com",
        auth: { kind: "bearer", username: "", password: "", token: "${ACCESS_TOKEN}", api_key: "", api_value: "" },
      }),
    );
    expect(curl).toContain("Authorization: Bearer ${ACCESS_TOKEN}");
  });

  it("apikey auth는 헤더 이름을 보존하고 value 평문을 마스킹한다", () => {
    const curl = buildCurl(
      baseReq({
        url: "https://api.example.com",
        auth: { kind: "apikey", username: "", password: "", token: "", api_key: "X-API-Key", api_value: "secret" },
      }),
    );
    expect(curl).toContain("X-API-Key: [REDACTED]");
    expect(curl).not.toContain("secret");
  });

  it("apikey auth의 environment reference는 기본 cURL에 보존한다", () => {
    const curl = buildCurl(
      baseReq({
        url: "https://api.example.com",
        auth: { kind: "apikey", username: "", password: "", token: "", api_key: "X-API-Key", api_value: "${API_KEY}" },
      }),
    );
    expect(curl).toContain("X-API-Key: ${API_KEY}");
  });

  it("username이 없는 basic auth는 헤더를 추가하지 않는다", () => {
    const curl = buildCurl(
      baseReq({
        url: "https://api.example.com",
        auth: { kind: "basic", username: "", password: "", token: "", api_key: "", api_value: "" },
      }),
    );
    expect(curl).not.toContain("Authorization");
  });

  it("body_kind가 none이 아니고 body가 있으면 --data를 추가한다", () => {
    const curl = buildCurl(baseReq({ url: "https://api.example.com", body_kind: "json", body: '{"a":1}' }));
    expect(curl).toContain("--data '{\"a\":1}'");
  });

  it("JSON body의 민감한 field는 기본 cURL에 평문으로 포함하지 않는다", () => {
    const secret = "body-password-123";
    const curl = buildCurl(
      baseReq({
        url: "https://api.example.com",
        body_kind: "json",
        body: JSON.stringify({ username: "alice", password: secret, token: "${BODY_TOKEN}" }),
      }),
    );
    expect(curl).not.toContain(secret);
    expect(curl).toContain('"password":"[REDACTED]"');
    expect(curl).toContain('"token":"${BODY_TOKEN}"');
  });

  it("Authorization/Cookie와 알려진 token pattern header는 기본 cURL에서 마스킹한다", () => {
    const authorization = "Bearer direct-header-secret";
    const cookie = "session=direct-cookie-secret";
    const token = "ghp_1234567890abcdef";
    const curl = buildCurl(
      baseReq({
        url: "https://api.example.com",
        headers: [
          { key: "Authorization", value: authorization },
          { key: "Cookie", value: cookie },
          { key: "X-Debug-Token", value: token },
          { key: "X-Request-Id", value: "request-123" },
        ],
      }),
    );
    expect(curl).not.toContain(authorization);
    expect(curl).not.toContain(cookie);
    expect(curl).not.toContain(token);
    expect(curl).toContain("Authorization: [REDACTED]");
    expect(curl).toContain("Cookie: [REDACTED]");
    expect(curl).toContain("X-Debug-Token: [REDACTED]");
    expect(curl).toContain("X-Request-Id: request-123");
  });

  it("구조화 Cookie는 순서대로 한 header로 만들고 직접 값만 마스킹한다", () => {
    const curl = buildCurl(baseReq({
      url: "https://api.example.com",
      cookies: [
        { name: "session", value: "direct-cookie", enabled: true },
        { name: "token", value: "${COOKIE_TOKEN}", enabled: true },
        { name: "empty", value: "", enabled: true },
        { name: "skip", value: "disabled-secret", enabled: false },
      ],
    }));

    expect(curl).toContain("Cookie: session=[REDACTED]; token=${COOKIE_TOKEN}; empty=");
    expect(curl).not.toContain("direct-cookie");
    expect(curl).not.toContain("disabled-secret");
    expect(curl.match(/Cookie:/g)).toHaveLength(1);
  });

  it("raw Cookie header 충돌 또는 잘못된 구조화 Cookie는 cURL도 fail-closed한다", () => {
    expect(buildCurl(baseReq({
      url: "https://api.example.com",
      headers: [{ key: "Cookie", value: "legacy=one" }],
      cookies: [{ name: "session", value: "two" }],
    }))).toBe("");
    expect(buildCurl(baseReq({
      url: "https://api.example.com",
      cookies: [{ name: "bad name", value: "two" }],
    }))).toBe("");
  });

  it("body_kind가 none이면 body가 있어도 --data를 추가하지 않는다", () => {
    const curl = buildCurl(baseReq({ url: "https://api.example.com", body_kind: "none", body: "should-be-ignored" }));
    expect(curl).not.toContain("should-be-ignored");
  });
});
