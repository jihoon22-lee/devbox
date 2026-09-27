import { expect, it } from "vitest";
import { FILE_COLLECTION_MARKER, parseFileCollection, safeFileName, serializeFileCollection } from "./fileCollection";
import { emptyRequest } from "./importers";
const entry = (id: string, name: string, folder: string) => ({
  id,
  name,
  folder,
  saved_at: 1,
  requiresSecretReview: false,
  request: { ...emptyRequest(), url: `https://x.test/${id}`, requiresSecretReview: false },
});
it("writes deterministic request files, sanitizes names and preserves folder grouping", () => {
  const files = serializeFileCollection("Demo", {
    version: 2,
    collections: [
      entry("a", "List: users?", "Users/Admin"),
      entry("b", "List: users?", "Users/Admin"),
      entry("c", "Health", ""),
    ],
  });
  expect(files.map((file) => file.relativePath)).toEqual([
    FILE_COLLECTION_MARKER,
    "Users/Admin/List- users-.request.json",
    "Users/Admin/List- users- (2).request.json",
    "Health.request.json",
  ]);
  expect(JSON.parse(files[1].text)).toMatchObject({
    schema: "devbox.api-studio.request",
    schemaVersion: 1,
    name: "List: users?",
  });
  expect(parseFileCollection(files).requests.map((item) => [item.name, item.folder])).toEqual([
    ["List: users?", "Users/Admin"],
    ["List: users?", "Users/Admin"],
    ["Health", ""],
  ]);
});
it("normalizes reserved names and avoids case-insensitive collisions", () => {
  expect(safeFileName('  a/b\\c:d*e?f"g<h>i|j.  ')).toBe("a-b-c-d-e-f-g-h-i-j");
  expect(safeFileName("CON.txt")).toBe("_CON.txt");
  expect(safeFileName("CON")).toBe("_CON");
  expect(safeFileName("")).toBe("untitled");
  expect(safeFileName("가".repeat(100)).length).toBeLessThanOrEqual(80);
  const files = serializeFileCollection("Demo", {
    version: 2,
    collections: [entry("a", "Foo", ""), entry("b", "foo", "")],
  });
  expect(files[2].relativePath).toBe("foo (2).request.json");
});
it("rejects unsafe paths, malformed schema and oversized input", () => {
  expect(() => parseFileCollection([{ relativePath: "../x.request.json", text: "{}" }])).toThrow();
  expect(() => parseFileCollection([{ relativePath: "x.request.json", text: "{}" }])).toThrow();
  expect(() => parseFileCollection([{ relativePath: "x.bru", text: "x".repeat(1024 * 1024 + 1) }])).toThrow();
});
it("redacts secrets before serialization and keeps review requirements after round trip", () => {
  const item = entry("x", "Secret", "");
  item.request.url = "https://x.test?token=secret";
  const files = serializeFileCollection("Demo", { version: 2, collections: [item] });
  expect(files[1].text).not.toContain("token=secret");
  expect(parseFileCollection(files).requests[0].requiresSecretReview).toBe(true);
});
