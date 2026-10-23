import { expect, it } from "vitest";
import { generateCode } from "../lib/codegen";
import { emptyDraft, row, setAuthKind, setBodyKind, updateAuth, updateBody } from "./draft";
import { storedFromTemplate } from "./fromTemplate";
import { toStored } from "./stored";
import { templateFromDraft } from "./toTemplate";

it("draft_to_template_round_trips_for_code_generation", () => {
  let draft = { ...emptyDraft(), method: "POST", url: "https://x.test/a" };
  draft.query = [row("q", "1")];
  draft.headers = [row("X-Trace", "abc"), row("X-Off", "0", false)];
  draft = updateBody(setBodyKind(draft, "json"), "json", { text: '{"a":1}' });
  draft = updateAuth(setAuthKind(draft, "bearer"), "bearer", { token: "{{secret:API_TOKEN}}" });
  const template = templateFromDraft(draft);
  const back = storedFromTemplate(template);
  const stored = toStored(draft);
  expect(back.headers).toEqual(stored.headers);
  expect(back.query).toEqual(stored.query);
  expect(back.body).toEqual(stored.body);
  expect(back.auth).toEqual(stored.auth);
  const { code } = generateCode("curl", template, []);
  expect(code).toContain("https://x.test/a");
  expect(code).not.toContain("X-Off");
});
