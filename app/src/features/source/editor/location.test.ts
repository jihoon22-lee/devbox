import { EditorState } from "@codemirror/state";
import { expect, test } from "vitest";
import { decodeCodeKey, offsetAt, sourceSearch } from "./location";

test("code popout identity preserves root and Unicode but rejects scope escapes", () => {
  const key = { root: "project:one", path: "src/가😀 name.ts" };
  expect(decodeCodeKey(JSON.stringify([key.root, key.path]))).toEqual(key);
  for (const path of ["../host", "/etc/passwd", ".git/config", "x/.trash/y", "a\0b"])
    expect(decodeCodeKey(JSON.stringify([key.root, path]))).toBeUndefined();
  expect(sourceSearch({ root: key.root, path: key.path, line: 2, column: 3 })).toMatchObject({
    ...key,
    line: 2,
    column: 3,
  });
  expect(sourceSearch({ root: key.root, path: key.path, line: -1, column: 0 })).toMatchObject({ line: 1, column: 1 });
});
test("locations use 1-based lines and UTF-16 columns, bounded by the document", () => {
  const state = EditorState.create({ doc: "first\n😀a\nlast" });
  expect(offsetAt(state, { line: 2, column: 3 })).toBe(8);
  expect(offsetAt(state, { line: 200, column: 200 })).toBe(state.doc.length);
});
