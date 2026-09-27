import { describe, expect, it } from "vitest";
import { parseHar } from "./har";

const har = (entries: unknown[]) => JSON.stringify({ log: { version: "1.2", entries } });
const entry = (method: string, url: string, extra: Record<string, unknown> = {}) => ({
  request: {
    method,
    url,
    headers: [
      { name: ":authority", value: "x.test" },
      { name: "Accept", value: "*/*" },
      { name: "Content-Length", value: "2" },
      { name: "Cookie", value: "a=1" },
    ],
    ...extra,
  },
  response: { status: 200 },
});

describe("HAR import", () => {
  it("keeps http requests, drops browser-managed headers and groups by host", () => {
    const bundle = parseHar(
      har([
        entry("GET", "https://x.test/api/users?page=1"),
        entry("POST", "https://x.test/api/users", { postData: { mimeType: "application/json", text: '{"a":1}' } }),
        entry("GET", "data:image/png;base64,AAAA"),
        entry("GET", "https://x.test/api/users?page=1"),
      ]),
    );
    expect(bundle.requests.map((r) => `${r.folder}|${r.name}`)).toEqual([
      "x.test|GET /api/users",
      "x.test|POST /api/users",
    ]);
    const [get, post] = bundle.requests.map((r) => r.request);
    expect(get.headers).toEqual([{ key: "Accept", value: "*/*", enabled: true }]);
    expect(get.cookies).toEqual([{ name: "a", value: "1", enabled: true }]);
    expect(post).toMatchObject({ body_kind: "json", body: '{"a":1}' });
    expect(bundle.warnings).toEqual(["같은 요청 1개와 http(s)가 아닌 요청 1개를 건너뛰었습니다."]);
  });
  it("caps very large archives", () => {
    const many = Array.from({ length: 510 }, (_, i) => entry("GET", `https://x.test/${i}`));
    const bundle = parseHar(har(many));
    expect(bundle.requests).toHaveLength(500);
    expect(bundle.warnings).toContain("요청이 많아 처음 500개만 가져왔습니다.");
  });
});

it("prefers structured cookies and keeps distinct multipart bodies without local paths", () => {
  const entries = ["one", "two"].map((value) =>
    entry("POST", "https://x.test/upload", {
      cookies: [{ name: "session", value: "cookie" }],
      postData: {
        mimeType: "multipart/form-data",
        params: [
          { name: "q", value },
          { name: "doc", fileName: "/private/report.pdf" },
        ],
      },
    }),
  );
  const bundle = parseHar(har(entries));
  expect(bundle.requests).toHaveLength(2);
  expect(bundle.requests[0].request.cookies).toEqual([{ name: "session", value: "cookie", enabled: true }]);
  expect(bundle.requests[0].request.multipart[1]).toMatchObject({ file_path: "", file_name: "report.pdf" });
  expect(JSON.stringify(bundle)).not.toContain("private");
});
