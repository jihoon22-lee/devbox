import { describe, expect, it } from "vitest";
import fixture from "./fixtures/insomnia-v4.json";
import { parseInsomnia } from "./insomnia";

describe("Insomnia v4 import", () => {
  const bundle = parseInsomnia(JSON.stringify(fixture));
  it("normalizes template variables and folders", () => {
    const me = bundle.requests.find((r) => r.name === "Me")!;
    expect(me.folder).toBe("Auth");
    expect(me.request.url).toBe("{{baseUrl}}/me");
    expect(me.request.auth).toMatchObject({ kind: "bearer", token: "{{token}}" });
  });
  it("maps form bodies and query parameters", () => {
    const login = bundle.requests.find((r) => r.name === "Login")!.request;
    expect(login).toMatchObject({ body_kind: "form", body: "user=kim" });
    const search = bundle.requests.find((r) => r.name === "Search")!.request;
    expect(search.params).toEqual([{ key: "q", value: "devbox" }]);
  });
  it("merges base values into sub environments and warns about tags", () => {
    expect(bundle.environments.map((e) => e.name)).toEqual(["Base Environment", "staging"]);
    expect(bundle.environments[1].variables).toEqual([
      { key: "baseUrl", value: "https://staging.x.test" },
      { key: "token", value: "t" },
    ]);
    expect(bundle.warnings.some((w) => w.includes("템플릿 태그"))).toBe(true);
  });
});

it("rejects cyclic folder ancestry rather than looping or importing partial requests", () => {
  const cyclic = {
    _type: "export",
    __export_format: 4,
    resources: [
      { _id: "group", _type: "request_group", parentId: "group", name: "Loop" },
      { _id: "request", _type: "request", parentId: "group", name: "Me", method: "GET", url: "https://x.test" },
    ],
  };
  expect(() => parseInsomnia(JSON.stringify(cyclic))).toThrow();
});
it("keeps a multipart file placeholder without retaining the local path", () => {
  const bundle = parseInsomnia(
    JSON.stringify({
      _type: "export",
      __export_format: 4,
      resources: [
        { _id: "workspace", _type: "workspace", name: "Demo" },
        {
          _id: "request",
          _type: "request",
          parentId: "workspace",
          name: "Upload",
          method: "POST",
          url: "https://x.test",
          body: {
            mimeType: "multipart/form-data",
            params: [{ name: "doc", type: "file", fileName: "/private/report.pdf" }],
          },
        },
      ],
    }),
  );
  expect(bundle.requests[0].request.multipart[0]).toMatchObject({ file_path: "", file_name: "report.pdf" });
  expect(JSON.stringify(bundle)).not.toContain("private");
});
