import { describe, expect, it } from "vitest";
import { generateCode, type CodeTarget } from "./codegen";
import { emptyRequest } from "./importers";
import type { RequestTemplate } from "../types";
const targets: CodeTarget[] = ["curl", "fetch", "python", "go", "csharp"];
const env = [
  { key: "baseUrl", value: "https://api.x.test", secret: false },
  { key: "token", value: "sealed-blob", secret: true },
];
const post: RequestTemplate = {
  ...emptyRequest(),
  method: "POST",
  url: "{{baseUrl}}/notes",
  headers: [
    { key: "X-Note", value: `it's "quoted"\nline 한글`, enabled: true },
    { key: "X-Off", value: "disabled", enabled: false },
  ],
  params: [{ key: "q", value: "한글 값" }],
  body_kind: "json",
  body: '{"title":"안녕"}',
  auth: { kind: "bearer", username: "", password: "", token: "{{token}}", api_key: "", api_value: "" },
};
describe("code generation", () => {
  it("resolves public values and escapes shell arguments while preserving secret placeholders", () => {
    const { code, placeholders } = generateCode("curl", post, env);
    expect(placeholders).toEqual(["token"]);
    expect(code).toContain("# 채워 넣을 값: {{token}}");
    expect(code).toContain("curl -X POST 'https://api.x.test/notes?q=%ED%95%9C%EA%B8%80%20%EA%B0%92'");
    expect(code).toContain(`-H 'X-Note: it'\\''s "quoted" line 한글'`);
    expect(code).toContain("-H 'Authorization: Bearer {{token}}'");
    expect(code).toContain('--data-raw \'{"title":"안녕"}\'');
    expect(code).not.toContain("X-Off");
  });
  it("uses valid string literals for every target", () => {
    expect(generateCode("fetch", post, env).code).toContain('"X-Note", "it\'s \\"quoted\\" line 한글"');
    expect(generateCode("python", post, env).code).toContain('"X-Note": "it\'s \\"quoted\\" line 한글"');
    expect(generateCode("go", post, env).code).toContain('req.Header.Add("X-Note", "it\'s \\"quoted\\" line 한글")');
    expect(generateCode("csharp", post, env).code).toContain(
      'request.Headers.TryAddWithoutValidation("X-Note", "it\'s \\"quoted\\" line 한글");',
    );
  });
  it("never exports sealed values, literal credentials, private file paths or redaction markers", () => {
    const req = {
      ...post,
      body: '{"password":"literal-secret","k":"[REDACTED]"}',
      auth: { ...post.auth!, token: "literal-token" },
    };
    for (const target of targets) {
      const { code, placeholders } = generateCode(target, req, env);
      for (const value of ["sealed-blob", "literal-secret", "literal-token", "[REDACTED]"])
        expect(code).not.toContain(value);
      expect(placeholders).toContain("REDACTED_VALUE");
    }
  });
  it("emits OAuth access-token placeholders without configuration or cache secrets", () => {
    for (const target of targets) {
      const result = generateCode(
        target,
        { ...post, auth: { ...post.auth!, kind: "oauth2", token: "unused-secret" } },
        env,
      );
      expect(result.code).toContain("Bearer {{access_token}}");
      expect(result.code).not.toContain("unused-secret");
      expect(result.placeholders).toContain("access_token");
    }
  });
  it("keeps duplicate headers and ignores disabled multipart parts", () => {
    const req: RequestTemplate = {
      ...emptyRequest(),
      url: "https://x.test",
      method: "POST",
      body_kind: "multipart",
      headers: [
        { key: "X-Many", value: "1" },
        { key: "X-Many", value: "2" },
      ],
      multipart: [
        { kind: "text", name: "note", value: "한글", file_path: "", file_name: "", content_type: "", enabled: true },
        {
          kind: "file",
          name: "upload",
          value: "",
          file_path: "/private/file.txt",
          file_name: "file.txt",
          content_type: "text/plain",
          enabled: true,
        },
        {
          kind: "text",
          name: "off",
          value: "never-include",
          file_path: "",
          file_name: "",
          content_type: "",
          enabled: false,
        },
      ],
    };
    for (const target of targets) {
      const result = generateCode(target, req, []);
      expect(result.code).not.toContain("/private/file.txt");
      expect(result.code).not.toContain("never-include");
      expect(result.code).toContain("{{file:upload}}");
      expect(result.placeholders).toContain("file:upload");
      expect(result.code).toContain("한글");
    }
    expect(generateCode("curl", req, []).code.match(/X-Many/g)).toHaveLength(2);
  });
  it("projects form, cookies and GraphQL as transmitted bodies", () => {
    const form = {
      ...emptyRequest(),
      method: "POST",
      url: "https://x.test/#fragment",
      body_kind: "form",
      body: "name=한 글\ncount=2",
      cookies: [{ name: "session", value: "{{session}}", enabled: true }],
    };
    const result = generateCode("curl", form, []);
    expect(result.code).toContain("name=%ED%95%9C+%EA%B8%80&count=2");
    expect(result.code).toContain("Cookie: session={{session}}");
    const graphql = {
      ...emptyRequest(),
      method: "POST",
      url: "https://x.test/",
      body_kind: "graphql",
      graphql: { query: "query Q { viewer { id } }", variables: "{}", operation_name: "Q" },
    };
    expect(generateCode("curl", graphql, []).code).toContain('"operationName":"Q"');
    expect(generateCode("curl", { ...graphql, method: "GET" }, []).code).toContain("query=");
    expect(generateCode("curl", { ...graphql, method: "GET" }, []).code).not.toContain("--data-raw");
  });
  it("preserves TLS intent and requires external PEM paths instead of leaking native credentials", () => {
    const req = { ...post, tls: { credentialId: "a".repeat(32), verify: false } };
    expect(generateCode("curl", req, env).code).toContain("--insecure");
    expect(generateCode("python", req, env).code).toContain("verify=False");
    expect(generateCode("go", req, env).code).toContain("InsecureSkipVerify: true");
    expect(generateCode("csharp", req, env).code).toContain("DangerousAcceptAnyServerCertificateValidator");
    expect(generateCode("fetch", req, env).code).toContain("throw new Error");
    for (const target of ["curl", "python", "go", "csharp"] as const) {
      const result = generateCode(target, req, env);
      expect(result.code).toContain("{{client_cert_pem}}");
      expect(result.code).not.toContain("a".repeat(32));
    }
  });
});

it("lists only referenced secrets and preserves multipart media types", () => {
  expect(generateCode("curl", { ...emptyRequest(), url: "https://x.test/" }, env).placeholders).toEqual([]);
  const req = {
    ...emptyRequest(),
    method: "POST",
    url: "https://x.test/",
    body_kind: "multipart",
    multipart: [
      {
        kind: "text",
        name: "payload",
        value: '{"hello":"world"}',
        file_path: "",
        file_name: "",
        content_type: "application/json",
        enabled: true,
      },
    ],
  };
  for (const target of targets) expect(generateCode(target, req, []).code).toContain("application/json");
});
it("does not follow redirects with client identity", () => {
  const req = { ...post, tls: { credentialId: "a".repeat(32), verify: true } };
  expect(generateCode("python", req, env).code).toContain("allow_redirects=False");
  expect(generateCode("go", req, env).code).toContain("http.ErrUseLastResponse");
  expect(generateCode("csharp", req, env).code).toContain("AllowAutoRedirect = false");
});
it("does not leak sealed bytes inserted through a public variable and sends Python UTF-8 bytes", () => {
  const request = { ...post, body_kind: "raw", body: "{{public}}" };
  for (const target of targets) {
    const result = generateCode(target, request, [...env, { key: "public", value: "sealed-blob", secret: false }]);
    expect(result.code).not.toContain("sealed-blob");
  }
  expect(generateCode("python", post, env).code).toContain('.encode("utf-8")');
});
it("keeps query and form placeholders directly editable", () => {
  const req = {
    ...emptyRequest(),
    method: "POST",
    url: "https://x.test/",
    params: [{ key: "q", value: "{{missing}}" }],
    body_kind: "form",
    body: "key={{value}}",
  };
  const code = generateCode("curl", req, []).code;
  expect(code).toContain("q={{missing}}");
  expect(code).toContain("key={{value}}");
});
it("loads a C# trust certificate without requiring its private key", () => {
  const request = { ...post, tls: { credentialId: "a".repeat(32), verify: true } };
  const code = generateCode("csharp", request, env).code;
  expect(code).toContain('X509Certificate2.CreateFromPem(File.ReadAllText("{{ca_pem}}"))');
  expect(code).not.toContain('X509Certificate2.CreateFromPemFile("{{ca_pem}}")');
  expect(code).toContain("X509CertificateLoader.LoadPkcs12");
  expect(code).toContain("CryptographicOperations.ZeroMemory");
});
