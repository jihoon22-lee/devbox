import { describe, expect, it } from "vitest";
import type { WebhookReceivedDetail } from "../../../rpc/gen/rpc";
import { activeBody } from "../api/model/draft";
import { draftFromFile, fileFromDraft } from "../api/model/fileDraft";
import { parseRequestFile, serializeRequestFile } from "../api/model/requestFile";
import { fixtureToRule } from "./fixtureToRule";
import { toRequest } from "./toRequest";

const detail: WebhookReceivedDetail = {
  received: { id: "r", listenerId: "l", method: "POST", path: "/hook", atMs: 1, sizeBytes: 3, truncated: false },
  query: "a=1",
  headers: [
    ["Host", "bad"],
    ["Content-Length", "3"],
    ["Connection", "X-Local"],
    ["X-Local", "omit"],
    ["Content-Type", "application/octet-stream"],
    ["Authorization", "Bearer fixture-webhook-token"],
  ],
  bodyBase64: "AP+A",
  previewTruncated: false,
};
describe("received request conversion", () => {
  it("preserves binary bytes and removes transport headers without sending", () => {
    const draft = toRequest(detail, "http://127.0.0.1:1234");
    expect(draft.url).toBe("http://127.0.0.1:1234/hook?a=1");
    expect(draft.headers.map((h) => h.name)).toEqual(["Content-Type", "Authorization"]);
    expect(activeBody(draft)).toEqual({
      kind: "raw",
      text: "AP+A",
      contentType: "application/octet-stream",
      encoding: "base64",
    });
    expect([...atob(draft.bodies.raw?.text ?? "")].map((c) => c.charCodeAt(0))).toEqual([0, 255, 128]);
  });
  it("received_authorization_is_not_saved_in_plain_text", () => {
    const draft = toRequest(detail, "http://127.0.0.1:1234");
    const saved = serializeRequestFile(fileFromDraft(draft, "Received"), { kind: "project", projectId: "p" });
    expect(saved.text).not.toContain("fixture-webhook-token");
    expect(saved.text).toContain("[REDACTED]");
    const again = draftFromFile(parseRequestFile(saved.text));
    expect(again.bodies.raw?.encoding).toBe("base64");
  });
  it("roundtrips binary bytes through local stored request", () => {
    const draft = toRequest(detail, "http://localhost:1234");
    const saved = serializeRequestFile(fileFromDraft(draft, "Received"), { kind: "personal" });
    expect(activeBody(draftFromFile(parseRequestFile(saved.text)))).toEqual(activeBody(draft));
  });
  it("rejects truncated captures instead of replaying incomplete bytes", () => {
    expect(() => toRequest({ ...detail, previewTruncated: true }, "http://localhost")).toThrow();
  });
});

it("fixture_to_rule_draft_is_local_and_has_empty_response_metadata", () => {
  const rule = fixtureToRule({ id: "f", name: "fixture", request: detail });
  expect(rule).toMatchObject({
    id: "",
    method: "POST",
    path: "/hook",
    status: 200,
    headers: [],
    body: "",
    delayMs: 0,
    sequence: [],
  });
});
it("valid UTF-8 including a BOM preserves original bytes", () => {
  const draft = toRequest({ ...detail, bodyBase64: "77u/YQ==" }, "http://localhost");
  expect([...new TextEncoder().encode(draft.bodies.raw?.text)]).toEqual([239, 187, 191, 97]);
});
it("binary project file roundtrip preserves bytes and omits authorization", () => {
  const draft = toRequest(detail, "http://localhost");
  const file = serializeRequestFile(fileFromDraft(draft, "Binary"), { kind: "project", projectId: "p" });
  const restored = draftFromFile(parseRequestFile(file.text));
  expect(restored.bodies.raw).toEqual(draft.bodies.raw);
  expect(file.text).not.toContain("fixture-webhook-token");
});
