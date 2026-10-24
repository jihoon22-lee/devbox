import { describe, expect, it } from "vitest";
import { CODE_TARGETS, generateCode } from "../lib/codegen";
import { prepare } from "../lib/codegen/plan";
import { sanitizeRequestForPersistence } from "../lib/storage";
import type { EnvVariable } from "../lib/template";
import { emptyDraft, row, setAuthKind, updateAuth } from "./draft";
import { storedFromTemplate } from "./fromTemplate";
import { templateFromDraft } from "./toTemplate";

function draft(name = "client key&/한", value = "{{secret:OWNED}}", placement: "query" | "header" = "query") {
  return updateAuth(
    setAuthKind(
      {
        ...emptyDraft(),
        url: "https://owned.invalid/path?q=existing#fragment",
        query: [row("q", "one"), row("q", "two"), row("skip", "off", false)],
      },
      "apiKey",
    ),
    "apiKey",
    { name, value, placement },
  );
}

it("active query authentication survives draft, sanitizer, roundtrip and shared codegen plan", () => {
  const input = draft();
  const template = sanitizeRequestForPersistence(templateFromDraft(input));
  expect(storedFromTemplate(template).auth).toEqual({
    kind: "apiKey",
    name: "client key&/한",
    value: "{{secret:OWNED}}",
    placement: "query",
  });
  const plan = prepare(template, [], new Set());
  const url = new URL(plan.url);
  expect(url.searchParams.getAll("q")).toEqual(["existing", "one", "two"]);
  expect(url.searchParams.get("client key&/한")).toBe("{{secret:OWNED}}");
  expect(url.searchParams.has("skip")).toBe(false);
  expect(url.hash).toBe("");
  expect(plan.headers).toEqual([]);
  for (const target of CODE_TARGETS) {
    const generated = generateCode(target, template, []);
    expect(generated.code, target).toContain(plan.url);
    expect(generated.placeholders, target).toEqual(["secret:OWNED"]);
  }
});

it("auth query appends duplicates without replacing existing URL or enabled rows", () => {
  const plan = prepare(templateFromDraft(draft("q")), [], new Set());
  expect(new URL(plan.url).searchParams.getAll("q")).toEqual(["existing", "one", "two", "{{secret:OWNED}}"]);
  expect(plan.headers).toEqual([]);
});

describe.each([
  ["literal", "owned-literal-key", "{{REDACTED_VALUE}}", []],
  [
    "ordinary environment",
    "{{credentialValue}}",
    "{{credentialValue}}",
    [{ key: "credentialValue", value: "owned-env-key", secret: false }],
  ],
  [
    "secret reference",
    "{{secret:OWNED}}",
    "{{secret:OWNED}}",
    [{ key: "secret:OWNED", value: "must-not-inline", secret: false }],
  ],
] as const)("query authentication keeps %s values sensitive", (_label, value, expected, environment) => {
  it("never inlines auth credentials for an arbitrary query name in any target", () => {
    const template = sanitizeRequestForPersistence(templateFromDraft(draft("ordinary", value)));
    const env: EnvVariable[] = environment.map((item) => ({ ...item }));
    const plan = prepare(template, env, new Set());
    expect(new URL(plan.url).searchParams.get("ordinary")).toBe(expected);
    expect(plan.headers).toEqual([]);
    for (const target of CODE_TARGETS) {
      const generated = generateCode(target, template, env);
      expect(generated.code, target).toContain(plan.url);
      for (const hidden of ["owned-literal-key", "owned-env-key", "must-not-inline"])
        expect(generated.code).not.toContain(hidden);
    }
  });
});

it("explicit and legacy header placement remain headers, while inactive drafts add no auth", () => {
  const explicit = templateFromDraft(draft("X-Key", "{{secret:OWNED}}", "header"));
  const legacy = { ...explicit, auth: explicit.auth && { ...explicit.auth } };
  if (legacy.auth) delete legacy.auth.placement;
  for (const template of [explicit, legacy]) {
    const plan = prepare(template, [], new Set());
    expect(plan.headers).toEqual([["X-Key", "{{secret:OWNED}}"]]);
    expect(new URL(plan.url).searchParams.has("X-Key")).toBe(false);
  }
  const inactive = templateFromDraft(setAuthKind(draft(), "none"));
  expect(prepare(inactive, [], new Set()).headers).toEqual([]);
  expect(new URL(prepare(inactive, [], new Set()).url).searchParams.has("client key&/한")).toBe(false);
});
