import { describe, expect, it } from "vitest";
import type { HistoryItem, RequestTemplate } from "../types";
import { filterHistory, historyDisplayLabel, historyMethod, MAX_HISTORY_QUERY_CHARS } from "./history";
import { parseHistoryStore } from "./persistence";

function item(id: string, url: string, status?: number, name?: string): HistoryItem {
  const request: RequestTemplate = {
    method: id === "two" ? "POST" : "GET",
    url,
    headers: [],
    cookies: [],
    multipart: [],
    params: [],
    body_kind: "none",
    body: "",
    auth: null,
    timeout_ms: 10_000,
  };
  return {
    id,
    name,
    saved_at: 1,
    request: { ...request, requiresSecretReview: false },
    status,
  };
}

describe("history filter", () => {
  const history = [
    item("one", "https://api.example.com/users", 200, "Users"),
    item("two", "https://api.example.com/login", 401, "Login"),
    item("three", "https://api.example.com/health"),
  ];

  it("searches safe name/url/method metadata and filters status", () => {
    expect(filterHistory(history, { query: "users", method: "", status: "all" }).map((entry) => entry.id)).toEqual(["one"]);
    expect(filterHistory(history, { query: "", method: "POST", status: "error" }).map((entry) => entry.id)).toEqual(["two"]);
    expect(filterHistory(history, { query: "", method: "", status: "success" }).map((entry) => entry.id)).toEqual(["one"]);
    expect(filterHistory(history, { query: "", method: "", status: "error" }).map((entry) => entry.id)).toEqual(["two", "three"]);
  });

  it("does not inspect request body/header secrets and bounds the query", () => {
    const secret = item("secret", "https://api.example.com/safe", 200);
    secret.request.headers = [{ key: "Authorization", value: "header-secret" }];
    secret.request.body = "body-secret";
    expect(filterHistory([secret], { query: "header-secret", method: "", status: "all" })).toEqual([]);
    expect(filterHistory([secret], { query: "x".repeat(MAX_HISTORY_QUERY_CHARS + 20), method: "", status: "all" })).toEqual([]);
  });

  it("redacts a manually edited sensitive URL before searching or displaying it", () => {
    const unsafe = item("unsafe", "https://api.example.com/safe?token=direct-secret", 200);
    expect(filterHistory([unsafe], { query: "direct-secret", method: "", status: "all" })).toEqual([]);
    expect(historyDisplayLabel(unsafe)).toContain("REDACTED");
    expect(historyDisplayLabel(unsafe)).not.toContain("direct-secret");
  });

  it("projects manually edited method/name/url metadata into bounded safe values", () => {
    const source = item("unsafe", "https://example.test/" + "x".repeat(800), 700, "ghp_1234567890abcdef\u0000name");
    source.request.method = "sk_1234567890abcdef";
    const malformed = item("malformed", "https://example.test/path", Number.POSITIVE_INFINITY, "ok");
    malformed.request.method = "GET\u0000POST";
    const parsed = parseHistoryStore(JSON.stringify({ version: 2, history: [source, malformed] }));

    expect(parsed?.history).toHaveLength(2);
    expect(parsed?.history[0].request.method).toBe("UNKNOWN");
    expect(historyMethod(parsed!.history[0])).toBe("UNKNOWN");
    expect(parsed?.history[0].name).toContain("[REDACTED]");
    expect(parsed?.history[0].name).not.toMatch(/[\u0000-\u001f\u007f-\u009f\u2028\u2029]/u);
    expect(parsed?.history[0].request.url.length).toBeLessThanOrEqual(513);
    expect(parsed?.history[1].request.method).toBe("UNKNOWN");
    expect(parsed?.history[1].status).toBeUndefined();
  });

  it("redacts token-shaped URL metadata before method options and search", () => {
    const unsafe = item("token-url", "https://example.test/path/ghp_1234567890abcdef", 200, "safe");
    const parsed = parseHistoryStore(JSON.stringify({ version: 2, history: [unsafe] }));
    expect(parsed?.history[0].request.url).not.toContain("ghp_1234567890abcdef");
    expect(filterHistory(parsed?.history ?? [], {
      query: "ghp_1234567890abcdef",
      method: "",
      status: "all",
    })).toEqual([]);
  });
});
