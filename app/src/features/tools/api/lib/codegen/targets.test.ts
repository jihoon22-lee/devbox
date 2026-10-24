import { expect, it } from "vitest";
import { emptyRequest } from "../importers";
import type { RequestTemplate } from "../template";
import { CODE_TARGETS, generateCode } from "./index";
import { rustString } from "./rust";

// S5-3 새 시험: v1 대상 axios·Rust reqwest와 v1 비밀 참조 `{{secret:NAME}}` 자리 표시.
const post: RequestTemplate = {
  ...emptyRequest(),
  method: "POST",
  url: "https://api.x.test/notes",
  headers: [
    { key: "X-Note", value: `it's "quoted"\nline 한글`, enabled: true },
    { key: "X-Trace", value: "{{secret:TRACE_ID}}", enabled: true },
  ],
  body_kind: "json",
  body: '{"title":"안녕"}',
  auth: { kind: "bearer", username: "", password: "", token: "{{secret:API_TOKEN}}", api_key: "", api_value: "" },
};

it("keeps secret references as placeholders in every target", () => {
  for (const target of CODE_TARGETS) {
    const { code, placeholders } = generateCode(target, post, []);
    expect(code, target).toContain("Bearer {{secret:API_TOKEN}}");
    expect(code, target).toContain("{{secret:TRACE_ID}}");
    expect(placeholders, target).toEqual(["secret:API_TOKEN", "secret:TRACE_ID"]);
  }
  const query = generateCode("curl", { ...post, params: [{ key: "trace", value: "{{secret:TRACE_ID}}" }] }, []);
  expect(query.code).toContain("trace={{secret:TRACE_ID}}");
});

it("emits an axios request with headers, body, timeout and text response", () => {
  const code = generateCode("axios", post, []).code;
  expect(code).toContain('import axios from "axios";');
  expect(code).toContain('["X-Note", "it\'s \\"quoted\\" line 한글"],');
  expect(code).toContain('method: "POST",');
  expect(code).toContain('data: "{\\"title\\":\\"안녕\\"}",');
  expect(code).toContain("timeout: 30000,");
  expect(code).not.toContain("httpsAgent");
});

it("stops axios before sending when duplicate headers cannot be kept", () => {
  const request = {
    ...post,
    headers: [
      { key: "X-Many", value: "1" },
      { key: "x-many", value: "2" },
    ],
  };
  expect(generateCode("axios", request, []).code).toContain("axios는 중복 헤더를 보존하지 않습니다");
  expect(generateCode("rust", request, []).code.match(/\.header\("[Xx]-[Mm]any"/g)).toHaveLength(2);
});

it("maps TLS intent for axios and Rust without exporting credentials", () => {
  const insecure = { ...post, tls: { credentialId: "a".repeat(32), verify: false } };
  const verified = { ...post, tls: { credentialId: "a".repeat(32), verify: true } };
  expect(generateCode("axios", insecure, []).code).toContain("rejectUnauthorized: false,");
  expect(generateCode("rust", insecure, []).code).toContain(".danger_accept_invalid_certs(true)");
  const axios = generateCode("axios", verified, []).code;
  expect(axios).toContain('ca: readFileSync("{{ca_pem}}"),');
  expect(axios).toContain("maxRedirects: 0,");
  const rust = generateCode("rust", verified, []).code;
  expect(rust).toContain('reqwest::Certificate::from_pem(&std::fs::read("{{ca_pem}}")?)?');
  expect(rust).toContain("reqwest::redirect::Policy::none()");
  for (const code of [axios, rust]) expect(code).not.toContain("a".repeat(32));
});

it("builds Rust multipart parts with file placeholders and media types", () => {
  const request: RequestTemplate = {
    ...emptyRequest(),
    method: "POST",
    url: "https://x.test/upload",
    body_kind: "multipart",
    multipart: [
      { kind: "text", name: "meta", value: "{}", file_path: "", file_name: "", content_type: "application/json" },
      { kind: "file", name: "upload", value: "", file_path: "/home/u/a.txt", file_name: "a.txt", content_type: "" },
    ],
  };
  const code = generateCode("rust", request, []).code;
  expect(code).toContain('Part::text("{}").mime_str("application/json")?');
  expect(code).toContain('Part::file("{{file:upload}}")?.file_name("a.txt")');
  expect(code).toContain(".multipart(form)");
  expect(code).not.toContain("/home/u/a.txt");
});

it("escapes Rust string literals without JSON-only escapes", () => {
  expect(rustString('a"b\\c\n\t\u0001 한😀')).toBe('"a\\"b\\\\c\\n\\t\\u{1}\\u{2028}한😀"');
  expect(rustString("\uD800")).toBe('"�"');
});

// 리뷰 권장: `secret:false`라도 민감 이름·토큰 모양 값은 코드에 넣지 않고, `secret:` 이름은 환경 값으로 덮지 않는다.
it("keeps sensitive-looking environment values and secret names as placeholders", () => {
  const request: RequestTemplate = {
    ...emptyRequest(),
    method: "GET",
    url: "https://x.test/p?x={{plain}}",
    headers: [
      { key: "X-A", value: "{{apiToken}}" },
      { key: "X-B", value: "{{shaped}}" },
      { key: "X-C", value: "{{secret:T}}" },
    ],
  };
  const env = [
    { key: "plain", value: "visible", secret: false },
    { key: "apiToken", value: "plain-token-value", secret: false },
    { key: "shaped", value: "ghp_abcdefghijklmnopqrstuvwxyz0123", secret: false },
    { key: "secret:T", value: "shadowed-value", secret: false },
  ];
  for (const target of CODE_TARGETS) {
    const { code, placeholders } = generateCode(target, request, env);
    expect(code, target).toContain("visible");
    for (const leaked of ["plain-token-value", "ghp_abcdefghijklmnopqrstuvwxyz0123", "shadowed-value"])
      expect(code, target).not.toContain(leaked);
    expect(placeholders, target).toEqual(["apiToken", "secret:T", "shaped"]);
  }
});

// 리뷰 권장: 객체 리터럴의 `"__proto__"` 키는 프로토타입을 바꿔 헤더가 사라진다.
it("keeps a __proto__ header in axios code", () => {
  const code = generateCode("axios", { ...post, headers: [{ key: "__proto__", value: "v" }] }, []).code;
  expect(code).toContain("const headers = Object.fromEntries([");
  expect(code).toContain('  ["__proto__", "v"],');
  const build = code.slice(code.indexOf("const headers"), code.indexOf("]);") + 3);
  const headers = new Function(`${build}; return headers;`)() as Record<string, string>;
  expect(Object.keys(headers)).toContain("__proto__");
});
