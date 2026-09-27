import { expect, it } from "vitest";
import { parseStore, duplicateEntry, type CollectionStore } from "./collections";
import { parseCollectionExport, serializeCollectionExport } from "./transfer";
import { parseFileCollection, serializeFileCollection } from "./fileCollection";
import { emptyRequest, parseImport, toImportPreview } from "./importers";
import { cleanAssertions } from "./assertions";
import { cleanCaptures } from "./captures";
const assertion = {
  id: "a",
  enabled: true,
  source: "status" as const,
  target: "",
  operator: "equals" as const,
  expected: "200",
};
const capture = { id: "c", enabled: true, variable: "token", source: "jsonPath" as const, target: "$.token" };
const entry = () => ({
  id: "r",
  name: "Request",
  folder: "",
  saved_at: 1,
  requiresSecretReview: false,
  request: { ...emptyRequest(), url: "https://x.test", requiresSecretReview: false },
});
it("reads old documents with empty checks and round trips checks through store, JSON and files", () => {
  expect(parseStore(JSON.stringify({ version: 2, collections: [entry()] }))!.collections[0]).toMatchObject({
    assertions: [],
    captures: [],
  });
  const store: CollectionStore = {
    version: 2,
    collections: [{ ...entry(), assertions: [assertion], captures: [capture] }],
  };
  expect(parseStore(JSON.stringify(store))!.collections[0]).toMatchObject({
    assertions: [assertion],
    captures: [capture],
  });
  const raw = serializeCollectionExport(store);
  expect(parseCollectionExport(raw)!.collections[0]).toMatchObject({ assertions: [assertion], captures: [capture] });
  const files = parseFileCollection(serializeFileCollection("Demo", store));
  expect(files.requests[0]).toMatchObject({ assertions: [assertion], captures: [capture] });
  const preview = toImportPreview(parseImport("devbox", [{ relativePath: "export.json", text: raw }]), () => "new");
  expect(preview.collections.collections[0]).toMatchObject({ assertions: [assertion], captures: [capture] });
  const copied = duplicateEntry(store, "r", 2, () => "copy");
  copied.collections[0].assertions![0].expected = "201";
  expect(store.collections[0].assertions![0].expected).toBe("200");
});
it("drops invalid checks individually, counts them and redacts known expected tokens", () => {
  const cleaned = cleanAssertions([
    ...Array.from({ length: 51 }, (_, index) => ({ ...assertion, id: String(index) })),
    { ...assertion, operator: "unknown" },
  ]);
  expect(cleaned.assertions).toHaveLength(50);
  expect(cleaned.dropped).toBe(2);
  expect(cleanCaptures([{ ...capture, variable: "bad name" }, capture])).toEqual({ captures: [capture], dropped: 1 });
  const raw = JSON.stringify({
    version: 2,
    collections: [
      {
        ...entry(),
        assertions: [
          { ...assertion, expected: "ghp_abcdefghijklmnopqrstuvwxyz0123" },
          { ...assertion, source: "invalid" },
        ],
        captures: [capture],
      },
    ],
  });
  const parsed = parseStore(raw)!;
  expect(parsed.collections).toHaveLength(1);
  expect(parsed.collections[0]).toMatchObject({
    requiresSecretReview: true,
    assertions: [{ ...assertion, expected: "[REDACTED]" }],
  });
  expect(serializeCollectionExport(parsed)).not.toContain("ghp_");
});
it("retains structurally valid invalid expressions so evaluation can report their failures", () => {
  const invalidRegex = { ...assertion, operator: "matches" as const, expected: "(unclosed" };
  expect(cleanAssertions([invalidRegex]).assertions).toEqual([invalidRegex]);
  const invalidPath = { ...capture, target: "$[" };
  expect(cleanCaptures([invalidPath]).captures).toEqual([invalidPath]);
});
