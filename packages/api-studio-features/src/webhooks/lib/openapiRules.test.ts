import { describe, expect, it } from "vitest";
import { openApiOperationToRule, previewOpenApiRules } from "./openapiRules";

describe("OpenAPI rule drafts", () => {
  it("creates data-only drafts and selects the lowest documented 2xx status", () => {
    const result = previewOpenApiRules(JSON.stringify({
      openapi: "3.1.0",
      servers: [{ url: "https://user:secret@example.test" }],
      paths: {
        "/hook": {
          post: {
            security: [{ bearer: [] }],
            requestBody: { content: { "application/json": { example: { token: "private" } } } },
            responses: { "204": { description: "ok" }, "201": { description: "created" } },
          },
        },
      },
    }), "json", "C:\\private\\hooks.json");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.preview.sourceName).toBe("hooks.json");
    expect(result.preview.operations).toEqual([{
      id: "post:/hook",
      method: "POST",
      path: "/hook",
      status: 201,
      applyable: true,
      reason: null,
    }]);
    const draft = openApiOperationToRule(result.preview.operations[0]);
    expect(draft).toEqual({
      id: "",
      priority: 0,
      method: "POST",
      path: "/hook",
      status: 201,
      headers: [],
      body: "",
      delayMs: 0,
      sequence: [],
    });
    const serialized = JSON.stringify(result.preview);
    for (const privateValue of ["user:secret", "bearer", "token", "private", "example.test"]) {
      expect(serialized).not.toContain(privateValue);
    }
  });

  it("shows parameterized paths but never silently turns them into wildcards", () => {
    const result = previewOpenApiRules(`
openapi: 3.0.3
paths:
  /events/{id}:
    get:
      responses:
        default: { description: fallback }
`, "yaml", "api.yaml");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.preview.operations[0]).toMatchObject({
      path: "/events/{id}",
      status: 200,
      applyable: false,
      reason: "pathParametersUnsupported",
    });
    expect(openApiOperationToRule(result.preview.operations[0])).toBeNull();
  });

  it("isolates invalid operations and rejects unsupported documents", () => {
    const preview = previewOpenApiRules(JSON.stringify({
      openapi: "3.0.0",
      paths: { "/hook": { post: { $ref: "#/private" }, get: null } },
    }), "json", "api.json");
    expect(preview.ok).toBe(true);
    if (preview.ok) {
      expect(preview.preview.operations.map((operation) => operation.reason))
        .toEqual(["operationInvalid", "referenceUnsupported"]);
    }
    expect(previewOpenApiRules("openapi: 2.0\npaths: {}", "yaml", "api.yaml"))
      .toMatchObject({ ok: false, code: "VERSION_UNSUPPORTED" });
    expect(previewOpenApiRules("{", "json", "api.json"))
      .toMatchObject({ ok: false, code: "DOCUMENT_INVALID" });
  });

  it("marks paths outside the ASCII Webhook matcher contract as non-applyable", () => {
    const preview = previewOpenApiRules(JSON.stringify({
      openapi: "3.1.0",
      paths: { "/이벤트": { post: { responses: { "200": {} } } } },
    }), "json", "api.json");
    expect(preview.ok).toBe(true);
    if (!preview.ok) return;
    expect(preview.preview.operations[0]).toMatchObject({
      path: "/이벤트",
      applyable: false,
      reason: "pathUnsupported",
    });
  });
});
