import { EditorSelection, EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import {
  hasSelectedText,
  insertMarkdownLink,
  removeSelectedText,
  selectedText,
} from "./editorActions";

describe("Markdown editor context actions", () => {
  it("collects and cuts every non-empty selection deterministically", () => {
    const state = EditorState.create({
      doc: "alpha beta gamma",
      selection: EditorSelection.create([
        EditorSelection.range(0, 5),
        EditorSelection.range(11, 16),
      ]),
      extensions: [EditorState.allowMultipleSelections.of(true)],
    });

    expect(hasSelectedText(state)).toBe(true);
    expect(selectedText(state)).toBe("alpha\ngamma");
    expect(state.update(removeSelectedText(state)).state.doc.toString()).toBe(" beta ");
  });

  it("wraps selected text and inserts a visible label for an empty selection", () => {
    const selected = EditorState.create({
      doc: "docs",
      selection: EditorSelection.range(0, 4),
    });
    expect(
      selected.update(insertMarkdownLink(selected, "https://example.com")).state.doc.toString(),
    ).toBe("[docs](https://example.com)");

    const empty = EditorState.create({ doc: "start ", selection: { anchor: 6 } });
    expect(
      empty.update(insertMarkdownLink(empty, "note.md")).state.doc.toString(),
    ).toBe("start [링크](note.md)");
  });
});
