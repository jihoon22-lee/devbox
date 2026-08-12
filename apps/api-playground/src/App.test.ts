import { describe, expect, it } from "vitest";
import { buildCurl, shellQuote, statusClass, tryPretty } from "./App";
import type { ApiRequest } from "./types";

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

function baseReq(overrides: Partial<ApiRequest> = {}): ApiRequest {
  return {
    method: "GET",
    url: "",
    headers: [],
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

  it("basic auth는 Authorization: Basic 헤더를 base64로 추가한다", () => {
    const curl = buildCurl(
      baseReq({
        url: "https://api.example.com",
        auth: { kind: "basic", username: "user", password: "pass", token: "", api_key: "", api_value: "" },
      }),
    );
    expect(curl).toContain(`Authorization: Basic ${btoa("user:pass")}`);
  });

  it("bearer auth는 Authorization: Bearer 헤더를 추가한다", () => {
    const curl = buildCurl(
      baseReq({
        url: "https://api.example.com",
        auth: { kind: "bearer", username: "", password: "", token: "tok123", api_key: "", api_value: "" },
      }),
    );
    expect(curl).toContain("Authorization: Bearer tok123");
  });

  it("apikey auth는 지정한 헤더 이름으로 추가한다", () => {
    const curl = buildCurl(
      baseReq({
        url: "https://api.example.com",
        auth: { kind: "apikey", username: "", password: "", token: "", api_key: "X-API-Key", api_value: "secret" },
      }),
    );
    expect(curl).toContain("X-API-Key: secret");
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

  it("body_kind가 none이면 body가 있어도 --data를 추가하지 않는다", () => {
    const curl = buildCurl(baseReq({ url: "https://api.example.com", body_kind: "none", body: "should-be-ignored" }));
    expect(curl).not.toContain("should-be-ignored");
  });
});
