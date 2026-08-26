import { describe, expect, it } from "vitest";
import type { ResponseRule } from "../api";
import {
  MAX_EXAMPLE_BODY_CHARS,
  MAX_EXAMPLE_DELAY_MS,
  MAX_EXAMPLE_HEADER_COUNT,
  MAX_EXAMPLE_HEADER_TOTAL_CHARS,
  MAX_EXAMPLE_HEADER_VALUE_CHARS,
  MAX_EXAMPLE_JSON_DEPTH,
  MAX_EXAMPLE_JSON_NODES,
  MAX_EXAMPLE_JSON_STRING_CHARS,
  MAX_EXAMPLE_PATH_CHARS,
  MAX_EXAMPLE_STATUS,
  MIN_EXAMPLE_STATUS,
  buildExampleCurl,
  posixShellQuote,
  powershellQuote,
  shellQuote,
  type CurlShell,
} from "./exampleCurl";

function rule(overrides: Partial<ResponseRule> = {}): ResponseRule {
  return {
    id: "rule-1",
    method: "POST",
    path: "/events/*",
    status: 201,
    headers: [
      ["Content-Type", "application/json"],
      ["Authorization", "Bearer header-secret"],
      ["X-Trace", "it's safe"],
      ["X-Reference", "${TRACE_TOKEN}"],
    ],
    body: JSON.stringify({
      ok: true,
      token: "body-secret",
      reference: "${BODY_TOKEN}",
      mixed: "prefix ${MIXED_TOKEN}",
    }),
    delayMs: 25,
    ...overrides,
  };
}

describe("Webhook Lab example curl", () => {
  it("renders a deterministic POSIX golden with a concrete trailing-star sample", () => {
    expect(buildExampleCurl(rule(), "127.0.0.1:9000", "posix")).toBe(
      [
        "curl --globoff --path-as-is --include --request POST 'http://127.0.0.1:9000/events/example'",
        "",
        "# Webhook Lab response metadata (not request data): status 201, delay 25ms",
        "# Response headers:",
        "# Content-Type: application/json",
        "# Authorization: [REDACTED]",
        "# X-Trace: it's safe",
        "# X-Reference: ${TRACE_TOKEN}",
        '# Response body: "{\\"ok\\":true,\\"token\\":\\"[REDACTED]\\",\\"reference\\":\\"${BODY_TOKEN}\\",\\"mixed\\":\\"[REDACTED]\\"}"',
        "# Concrete trailing-* sample path: /events/example",
      ].join("\n"),
    );
  });

  it("renders the PowerShell curl.exe golden independently from POSIX quoting", () => {
    expect(buildExampleCurl(rule({ path: "/hook", headers: [], body: "" }), "0.0.0.0:9100", "powershell")).toBe(
      [
        "curl.exe --globoff --path-as-is --include --request POST 'http://127.0.0.1:9100/hook'",
        "",
        "# Webhook Lab response metadata (not request data): status 201, delay 25ms",
        "# Response headers:",
        "# (none)",
        '# Response body: ""',
      ].join("\n"),
    );
  });

  it("uses shell-specific single-quote escaping without command substitution", () => {
    const value = "it's $HOME $(Get-Date)";
    expect(posixShellQuote(value)).toBe("'it'\\''s $HOME $(Get-Date)'");
    expect(powershellQuote(value)).toBe("'it''s $HOME $(Get-Date)'");
    expect(shellQuote(value)).toBe(posixShellQuote(value));
  });

  it("keeps response metadata out of request arguments and masks mixed placeholders", () => {
    const curl = buildExampleCurl(rule({
      headers: [
        ["X-Exact", "${TOKEN}"],
        ["X-Mixed", "prefix ${TOKEN}"],
        ["Authorization", "Bearer ${TOKEN}"],
      ],
      body: JSON.stringify({ safe: "prefix ${TOKEN}", token: "${TOKEN}" }),
    }), "127.0.0.1:9000");

    expect(curl).toContain("# X-Exact: ${TOKEN}");
    expect(curl).toContain("# X-Mixed: [REDACTED]");
    expect(curl).toContain("# Authorization: [REDACTED]");
    expect(curl).toContain('\\"safe\\":\\"[REDACTED]\\"');
    expect(curl).toContain('\\"token\\":\\"${TOKEN}\\"');
    expect(curl).not.toContain("prefix ${TOKEN}");
    expect(curl).not.toContain("Bearer ${TOKEN}");
    expect(curl).not.toContain(" --header ");
    expect(curl).not.toContain(" --data ");
    expect(curl).toContain("--path-as-is");
    expect(curl).toContain("--include");
  });

  it("fails closed when masking a sensitive query would change the exact route", () => {
    expect(buildExampleCurl(rule({ path: "/hook?token=direct-url-secret" }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ path: "/hook?token=${TOKEN}" }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ path: "/hook/ghp_12345678901234567890" }), "127.0.0.1:9000")).toBeNull();
  });

  it("accepts loopback and normalizes IPv4/IPv6 wildcard bind destinations", () => {
    expect(buildExampleCurl(rule({ path: "/hook" }), "127.0.0.1:9000")).toContain("http://127.0.0.1:9000/hook");
    expect(buildExampleCurl(rule({ path: "/hook" }), "0.0.0.0:9000")).toContain("http://127.0.0.1:9000/hook");
    expect(buildExampleCurl(rule({ path: "/hook" }), "[::1]:9000")).toContain("http://[::1]:9000/hook");
    expect(buildExampleCurl(rule({ path: "/hook" }), "[::]:9000")).toContain("http://[::1]:9000/hook");
    expect(buildExampleCurl(rule({ path: "/hook" }), "192.168.0.5:9000")).toBeNull();
    expect(buildExampleCurl(rule({ path: "/hook" }), "::1:9000")).toBeNull();
  });

  it("rejects unsafe URI grammar and curl-normalizing inputs", () => {
    expect(buildExampleCurl(rule({ path: "//external.example/hook" }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ path: "/hook with-space" }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ path: "/hook%ZZ" }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ path: "/hook#fragment" }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ path: "/hook\nnext" }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ path: "/hook%20with-space" }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ path: "/%2F%2Fexternal" }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ path: "/hook?x=%0A" }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ path: "/hook?x=%24%7BTOKEN%7D" }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ path: "/hook?x=ghp_12345678901234567890" }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ path: "/hook/%67%68%70_12345678901234567890" }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ path: "/hook/../secret" }), "127.0.0.1:9000")).toContain("--path-as-is");
  });

  it("enforces path/header/body/JSON bounds and catches invalid builder inputs", () => {
    const deep: unknown = Array.from({ length: MAX_EXAMPLE_JSON_DEPTH + 2 }).reduce<unknown>(
      (value) => ({ child: value }),
      "ok",
    );
    expect(buildExampleCurl(rule({ path: "/".padEnd(MAX_EXAMPLE_PATH_CHARS + 1, "x") }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ headers: Array.from({ length: MAX_EXAMPLE_HEADER_COUNT + 1 }, (_, index) => [`X-${index}`, "v"] as [string, string]) }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ headers: [["X-Large", "x".repeat(MAX_EXAMPLE_HEADER_VALUE_CHARS + 1)]] }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ headers: Array.from({ length: 5 }, (_, index) => [`X-${index}`, "x".repeat(Math.floor(MAX_EXAMPLE_HEADER_TOTAL_CHARS / 4))] as [string, string]) }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ body: "x".repeat(MAX_EXAMPLE_BODY_CHARS + 1) }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ body: JSON.stringify(deep) }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ body: JSON.stringify(Array.from({ length: MAX_EXAMPLE_JSON_NODES + 1 }, () => 0)) }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ body: JSON.stringify("x".repeat(MAX_EXAMPLE_JSON_STRING_CHARS + 1)) }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ body: JSON.stringify({ "${TOKEN}": "value" }) }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ body: JSON.stringify({ ["x".repeat(MAX_EXAMPLE_JSON_STRING_CHARS + 1)]: "value" }) }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ body: "token: secret with a suffix" }), "127.0.0.1:9000")).toContain('# Response body: "[REDACTED]"');
    expect(buildExampleCurl(rule({ headers: [["X-Bad", "value", "extra"] as unknown as [string, string]] }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ status: MIN_EXAMPLE_STATUS - 1 }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ status: MAX_EXAMPLE_STATUS + 1 }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ delayMs: -1 }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule({ delayMs: MAX_EXAMPLE_DELAY_MS + 1 }), "127.0.0.1:9000")).toBeNull();
    expect(buildExampleCurl(rule(), "127.0.0.1:9000", "cmd" as unknown as CurlShell)).toBeNull();
    expect(buildExampleCurl({} as ResponseRule, "127.0.0.1:9000")).toBeNull();
  });

  it("uses POST for an any-method rule and keeps an empty body explicit", () => {
    const curl = buildExampleCurl(rule({ method: null, path: "/hook", headers: [], body: "" }), "[::1]:9000");
    expect(curl).toContain("curl --globoff --path-as-is --include --request POST 'http://[::1]:9000/hook'");
    expect(curl).toContain("# Response headers:\n# (none)");
    expect(curl).toContain('# Response body: ""');
    expect(curl).toContain("# Rule method is any; this example uses POST.");
  });
});
