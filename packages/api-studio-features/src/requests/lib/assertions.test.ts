import { describe, expect, it } from "vitest";
import { evaluateAssertions, type Assertion } from "./assertions";
import type { ApiResponse } from "../types";

function response(overrides: Partial<ApiResponse> = {}): ApiResponse {
  return {
    status: 201,
    status_text: "Created",
    headers: [
      { key: "Content-Type", value: "application/json" },
      { key: "X-Id", value: "42" },
    ],
    duration_ms: 120,
    size_bytes: 20,
    body: '{"user":{"id":7,"name":"kim"}}',
    is_json: true,
    final_url: "https://x.test",
    redirects: [],
    cookies: [],
    response_id: null,
    raw_headers_available: false,
    headers_truncated: false,
    ...overrides,
  };
}
const a = (source: Assertion["source"], operator: Assertion["operator"], expected = "", target = ""): Assertion => ({
  id: `${source}-${operator}-${target}`,
  enabled: true,
  source,
  target,
  operator,
  expected,
});

describe("assertions", () => {
  it("checks status, headers, JSON values and duration", () => {
    const results = evaluateAssertions(
      [
        a("status", "equals", "201"),
        a("header", "equals", "42", "x-id"),
        a("jsonPath", "equals", "7", "$.user.id"),
        a("jsonPath", "exists", "", "$.user.name"),
        a("jsonPath", "notExists", "", "$.user.email"),
        a("body", "contains", "kim"),
        a("duration", "lessThan", "500"),
      ],
      response(),
    );
    expect(results.map((r) => r.passed)).toEqual([true, true, true, true, true, true, true]);
  });

  it("reports actual values and reasons on failure", () => {
    const [status, regex, numeric] = evaluateAssertions(
      [
        a("status", "equals", "200"),
        a("body", "matches", "(unclosed"),
        a("jsonPath", "greaterThan", "10", "$.user.name"),
      ],
      response(),
    );
    expect(status).toMatchObject({ passed: false, actual: "201" });
    expect(regex).toMatchObject({ passed: false, message: "정규식이 올바르지 않습니다" });
    expect(numeric).toMatchObject({ passed: false, message: "숫자가 아닌 값입니다" });
  });

  it("fails JSON checks on non-JSON and binary responses without stopping others", () => {
    const text = evaluateAssertions(
      [a("jsonPath", "exists", "", "$.a"), a("status", "equals", "201")],
      response({ is_json: false, body: "<html>" }),
    );
    expect(text.map((r) => [r.passed, r.message])).toEqual([
      [false, "JSON이 아닌 응답"],
      [true, ""],
    ]);
    const binary = evaluateAssertions(
      [a("body", "contains", "x")],
      response({
        binary: {
          media_type: "image/png",
          size_bytes: 3,
          hex_preview: "00",
          hex_truncated: false,
        } as ApiResponse["binary"],
      }),
    );
    expect(binary[0]).toMatchObject({ passed: false, message: "binary 응답" });
  });

  it("skips disabled assertions", () => {
    expect(evaluateAssertions([{ ...a("status", "equals", "500"), enabled: false }], response())).toEqual([]);
  });
});
