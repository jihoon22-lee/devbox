import { describe, expect, it } from "vitest";
import { OPENAPI_DOCUMENT_LIMITS, parseBoundedOpenApiDocument } from "./index";

describe("parseBoundedOpenApiDocument", () => {
  it("normalizes JSON and YAML into null-prototype records", () => {
    for (const [text, format] of [
      ['{"openapi":"3.1.0","paths":{}}', "json"],
      ["openapi: 3.1.0\npaths: {}\n", "yaml"],
    ] as const) {
      const parsed = parseBoundedOpenApiDocument(text, format);
      expect(parsed.ok).toBe(true);
      if (parsed.ok) expect(Object.getPrototypeOf(parsed.value)).toBeNull();
    }
  });

  it("rejects empty, oversized, dangerous, aliased, and unsafe-number input", () => {
    expect(parseBoundedOpenApiDocument(" ", "yaml")).toEqual({
      ok: false,
      error: { code: "EMPTY_SOURCE" },
    });
    expect(parseBoundedOpenApiDocument("x".repeat(OPENAPI_DOCUMENT_LIMITS.maxBytes + 1), "yaml"))
      .toEqual({ ok: false, error: { code: "SOURCE_TOO_LARGE" } });
    expect(parseBoundedOpenApiDocument('{"constructor":{}}', "json"))
      .toEqual({ ok: false, error: { code: "DANGEROUS_KEY" } });
    expect(parseBoundedOpenApiDocument('{"value":9007199254740992}', "json"))
      .toEqual({ ok: false, error: { code: "PARSER_ERROR" } });
    const aliases = `root: &x { value: ok }\nitems:\n${"  - *x\n".repeat(OPENAPI_DOCUMENT_LIMITS.maxAliases + 1)}`;
    expect(parseBoundedOpenApiDocument(aliases, "yaml").ok).toBe(false);
  });
});
