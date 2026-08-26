import { describe, expect, it } from "vitest";
import {
  OPENAPI_LIMITS,
  parseOpenApi,
  parseOpenApiSource,
  selectOpenApiServer,
} from "./openapi";

const jsonFixture = JSON.stringify({
  openapi: "3.0.3",
  info: { title: "local fixture", version: "1" },
  servers: [{ url: "https://api.example.test/v1" }, { url: "https://staging.example.test" }],
  components: {
    securitySchemes: {
      bearerAuth: { type: "http", scheme: "bearer" },
      ignoredOauth: { type: "oauth2", flows: {} },
    },
  },
  security: [{ bearerAuth: [] }],
  paths: {
    "/users/{userId}": {
      parameters: [{ name: "userId", in: "path", required: true, example: "42" }],
      post: {
        parameters: [
          { name: "z", in: "query", example: "last" },
          { name: "a", in: "query", schema: { default: "first" } },
          { name: "X-Trace", in: "header", example: "trace-id" },
          { name: "session_token", in: "cookie", example: "DO_NOT_IMPORT_THIS_SECRET" },
        ],
        requestBody: {
          content: {
            "text/plain": { example: "do not import opaque raw body" },
            "application/json": {
              schema: {
                type: "object",
                properties: {
                  password: { example: "DO_NOT_IMPORT_THIS_PASSWORD" },
                  name: { example: "Ada" },
                },
              },
            },
          },
        },
      },
      get: {
        security: [{ ignoredOauth: [] }, {}],
      },
    },
    "/z": { get: {} },
    "/a": { get: {} },
  },
}, null, 2);

describe("parseOpenApi", () => {
  it("parses JSON and produces a preview without sending or injecting credentials", () => {
    const result = parseOpenApi(jsonFixture, "json");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.preview.operations.map((operation) => operation.label)).toEqual([
      "GET /a",
      "GET /users/{userId}",
      "POST /users/{userId}",
      "GET /z",
    ]);
    const post = result.preview.operations.find((operation) => operation.method === "POST");
    expect(post?.request.url).toBe("https://api.example.test/v1/users/42");
    expect(post?.request.params).toEqual([{ key: "a", value: "first" }, { key: "z", value: "last" }]);
    expect(post?.request.auth).toMatchObject({ kind: "bearer", token: "" });
    expect(post?.request.cookies).toEqual([{ name: "session_token", value: "", enabled: true }]);
    expect(post?.request.body_kind).toBe("json");
    expect(post?.request.body).toContain('"name": "Ada"');
    expect(post?.request.body).not.toContain("DO_NOT_IMPORT_THIS_PASSWORD");
    expect(JSON.stringify(post)).not.toContain("DO_NOT_IMPORT_THIS_SECRET");
    expect(post?.security?.valuesInjected).toBe(false);
  });

  it("parses YAML deterministically and keeps unsupported refs isolated to one operation", () => {
    const yaml = [
      "openapi: 3.1.0",
      "info:",
      "  title: fixture",
      "  version: '1'",
      "servers:",
      "  - url: https://example.test",
      "paths:",
      "  /ok:",
      "    get:",
      "      parameters: []",
      "  /ref:",
      "    get:",
      "      parameters:",
      "        - $ref: '#/components/parameters/Id'",
      "    post:",
      "      parameters: []",
      "  /malformed-ref:",
      "    get:",
      "      $ref: 42",
    ].join("\n");
    const result = parseOpenApi(yaml, "yaml");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.preview.version).toBe("3.1");
    expect(result.preview.operations.find((operation) => operation.path === "/ok")?.applyable).toBe(true);
    expect(result.preview.operations.find((operation) => operation.path === "/malformed-ref")?.errors[0]?.code).toBe("UNSUPPORTED_REF");
    expect(result.preview.operations.find((operation) => operation.path === "/ref" && operation.method === "GET")?.errors[0]?.code).toBe("UNSUPPORTED_REF");
    expect(result.preview.operations.find((operation) => operation.path === "/ref" && operation.method === "POST")?.applyable).toBe(true);
  });

  it("parses fetched URL text without retaining the source URL and rejects unsafe document graphs", () => {
    const remote = parseOpenApiSource({ kind: "url", format: "json", text: jsonFixture });
    expect(remote.ok).toBe(true);
    if (remote.ok) expect(remote.preview.sourceName).toBe("remote-openapi.json");
    expect(JSON.stringify(remote)).not.toContain("sourceUrl");

    const dangerous = parseOpenApi('{"__proto__":{"polluted":true}}', "json");
    expect(dangerous).toMatchObject({ ok: false, error: { code: "DANGEROUS_KEY" } });
    const unsafePath = parseOpenApi(JSON.stringify({
      openapi: "3.0.0",
      info: { title: "fixture", version: "1" },
      servers: [{ url: "https://example.test" }],
      paths: { "/../DO_NOT_REFLECT_THIS_PATH": { get: {} } },
    }), "json");
    expect(unsafePath.ok).toBe(true);
    if (unsafePath.ok) expect(JSON.stringify(unsafePath.preview.errors)).not.toContain("DO_NOT_REFLECT_THIS_PATH");
    const cyclicYaml = parseOpenApi("openapi: &root\n  openapi: *root\n", "yaml");
    expect(cyclicYaml.ok).toBe(false);
  });

  it("enforces byte and depth bounds before building a preview", () => {
    const oversized = parseOpenApi("x".repeat(OPENAPI_LIMITS.maxBytes + 1), "json");
    expect(oversized).toMatchObject({ ok: false, error: { code: "SOURCE_TOO_LARGE" } });
    const nested = `${"a:\n".repeat(OPENAPI_LIMITS.maxDepth + 2)}1`;
    const deep = parseOpenApi(nested, "yaml");
    expect(deep.ok).toBe(false);
    expect(["DEPTH_LIMIT", "PARSER_ERROR"]).toContain(deep.ok ? "" : deep.error.code);
  });

  it("selects another validated server without changing operation ordering", () => {
    const result = parseOpenApi(jsonFixture, "json");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    const selected = selectOpenApiServer(result.preview, 1);
    expect(selected.operations[2].request.url).toBe("https://staging.example.test/users/42");
    expect(selected.operations.map((operation) => operation.id)).toEqual(result.preview.operations.map((operation) => operation.id));
  });

  it("keeps path templates and server overrides fail-closed", () => {
    const result = parseOpenApi(JSON.stringify({
      openapi: "3.0.0",
      info: { title: "fixture", version: "1" },
      servers: [{ url: "https://example.test" }],
      paths: {
        "/missing/{id}": { get: {} },
        "/override": { servers: [{ url: "https://other.example.test" }], get: {} },
        "/safe": { get: {} },
      },
    }), "json");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.preview.operations.find((operation) => operation.path === "/missing/{id}")?.errors.map((entry) => entry.code)).toContain("PARAMETER_INVALID");
    expect(result.preview.operations.find((operation) => operation.path === "/override")?.errors.map((entry) => entry.code)).toContain("SERVER_OVERRIDE_UNSUPPORTED");
    expect(result.preview.operations.find((operation) => operation.path === "/safe")?.applyable).toBe(true);
    const selected = selectOpenApiServer(result.preview, 0);
    expect(selected.operations.find((operation) => operation.path === "/override")?.applyable).toBe(false);
  });

  it("does not carry control characters or encoded traversal into draft fields", () => {
    const result = parseOpenApi(JSON.stringify({
      openapi: "3.0.0",
      info: { title: "fixture", version: "1" },
      servers: [{ url: "https://example.test" }],
      paths: {
        "/safe": {
          get: {
            parameters: [
              { name: "X-Trace", in: "header", example: "line\nvalue" },
              { name: "key", in: "query", example: "DO_NOT_IMPORT" },
            ],
          },
        },
        "/%2e%2e/private": { get: {} },
      },
    }), "json");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    const safe = result.preview.operations.find((operation) => operation.path === "/safe");
    expect(safe?.request.headers).toEqual([{ key: "X-Trace", value: "", enabled: true }]);
    expect(safe?.request.params).toEqual([{ key: "key", value: "" }]);
    expect(safe?.parameters.find((parameter) => parameter.name === "key")).toMatchObject({ redacted: true, value: "" });
    expect(result.preview.errors.map((entry) => entry.code)).toContain("PATH_INVALID");
  });

  it("omits structured body examples containing control characters", () => {
    const result = parseOpenApi(JSON.stringify({
      openapi: "3.0.0",
      info: { title: "fixture", version: "1" },
      servers: [{ url: "https://example.test" }],
      paths: {
        "/safe": {
          post: {
            requestBody: {
              content: { "application/json": { example: { note: "line\nvalue" } } },
            },
          },
        },
      },
    }), "json");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.preview.operations[0].request.body).toBe("");
    expect(result.preview.operations[0].warnings.map((entry) => entry.code)).toContain("BODY_EXAMPLE_OMITTED");
  });

  it("redacts credential-shaped examples and rejects them from paths, servers, and filenames", () => {
    const rawCredential = "ghp_12345678901234567890";
    const result = parseOpenApiSource({
      kind: "file",
      name: `C:\\private\\${rawCredential}.json`,
      format: "json",
      text: JSON.stringify({
        openapi: "3.0.0",
        info: { title: "fixture", version: "1" },
        servers: [
          { url: `https://example.test/${rawCredential}` },
          { url: "https://safe.example.test" },
        ],
        paths: {
          [`/${rawCredential}`]: { get: {} },
          "/safe": {
            post: {
              parameters: [{ name: "q", in: "query", example: rawCredential }],
              requestBody: {
                content: {
                  "application/json": { example: { note: rawCredential, visible: "ok" } },
                },
              },
            },
          },
        },
      }),
    });
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.preview.sourceName).toBe("openapi.yaml");
    expect(result.preview.errors.map((entry) => entry.code)).toEqual(expect.arrayContaining(["SERVER_INVALID", "PATH_INVALID"]));
    const operation = result.preview.operations.find((candidate) => candidate.path === "/safe");
    expect(operation?.request.params).toEqual([{ key: "q", value: "" }]);
    expect(operation?.parameters[0]).toMatchObject({ redacted: true, value: "" });
    expect(operation?.request.body).toContain('"visible": "ok"');
    expect(operation?.request.body).not.toContain(rawCredential);
    expect(JSON.stringify(result)).not.toContain(rawCredential);
  });

  it("counts parsed scalar/key nodes", () => {
    // Keep the boundary fixture compact. An equivalent 25k-line block YAML
    // document made this security assertion contend with every workspace UI
    // suite in CI even though the normalized graph under test is identical.
    const values = Object.fromEntries(
      Array.from({ length: Math.floor(OPENAPI_LIMITS.maxNodes / 2) + 10 }, (_, index) => [`k${index}`, "v"]),
    );
    expect(parseOpenApi(JSON.stringify({
      openapi: "3.0.0",
      info: { title: "fixture", version: "1" },
      paths: {},
      values,
    }), "json")).toMatchObject({ ok: false, error: { code: "NODE_LIMIT" } });
  });

  it("bounds multipart drafts", () => {
    const properties = Object.fromEntries(
      Array.from({ length: 51 }, (_, index) => [`field${index}`, { example: `value${index}` }]),
    );
    const multipart = parseOpenApi(JSON.stringify({
      openapi: "3.0.0",
      info: { title: "fixture", version: "1" },
      servers: [{ url: "https://example.test" }],
      paths: {
        "/upload": {
          post: {
            requestBody: {
              content: { "multipart/form-data": { schema: { type: "object", properties } } },
            },
          },
        },
      },
    }), "json");
    expect(multipart.ok).toBe(true);
    if (!multipart.ok) return;
    expect(multipart.preview.operations[0].request.multipart).toEqual([]);
    expect(multipart.preview.operations[0].warnings.map((entry) => entry.code)).toContain("BODY_TOO_LARGE");
  });

  it("sanitizes only the file basename in a source preview", () => {
    const result = parseOpenApiSource({
      kind: "file",
      name: "C:\\private\\secrets\\api.yaml",
      text: "openapi: 3.0.0\ninfo:\n  title: fixture\n  version: '1'\npaths: {}\n",
    });
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.preview.sourceName).toBe("api.yaml");
    expect(JSON.stringify(result.preview)).not.toContain("private");
  });

  it("bounds generated request rows after body and auth rows are added", () => {
    const parameters = Array.from({ length: OPENAPI_LIMITS.maxRequestRows }, (_, index) => ({
      name: `header-${index}`,
      in: "header",
    }));
    const result = parseOpenApi(JSON.stringify({
      openapi: "3.0.0",
      info: { title: "fixture", version: "1" },
      servers: [{ url: "https://example.test" }],
      components: { securitySchemes: { key: { type: "apiKey", in: "header", name: "X-API-Key" } } },
      paths: {
        "/bounded": {
          get: {
            parameters,
            security: [{ key: [] }],
            requestBody: { content: { "application/json": { example: { value: "ok" } } } },
          },
        },
      },
    }), "json");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    const operation = result.preview.operations[0];
    expect(operation.request.headers).toHaveLength(OPENAPI_LIMITS.maxRequestRows);
    expect(operation.errors.map((entry) => entry.code)).toContain("REQUEST_ROW_LIMIT");
    expect(operation.applyable).toBe(false);
  });

  it("does not import environment references as executable secret values", () => {
    const result = parseOpenApi(JSON.stringify({
      openapi: "3.0.0",
      info: { title: "fixture", version: "1" },
      servers: [{ url: "https://example.test" }],
      paths: { "/safe": { get: { parameters: [{ name: "q", in: "query", example: "${API_TOKEN}" }] } } },
    }), "json");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.preview.operations[0].request.params).toEqual([{ key: "q", value: "" }]);
    expect(result.preview.operations[0].parameters[0]).toMatchObject({ redacted: true, value: "" });
  });
});
