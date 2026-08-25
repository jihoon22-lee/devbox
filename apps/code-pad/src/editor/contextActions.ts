import { EditorSelection, type EditorState, type TransactionSpec } from "@codemirror/state";

export function hasSelectedText(state: EditorState): boolean {
  return state.selection.ranges.some((range) => !range.empty);
}

export function selectedText(state: EditorState): string {
  return state.selection.ranges
    .filter((range) => !range.empty)
    .map((range) => state.sliceDoc(range.from, range.to))
    .join(state.lineBreak);
}

export function removeSelectedText(state: EditorState): TransactionSpec {
  return state.changeByRange((range) => ({
    changes: { from: range.from, to: range.to, insert: "" },
    range: EditorSelection.cursor(range.from),
  }));
}
