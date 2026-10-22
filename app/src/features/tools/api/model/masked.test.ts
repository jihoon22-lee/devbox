// 검토 권장(3c): `masked` 경로가 행 번호라 행을 더하거나 지우면 어긋났다. 초안은 행 id로 기억한다.
import { describe, expect, it } from "vitest";
import { runCollection } from "../runner/runner";
import { Session } from "../runner/session";
import { row } from "./draft";
import { draftFromFile, fileFromDraft } from "./fileDraft";
import { maskedPaths, redactedFields } from "./masked";
import { parseRequestFile, serializeRequestFile } from "./requestFile";
import type { StoredRequest } from "./stored";

const personal = { kind: "personal" } as const;
const request: StoredRequest = {
  method: "GET",
  url: "https://x.test",
  query: [],
  headers: [
    { name: "X-A", value: "a", enabled: true },
    { name: "Authorization", value: "Bearer typed", enabled: true },
  ],
  cookies: [],
  body: { kind: "form", fields: [{ name: "password", value: "typed", enabled: true }] },
  auth: { kind: "none" },
  tls: { verify: true, caPath: null, clientCertPath: null, clientKeyPath: null },
  timeoutMs: 30_000,
  followRedirects: true,
};
const saved = () =>
  parseRequestFile(
    serializeRequestFile({ version: 1, name: "R", request, assertions: [], captures: [] }, personal).text,
  );

describe("masked positions follow rows", () => {
  it("masked_rows_survive_adding_and_removing_rows", async () => {
    const parsed = saved();
    expect(parsed.file.masked).toEqual(["request.headers.1.value", "request.body.fields.0.value"]);
    let draft = draftFromFile(parsed);
    expect(redactedFields(draft)).toEqual(["request.headers.1.value", "request.body.fields.0.value"]);
    // 위에 두 행을 넣고 맨 앞 행(X-A)을 지운다 → 가린 행은 2번이 된다.
    draft = { ...draft, headers: [row("X-New", "1"), row("X-Other", "2"), draft.headers[1]!] };
    draft = { ...draft, bodies: { form: { fields: [row("user", "u"), ...draft.bodies.form!.fields] } } };
    expect(maskedPaths(draft)).toEqual(["request.headers.2.value", "request.body.fields.1.value"]);
    expect(redactedFields(draft)).toEqual(["request.headers.2.value", "request.body.fields.1.value"]);
    // 새로 넣은 행에 `[REDACTED]`를 써도 막지 않는다.
    draft = { ...draft, headers: [row("X-Note", "[REDACTED]"), ...draft.headers.slice(1)] };
    expect(redactedFields(draft)).toEqual(["request.headers.2.value", "request.body.fields.1.value"]);
    // 다시 저장·다시 열어도 같은 행을 막는다.
    const again = parseRequestFile(serializeRequestFile(fileFromDraft(draft, "R", parsed), personal, parsed.raw).text);
    expect(again.file.masked).toEqual(["request.headers.2.value", "request.body.fields.1.value"]);
    // 실행기도 같은 판정으로 건너뛴다.
    const summary = await runCollection(
      [{ id: "r", name: "R", draft, reviewNeeded: false, produces: [] }],
      [],
      new Session(),
      { stopOnFailure: false, delayMs: 0 },
      { runId: "run", send: async () => Promise.reject(new Error("sent")), sleep: async () => {} },
      new AbortController().signal,
      () => {},
    );
    expect(summary.steps[0]).toMatchObject({ status: "skipped", message: "가린 값을 다시 입력해 주세요" });
    // 가린 행을 지우거나 다시 입력하면 막지 않는다.
    const removed = { ...draft, headers: draft.headers.slice(0, 2), bodies: { form: { fields: [row("user", "u")] } } };
    expect(maskedPaths(removed)).toEqual([]);
    const retyped = {
      ...draft,
      headers: draft.headers.map((h) => (h.name === "Authorization" ? { ...h, value: "Bearer {{secret:T}}" } : h)),
      bodies: { form: { fields: draft.bodies.form!.fields.map((f) => ({ ...f, value: "{{secret:P}}" })) } },
    };
    expect(redactedFields(retyped)).toEqual([]);
  });

  it("assertion_masks_follow_assertion_ids", () => {
    const assertion = { id: "a1", enabled: true, source: "status" as const, target: "", operator: "equals" as const };
    const text = serializeRequestFile(
      {
        version: 1,
        name: "R",
        request,
        assertions: [{ ...assertion, expected: "ghp_abcdefghijklmnopqrstuvwxyz0123" }],
        captures: [],
      },
      personal,
    ).text;
    let draft = draftFromFile(parseRequestFile(text));
    expect(redactedFields(draft)).toContain("assertions.0.expected");
    draft = { ...draft, assertions: [{ ...assertion, id: "a0", expected: "200" }, ...draft.assertions] };
    expect(maskedPaths(draft)).toContain("assertions.1.expected");
  });
});
