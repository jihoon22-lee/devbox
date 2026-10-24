import type { EditorState } from "@codemirror/state";
import type { DocKey } from "../../../rpc/gen/rpc";
export interface CodeLocation {
  line: number;
  column: number;
}
export type SourceSearch = { root?: string; path?: string; line: number; column: number };
export function validCodeKey(value: unknown): value is DocKey {
  if (!value || typeof value !== "object") return false;
  const key = value as Partial<DocKey>;
  return (
    typeof key.root === "string" &&
    /^project:[^\s\0]+$/.test(key.root) &&
    key.root.length <= 128 &&
    typeof key.path === "string" &&
    key.path.length > 0 &&
    key.path.length <= 4096 &&
    !key.path.includes("\0") &&
    !key.path
      .split("/")
      .some(
        (part) =>
          !part ||
          part === "." ||
          part === ".." ||
          part === ".git" ||
          part === ".trash" ||
          part.startsWith(".devbox-tmp-"),
      )
  );
}
export function decodeCodeKey(value?: string): DocKey | undefined {
  try {
    const [root, path] = JSON.parse(value ?? "null") as unknown[];
    const key = { root, path };
    return validCodeKey(key) ? key : undefined;
  } catch {
    return;
  }
}
const positive = (value: unknown) =>
  typeof value === "number" && Number.isSafeInteger(value) && value > 0 ? value : 1;
export function sourceSearch(search: Record<string, unknown>): SourceSearch {
  const key = { root: search.root, path: search.path };
  return { ...(validCodeKey(key) ? key : {}), line: positive(search.line), column: positive(search.column) };
}
export function offsetAt(state: EditorState, location: CodeLocation): number {
  const line = state.doc.line(Math.min(positive(location.line), state.doc.lines));
  return line.from + Math.min(positive(location.column) - 1, line.length);
}
