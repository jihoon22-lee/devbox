import { expect, it } from "vitest";
import { CODE_TARGETS, generateCode } from "../lib/codegen";
import { prepare } from "../lib/codegen/plan";
import { parseBruno } from "../lib/importers/bruno";
import { parseCurl } from "../lib/importers/curl";
import { parseHar } from "../lib/importers/har";
import { parseInsomnia } from "../lib/importers/insomnia";
import { type ImportBundle } from "../lib/importers/model";
import { parsePostman } from "../lib/importers/postman";
import { toImportPreview } from "../lib/importers/preview";
import { parsePreview } from "../lib/openapi/preview";
import { sanitizeRequestForPersistence } from "../lib/storage";
import { emptyDraft, row, setBodyKind, toSnapshot, updateBody } from "./draft";
import { importedFile, storedFromTemplate } from "./fromTemplate";
import { parseRequestFile, serializeRequestFile } from "./requestFile";
import { fromStored } from "./stored";
import { templateFromDraft } from "./toTemplate";

const url = "https://owned.invalid/form";
type Field = { key: string; value: string; disabled?: boolean };
const fields: Field[] = [
  { key: "q", value: "a b&c+%25=한" },
  { key: "q", value: "literal%20+%ZZ" },
  { key: "empty", value: "" },
  { key: "off", value: "not sent", disabled: true },
];
const enabled = (rows: Field[]) => rows.filter((f) => !f.disabled);
const wire = (rows: Field[]) => new URLSearchParams(enabled(rows).map((f) => [f.key, f.value])).toString();
const har = (postData: unknown) =>
  parseHar(JSON.stringify({ log: { version: "1.2", entries: [{ request: { method: "POST", url, postData } }] } }));
const producers: [string, (rows: Field[]) => ImportBundle][] = [
  [
    "Postman",
    (rows) =>
      parsePostman(
        JSON.stringify({
          info: { name: "owned", schema: "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
          item: [{ name: "owned", request: { method: "POST", url, body: { mode: "urlencoded", urlencoded: rows } } }],
        }),
      ),
  ],
  [
    "HAR params",
    (rows) =>
      har({
        mimeType: "application/x-www-form-urlencoded",
        params: enabled(rows).map((f) => ({ name: f.key, value: f.value })),
      }),
  ],
  ["HAR wire", (rows) => har({ mimeType: "application/x-www-form-urlencoded", text: wire(rows) })],
  [
    "Insomnia",
    (rows) =>
      parseInsomnia(
        JSON.stringify({
          _type: "export",
          __export_format: 4,
          resources: [
            {
              _id: "req-owned",
              _type: "request",
              name: "owned",
              method: "POST",
              url,
              body: { mimeType: "application/x-www-form-urlencoded", params: rows.map((f) => ({ ...f, name: f.key })) },
            },
          ],
        }),
      ),
  ],
  [
    "Bruno",
    (rows) =>
      parseBruno([
        {
          relativePath: "owned.bru",
          text: `meta {\n name: owned\n type: http\n}\npost {\n url: ${url}\n body: form-urlencoded\n}\nbody:form-urlencoded {\n${rows.map((f) => ` ${f.disabled ? "~" : ""}${f.key}: ${f.value}`).join("\n")}\n}`,
        },
      ]),
  ],
  ["cURL wire", (rows) => parseCurl(`curl '${url}' --data-raw '${wire(rows)}'`)],
  [
    "cURL urlencode",
    (rows) =>
      parseCurl(
        `curl '${url}' ${enabled(rows)
          .map((f) => `--data-urlencode '${encodeURIComponent(f.key)}=${f.value}'`)
          .join(" ")}`,
      ),
  ],
];

it.each(producers)("%s preserves decoded fields through preview, storage, snapshot and codegen", (_name, make) => {
  const preview = toImportPreview(make(fields)).requests[0]!;
  const file = importedFile(preview);
  const expected = enabled(fields).map((f) => ({ name: f.key, value: f.value, enabled: true }));
  expect(file.request.body).toEqual({ kind: "form", fields: expected });
  const draft = fromStored(file.request, { assertions: file.assertions, captures: file.captures });
  expect(toSnapshot(draft).body).toEqual({
    kind: "form",
    fields: enabled(fields).map((f) => ({ name: f.key, value: f.value })),
  });
  for (const template of [preview.request, templateFromDraft(draft)]) {
    const plan = prepare(template, [], new Set());
    expect([...new URLSearchParams(plan.body)]).toEqual(enabled(fields).map((f) => [f.key, f.value]));
    for (const target of CODE_TARGETS) expect(generateCode(target, template, []).code).toContain(plan.body);
  }
});

it.each(producers.filter(([name]) => name !== "Bruno"))(
  "%s preserves embedded line breaks without inventing rows",
  (_name, make) => {
    const input = [
      { key: "line\n=+&", value: "first\r\nsecond=+&한" },
      { key: "q", value: "%7B%7BVALUE%7D%7D" },
    ];
    const file = importedFile(toImportPreview(make(input)).requests[0]!);
    expect(file.request.body).toEqual({
      kind: "form",
      fields: input.map((f) => ({ name: f.key, value: f.value, enabled: true })),
    });
    const plan = prepare(
      templateFromDraft(fromStored(file.request, { assertions: file.assertions, captures: file.captures })),
      [],
      new Set(),
    );
    expect([...new URLSearchParams(plan.body)]).toEqual(input.map((f) => [f.key, f.value]));
  },
);

it("OpenAPI producer retains multiple decoded fields and wire-plus semantics", () => {
  const example = { first: "a b+%&=한", next: "second+ value", empty: "" };
  const parsed = parsePreview(
    {
      openapi: "3.0.3",
      info: { title: "owned", version: "1" },
      servers: [{ url }],
      paths: {
        "/send": {
          post: { requestBody: { content: { "application/x-www-form-urlencoded": { example } } }, responses: {} },
        },
      },
    },
    "json",
  );
  const request = parsed.operations[0]!.request;
  const preview = toImportPreview({
    requests: [{ name: "owned", folder: "", request }],
    environments: [],
    warnings: [],
  });
  expect(importedFile(preview.requests[0]!).request.body).toEqual({
    kind: "form",
    fields: Object.entries(example)
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([name, value]) => ({ name, value, enabled: true })),
  });
});

it("literal local template percent, plus and references remain literal", () => {
  const rows = [
    row("q", "literal%20+%ZZ"),
    row("q", "%7B%7BVALUE%7D%7D"),
    row("ref", "{{VALUE}}"),
    row("ref", "${VALUE}"),
  ];
  const draft = updateBody(setBodyKind({ ...emptyDraft(), url }, "form"), "form", { fields: rows });
  for (const t of [
    templateFromDraft(draft),
    { ...templateFromDraft(draft), form_fields: undefined, body: rows.map((f) => `${f.name}=${f.value}`).join("\n") },
  ]) {
    expect(storedFromTemplate(sanitizeRequestForPersistence(t)).body).toEqual({
      kind: "form",
      fields: rows.map((f) => ({ name: f.name, value: f.value, enabled: true })),
    });
    expect([...new URLSearchParams(prepare(t, [], new Set()).body)]).toEqual(
      rows.map((f) => [f.name, f.value.replace("${VALUE}", "{{VALUE}}")]),
    );
  }
});

it.each(producers)("%s masks decoded sensitive fields while retaining references", (_name, make) => {
  const input = [
    { key: "password", value: "PRIVATE_FORM_LITERAL" },
    { key: "password", value: "{{secret:OWN}}" },
    { key: "access_token", value: "${PUBLIC_REF}" },
  ];
  const preview = toImportPreview(make(input)).requests[0]!;
  const file = importedFile(preview);
  expect(file.request.body).toEqual({
    kind: "form",
    fields: [
      { name: "password", value: "[REDACTED]", enabled: true },
      { name: "password", value: "{{secret:OWN}}", enabled: true },
      { name: "access_token", value: "${PUBLIC_REF}", enabled: true },
    ],
  });
  expect(preview.requiresSecretReview).toBe(true);
  expect(file.masked).toContain("request.body.fields.0.value");
  expect(JSON.stringify(preview)).not.toContain("PRIVATE_FORM_LITERAL");
  const plan = prepare(preview.request, [{ key: "PUBLIC_REF", value: "PRIVATE_ENV_VALUE", secret: false }], new Set());
  expect(plan.body).toContain("password={{secret:OWN}}");
  expect(plan.body).not.toContain("PRIVATE_ENV_VALUE");
});

it.each(["HAR wire", "cURL wire"])("%s classifies encoded sensitive names before masking", (name) => {
  const text = "%70assword=PRIVATE_WIRE_LITERAL&password=%7B%7Bsecret%3AOWN%7D%7D&literal=%2520%2B%25ZZ";
  const bundle =
    name === "HAR wire"
      ? har({ mimeType: "application/x-www-form-urlencoded", text })
      : parseCurl(`curl '${url}' --data-raw '${text}'`);
  const file = importedFile(toImportPreview(bundle).requests[0]!);
  expect(file.request.body).toEqual({
    kind: "form",
    fields: [
      { name: "password", value: "[REDACTED]", enabled: true },
      { name: "password", value: "{{secret:OWN}}", enabled: true },
      { name: "literal", value: "%20+%ZZ", enabled: true },
    ],
  });
  expect(JSON.stringify(file)).not.toContain("PRIVATE_WIRE_LITERAL");
});

it("structured fields retain disabled metadata without leaking values or treating encoding as masking", () => {
  const template = templateFromDraft(
    updateBody(setBodyKind({ ...emptyDraft(), url }, "form"), "form", {
      fields: [row("q", "a b+%ZZ"), row("password", "PRIVATE_DISABLED", false)],
    }),
  );
  const safe = sanitizeRequestForPersistence(template);
  expect(safe.form_fields).toEqual([
    { key: "q", value: "a b+%ZZ", enabled: true },
    { key: "password", value: "[REDACTED]", enabled: false },
  ]);
  expect(storedFromTemplate(safe).body).toEqual({
    kind: "form",
    fields: [
      { name: "q", value: "a b+%ZZ", enabled: true },
      { name: "password", value: "[REDACTED]", enabled: false },
    ],
  });
  expect(prepare(safe, [], new Set()).body).toBe("q=a+b%2B%25ZZ");
  const plain = sanitizeRequestForPersistence({ ...template, form_fields: template.form_fields!.slice(0, 1) });
  expect(plain.requiresSecretReview).toBe(false);
});

it("stored file roundtrip uses existing schema and preserves unknown fields plus literal re-edits", () => {
  const file = importedFile(toImportPreview(producers[0]![1](fields)).requests[0]!);
  const first = serializeRequestFile(file, { kind: "personal" });
  const raw = JSON.parse(first.text);
  raw.extension = "keep";
  raw.request.extension = { keep: true };
  const parsed = parseRequestFile(JSON.stringify(raw));
  const draft = fromStored(parsed.file.request, { assertions: parsed.file.assertions, captures: parsed.file.captures });
  const edited = updateBody(draft, "form", { fields: [row("q", "edited%20+%ZZ&=한\nnext"), row("q", "", false)] });
  const request = storedFromTemplate(templateFromDraft(edited));
  const saved = serializeRequestFile({ ...parsed.file, request }, { kind: "personal" }, parsed.raw);
  expect(saved.text).not.toContain("form_fields");
  expect(JSON.parse(saved.text)).toMatchObject({ extension: "keep", request: { extension: { keep: true } } });
  const reloaded = parseRequestFile(saved.text);
  expect(reloaded.file.request.body).toEqual({
    kind: "form",
    fields: [
      { name: "q", value: "edited%20+%ZZ&=한\nnext", enabled: true },
      { name: "q", value: "", enabled: false },
    ],
  });
  expect(toSnapshot(fromStored(reloaded.file.request, { assertions: [], captures: [] })).body).toEqual({
    kind: "form",
    fields: [{ name: "q", value: "edited%20+%ZZ&=한\nnext" }],
  });
});

it("encoded-looking literal form values do not create codegen placeholders", () => {
  const template = templateFromDraft(
    updateBody(setBodyKind({ ...emptyDraft(), url }, "form"), "form", { fields: [row("q", "%7B%7BVALUE%7D%7D")] }),
  );
  for (const target of CODE_TARGETS) {
    const generated = generateCode(target, template, []);
    expect(generated.placeholders).toEqual([]);
    expect(generated.code).toContain("q=%257B%257BVALUE%257D%257D");
  }
});
