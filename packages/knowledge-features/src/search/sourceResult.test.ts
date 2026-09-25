import { expect, it } from "vitest";
import { sourceRowValue } from "./sourceResult";
it("normalizes heterogeneous source metadata without copying authority fields", () => {
  const row = sourceRowValue({ path: "C:/fixture/note.md", name: "note", snippet: "match", reference: "substituted" });
  expect(row).toMatchObject({ path: "C:/fixture/note.md", name: "note", snippet: "match", root_id: null, size: 0 });
  expect(row).not.toHaveProperty("reference");
});
it.each([null, [], "text", { path: 1, name: "note" }])("rejects an invalid source row", (value) => {
  expect(() => sourceRowValue(value)).toThrow("검색 결과 형식을 확인하지 못했습니다.");
});
