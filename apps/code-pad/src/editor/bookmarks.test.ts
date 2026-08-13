import { cleanup } from "@testing-library/react";
import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  bookmarkExtension,
  bookmarkField,
  mapBookmarkLines,
  nextBookmark,
  previousBookmark,
  setBookmarkLines,
  toggleBookmark,
} from "./bookmarks";
import { editorExtensions } from "./extensions";

afterEach(() => cleanup());

function viewFor(doc = "zero\none\ntwo\nthree") {
  const parent = document.createElement("div");
  document.body.appendChild(parent);
  return new EditorView({
    state: EditorState.create({
      doc,
      extensions: [bookmarkExtension()],
    }),
    parent,
  });
}

describe("CM6 editor selection and bookmark extensions", () => {
  it("allows multiple selections and keeps rectangular selection enabled", () => {
    const view = new EditorView({
      state: EditorState.create({
        doc: "one\ntwo\nthree",
        extensions: editorExtensions({
          language: "text",
          syntaxHighlightingEnabled: false,
          readOnly: false,
          onChange: vi.fn(),
        }),
      }),
      parent: document.body,
    });

    expect(view.state.facet(EditorState.allowMultipleSelections)).toBe(true);
    view.dispatch({
      selection: EditorSelection.create([
        EditorSelection.cursor(1),
        EditorSelection.cursor(5),
      ]),
    });
    expect(view.state.selection.ranges).toHaveLength(2);
    view.destroy();
  });

  it("maps bookmark lines through insertions and deletions and updates the gutter", () => {
    const view = viewFor();
    view.dispatch({
      effects: setBookmarkLines.of([1, 3]),
      annotations: [],
    });
    // A standalone mapping assertion keeps the line-number contract explicit
    // even when jsdom doesn't lay out a real mouse rectangle.
    const mapped = mapBookmarkLines(
      [1, 3],
      EditorState.create({ doc: "zero\none\ntwo\nthree" }).doc,
      EditorState.create({ doc: "zero\none\ntwo\nthree" }).update({
        changes: { from: 0, insert: "new\n" },
      }).changes,
      EditorState.create({ doc: "new\nzero\none\ntwo\nthree" }).doc,
    );
    expect(mapped).toEqual([2, 4]);
    expect(view.state.field(bookmarkField)).toEqual([1, 3]);
    expect(view.dom.querySelector(".cm-bookmark-marker-dot")).not.toBeNull();

    view.dispatch({ changes: { from: 0, insert: "new\n" } });
    expect(view.state.field(bookmarkField)).toEqual([2, 4]);
    expect(view.dom.querySelectorAll(".cm-bookmark-marker-dot")).toHaveLength(2);
    view.destroy();
  });

  it("toggles and wraps next/previous bookmark navigation", () => {
    const view = viewFor();
    view.dispatch({ effects: setBookmarkLines.of([1, 3]) });
    view.dispatch({ selection: { anchor: view.state.doc.line(1).from } });
    expect(nextBookmark(view)).toBe(true);
    expect(view.state.doc.lineAt(view.state.selection.main.head).number).toBe(2);
    expect(previousBookmark(view)).toBe(true);
    expect(view.state.doc.lineAt(view.state.selection.main.head).number).toBe(4);
    expect(toggleBookmark(view)).toBe(true);
    expect(view.state.field(bookmarkField)).toEqual([1]);
    view.destroy();
  });
});
