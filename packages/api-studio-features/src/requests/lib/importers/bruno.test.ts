import { describe, expect, it } from "vitest";
import { parseBruno, parseBruBlocks } from "./bruno";

const REQUEST = `meta {
  name: Create user
  type: http
  seq: 2
}

post {
  url: {{baseUrl}}/users
  body: json
  auth: bearer
}

headers {
  Accept: application/json
  ~X-Debug: 1
}

auth:bearer {
  token: {{token}}
}

body:json {
  {
    "name": "kim"
  }
}

script:pre-request {
  req.setHeader("x", "y");
}
`;

const ENV = `vars {
  baseUrl: https://x.test
}
vars:secret [
  token
]
`;

describe("Bruno import", () => {
  it("splits dictionary, text and list blocks", () => {
    const blocks = parseBruBlocks(REQUEST);
    expect(blocks.map((b) => b.name)).toEqual([
      "meta",
      "post",
      "headers",
      "auth:bearer",
      "body:json",
      "script:pre-request",
    ]);
    expect(blocks.find((b) => b.name === "headers")?.entries).toEqual([
      { key: "Accept", value: "application/json", enabled: true },
      { key: "X-Debug", value: "1", enabled: false },
    ]);
    expect(blocks.find((b) => b.name === "body:json")?.text).toBe('{\n  "name": "kim"\n}');
    expect(parseBruBlocks(ENV).find((b) => b.name === "vars:secret")?.items).toEqual(["token"]);
  });

  it("builds requests with folders and environments with secret keys", () => {
    const bundle = parseBruno([
      { relativePath: "users/create-user.bru", text: REQUEST },
      { relativePath: "environments/local.bru", text: ENV },
    ]);
    expect(bundle.requests[0]).toMatchObject({ name: "Create user", folder: "users" });
    expect(bundle.requests[0].request).toMatchObject({
      method: "POST",
      url: "{{baseUrl}}/users",
      body_kind: "json",
      body: '{\n  "name": "kim"\n}',
    });
    expect(bundle.requests[0].request.auth).toMatchObject({ kind: "bearer", token: "{{token}}" });
    expect(bundle.environments).toEqual([
      {
        name: "local",
        variables: [
          { key: "baseUrl", value: "https://x.test" },
          { key: "token", value: "", secret: true },
        ],
      },
    ]);
    expect(bundle.warnings).toContain("스크립트·테스트 블록 1개는 가져오지 않았습니다.");
  });
});

it("preserves explicit secret classification for arbitrary Bruno variable names", async () => {
  const { toImportPreview } = await import("./index");
  const bundle = parseBruno([{ relativePath: "environments/local.bru", text: "vars:secret [\n  opaque\n]\n" }]);
  const preview = toImportPreview(bundle, () => "id");
  expect(preview.environments.environments[0].variables).toEqual([
    { key: "opaque", reference: "${opaque}", secret: true },
  ]);
});

it("recognizes an individually selected environment from its vars blocks", () => {
  const bundle = parseBruno([{ relativePath: "local.bru", text: "vars {\n  baseUrl: https://x.test\n}\n" }]);
  expect(bundle.environments[0]).toEqual({ name: "local", variables: [{ key: "baseUrl", value: "https://x.test" }] });
});

it("accepts trailing whitespace on the block delimiter without trimming the body", () => {
  expect(parseBruBlocks("body:text {\n  a  \n}  \n")[0].text).toBe("a  ");
});
