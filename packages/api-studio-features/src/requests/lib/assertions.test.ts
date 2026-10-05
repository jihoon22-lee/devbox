import { afterEach, describe, expect, it, vi } from "vitest";
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
  it("checks status, headers, JSON values and duration", async () => {
    const results = await evaluateAssertions(
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

  it("reports actual values and reasons on failure", async () => {
    const [status, regex, numeric] = await evaluateAssertions(
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

  it("fails JSON checks on non-JSON and binary responses without stopping others", async () => {
    const text = await evaluateAssertions(
      [a("jsonPath", "exists", "", "$.a"), a("status", "equals", "201")],
      response({ is_json: false, body: "<html>" }),
    );
    expect(text.map((r) => [r.passed, r.message])).toEqual([
      [false, "JSON이 아닌 응답"],
      [true, ""],
    ]);
    const binary = await evaluateAssertions(
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

  it("skips disabled assertions", async () => {
    expect(await evaluateAssertions([{ ...a("status", "equals", "500"), enabled: false }], response())).toEqual([]);
  });
});

const workers: Array<{
  terminate: ReturnType<typeof vi.fn>;
  postMessage: ReturnType<typeof vi.fn>;
  onmessage?: (event: { data: unknown }) => void;
}> = [];
function installWorker() {
  vi.stubGlobal(
    "Worker",
    class {
      terminate = vi.fn();
      postMessage = vi.fn();
      constructor() {
        workers.push(this);
      }
    },
  );
}
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  workers.length = 0;
});
it("bounds regex assertion work without blocking other assertions", async () => {
  installWorker();
  vi.useFakeTimers();
  const task = evaluateAssertions([a("body", "matches", "safe"), a("status", "equals", "201")], response());
  expect(workers).toHaveLength(1);
  await vi.advanceTimersByTimeAsync(500);
  expect(await task).toEqual([
    expect.objectContaining({ passed: false, message: expect.stringContaining("제한 시간") }),
    expect.objectContaining({ passed: true }),
  ]);
  expect(workers[0].terminate).toHaveBeenCalledOnce();
});
it("terminates regex matching on cancellation and ignores stale replies", async () => {
  installWorker();
  const controller = new AbortController();
  const task = evaluateAssertions([a("body", "matches", "safe")], response(), controller.signal);
  expect(workers).toHaveLength(1);
  controller.abort();
  workers[0].onmessage?.({ data: { matched: true } });
  expect((await task)[0]).toMatchObject({ passed: false });
  expect(workers[0].terminate).toHaveBeenCalledOnce();
});
it("reports a worker regex match and cleans up ownership", async () => {
  installWorker();
  const task = evaluateAssertions([a("body", "matches", "kim")], response());
  expect(workers).toHaveLength(1);
  workers[0].onmessage?.({ data: { matched: true } });
  expect((await task)[0]).toMatchObject({ passed: true });
  expect(workers[0].terminate).toHaveBeenCalledOnce();
});
