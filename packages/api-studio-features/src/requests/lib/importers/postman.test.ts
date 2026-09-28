import { describe, expect, it } from "vitest";
import fixture from "./fixtures/postman-v21.json";
import { parsePostman } from "./postman";

describe("Postman v2.1 import", () => {
  const bundle = parsePostman(JSON.stringify(fixture));
  it("keeps the folder tree and variables", () => {
    expect(bundle.requests.map((r) => `${r.folder}|${r.name}`)).toEqual([
      "Users|List users",
      "Users|Create user",
      "Users/Admin|Login form",
      "Users/Admin|Query",
    ]);
    expect(bundle.environments[0].name).toBe("Postman: Devbox demo");
    expect(bundle.environments[0].variables.map((v) => v.key)).toEqual(["baseUrl", "key"]);
  });
  it("maps headers, bodies and inherited auth", () => {
    const [list, create, form, query] = bundle.requests.map((r) => r.request);
    expect(list.url).toBe("{{baseUrl}}/users");
    expect(list.headers.find((h) => h.key === "X-Trace")?.enabled).toBe(false);
    expect(list.auth).toMatchObject({ kind: "apikey", api_key: "X-Key", api_value: "{{key}}" });
    expect(create).toMatchObject({ method: "POST", body_kind: "json" });
    expect(create.auth).toMatchObject({ kind: "bearer", token: "{{token}}" });
    expect(form).toMatchObject({ body_kind: "form", body: "user=kim" });
    expect(query.body_kind).toBe("graphql");
    expect(query.graphql?.query).toContain("query");
  });
  it("warns about scripts and unsupported auth", () => {
    expect(bundle.warnings).toEqual(
      expect.arrayContaining([
        "스크립트 1개는 가져오지 않았습니다.",
        "Query: 지원하지 않는 인증 방식(oauth2)은 가져오지 않았습니다.",
      ]),
    );
  });
  it("rejects other schemas", () => {
    expect(() =>
      parsePostman(JSON.stringify({ info: { schema: "https://schema.getpostman.com/json/collection/v1.0.0/" } })),
    ).toThrow();
  });
});

it("stops inherited auth and encodes enabled form fields for the native line format", () => {
  const bundle = parsePostman(
    JSON.stringify({
      info: fixture.info,
      auth: fixture.auth,
      item: [
        {
          name: "Public",
          auth: { type: "noauth" },
          item: [
            {
              name: "Form",
              request: {
                method: "POST",
                url: "https://x.test",
                body: {
                  mode: "urlencoded",
                  urlencoded: [
                    { key: "q", value: "a b&c" },
                    { key: "off", value: "ignored", disabled: true },
                  ],
                },
              },
            },
          ],
        },
      ],
    }),
  );
  expect(bundle.requests[0].request.auth?.kind).toBe("none");
  expect(bundle.requests[0].request.body).toBe("q=a%20b%26c");
});
it("retains a filename but never an imported multipart path", () => {
  const bundle = parsePostman(
    JSON.stringify({
      info: fixture.info,
      item: [
        {
          name: "Upload",
          request: {
            method: "POST",
            url: "https://x.test",
            body: { mode: "formdata", formdata: [{ key: "doc", type: "file", src: "C:\\private\\report.pdf" }] },
          },
        },
      ],
    }),
  );
  expect(bundle.requests[0].request.multipart[0]).toMatchObject({ file_path: "", file_name: "report.pdf" });
  expect(JSON.stringify(bundle)).not.toContain("private");
  expect(bundle.warnings).toHaveLength(1);
});
