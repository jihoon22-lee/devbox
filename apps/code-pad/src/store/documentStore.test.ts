import { describe, expect, it } from "vitest";
import {
  createInitialEditorState,
  editorReducer,
  isEditorStateInvariantValid,
  stateToSession,
} from "./documentStore";
import type { Doc } from "../types";

function doc(id: string, path = `/workspace/${id}.ts`): Doc {
  return {
    id,
    path,
    text: `const ${id} = true;\n`,
    encoding: { encodingKind: "utf8", bom: false },
    lineEnding: "lf",
    readOnly: false,
    size: 20,
    mtimeNanos: "100",
    contentHash: "hash",
    lossy: false,
    dirty: false,
    revision: 0,
    cursor: 0,
    bookmarks: [],
  };
}

function withDocs(...docs: Doc[]) {
  return docs.reduce((state, item) => editorReducer(state, { type: "addDoc", doc: item }), createInitialEditorState());
}

describe("document registry transitions", () => {
  it("blocks duplicate document IDs without changing docs or adding a second placement", () => {
    const first = withDocs(doc("one"));
    const duplicate = editorReducer(first, { type: "addDoc", doc: { ...doc("one"), text: "new" }, view: 1 });

    expect(duplicate.docs).toHaveLength(1);
    expect(duplicate.docs[0].text).toBe(first.docs[0].text);
    expect(duplicate.views).toEqual([["one"], []]);
    expect(duplicate.activeView).toBe(0);
    expect(isEditorStateInvariantValid(duplicate)).toBe(true);
  });

  it("keeps the append-only docs order when moving a document between views", () => {
    let state = withDocs(doc("one"), doc("two"), doc("three"));
    state = editorReducer(state, { type: "moveDoc", docId: "two", toView: 1 });
    state = editorReducer(state, { type: "moveDoc", docId: "one", toView: 1 });

    expect(state.docs.map((item) => item.id)).toEqual(["one", "two", "three"]);
    expect(state.views).toEqual([["three"], ["two", "one"]]);
    expect(state.activeView).toBe(1);
    expect(state.activeDocByView).toEqual(["three", "one"]);
    expect(isEditorStateInvariantValid(state)).toBe(true);
  });

  it("selects a neighboring tab when the active tab is closed, including the last tab", () => {
    let state = withDocs(doc("one"), doc("two"), doc("three"));
    state = editorReducer(state, { type: "activateDoc", view: 0, docId: "two" });
    state = editorReducer(state, { type: "removeDoc", docId: "two" });
    expect(state.views[0]).toEqual(["one", "three"]);
    expect(state.activeDocByView[0]).toBe("three");

    state = editorReducer(state, { type: "removeDoc", docId: "three" });
    state = editorReducer(state, { type: "removeDoc", docId: "one" });
    expect(state.docs).toEqual([]);
    expect(state.views).toEqual([[], []]);
    expect(state.activeDocByView).toEqual([null, null]);
  });

  it("merges the second view without reordering the global registry", () => {
    let state = withDocs(doc("one"), doc("two"), doc("three"));
    state = editorReducer(state, { type: "moveDoc", docId: "two", toView: 1 });
    state = editorReducer(state, { type: "toggleSplit" });

    expect(state.split).toBe(false);
    expect(state.views).toEqual([["one", "three", "two"], []]);
    expect(state.docs.map((item) => item.id)).toEqual(["one", "two", "three"]);
    expect(isEditorStateInvariantValid(state)).toBe(true);
  });

  it("serializes session metadata without the editor buffer", () => {
    const state = editorReducer(createInitialEditorState(), { type: "addDoc", doc: doc("one") });
    const session = stateToSession(state);

    expect(session.docs[0]).toEqual({ id: "one", path: "/workspace/one.ts", cursor: 0, bookmarks: [] });
    expect(JSON.stringify(session)).not.toContain("const one");
  });

  it("hydrates only existing documents while preserving exact view coverage", () => {
    const session = {
      version: 1 as const,
      workspace_folder: "/workspace",
      docs: [
        { id: "one", path: "/workspace/one.ts", cursor: 4, bookmarks: [] },
        { id: "missing", path: "/workspace/missing.ts", cursor: 0, bookmarks: [] },
      ],
      views: [["missing", "one"], []] as [string[], string[]],
      active_view: 0 as const,
      active_doc_by_view: ["one", null] as [string | null, string | null],
      recent_files: [],
    };
    const restored = editorReducer(createInitialEditorState(), {
      type: "restoreSession",
      session,
      docs: [doc("one")],
    });

    expect(restored.docs.map((item) => item.id)).toEqual(["one"]);
    expect(restored.views).toEqual([["one"], []]);
    expect(restored.activeDocByView).toEqual(["one", null]);
    expect(isEditorStateInvariantValid(restored)).toBe(true);
  });

  it("moves the active view to the surviving pane when its preferred pane is empty", () => {
    const session = {
      version: 1 as const,
      workspace_folder: null,
      docs: [{ id: "two", path: "/workspace/two.ts", cursor: 0, bookmarks: [] }],
      views: [[], ["two"]] as [string[], string[]],
      active_view: 0 as const,
      active_doc_by_view: [null, "two"] as [string | null, string | null],
      recent_files: [],
    };
    const restored = editorReducer(createInitialEditorState(), {
      type: "restoreSession",
      session,
      docs: [doc("two")],
    });

    expect(restored.activeView).toBe(1);
    expect(restored.activeDocByView).toEqual([null, "two"]);
    expect(isEditorStateInvariantValid(restored)).toBe(true);
  });

  it("keeps a newer edit dirty while adopting the completed save snapshot", () => {
    let state = withDocs(doc("one"));
    state = editorReducer(state, { type: "setDocText", docId: "one", text: "first save\n" });
    const submitted = state.docs[0];
    state = editorReducer(state, { type: "setDocText", docId: "one", text: "newer edit\n" });
    state = editorReducer(state, {
      type: "saveDoc",
      docId: "one",
      submittedRevision: submitted.revision,
      submittedText: submitted.text,
      mtimeNanos: "200",
      size: 99,
      contentHash: "hash-200",
      durabilityWarning: "flush warning",
    });

    expect(state.docs[0]).toMatchObject({
      text: "newer edit\n",
      dirty: true,
      revision: submitted.revision + 1,
      mtimeNanos: "200",
      size: 99,
      contentHash: "hash-200",
      durabilityWarning: "flush warning",
    });
  });

  it("clears dirty only when both save revision and text still match", () => {
    let state = withDocs(doc("one"));
    state = editorReducer(state, { type: "setDocText", docId: "one", text: "saved\n" });
    const submitted = state.docs[0];
    state = editorReducer(state, {
      type: "saveDoc",
      docId: "one",
      submittedRevision: submitted.revision,
      submittedText: submitted.text,
      mtimeNanos: "201",
      size: 6,
      contentHash: "hash-201",
    });
    expect(state.docs[0].dirty).toBe(false);
  });

  it("rejects a state that omits a registered document from both views", () => {
    const state = withDocs(doc("one"), doc("two"));
    expect(isEditorStateInvariantValid({ ...state, views: [["one"], []] })).toBe(false);
  });

  it("falls back to the last remaining ID when the active reference is stale", () => {
    const state = withDocs(doc("one"), doc("two"));
    const malformed = {
      ...state,
      activeDocByView: ["missing", null] as [string | null, string | null],
    };
    const removed = editorReducer(malformed, { type: "removeDoc", docId: "one" });
    expect(removed.views[0]).toEqual(["two"]);
    expect(removed.activeDocByView[0]).toBe("two");
    expect(isEditorStateInvariantValid(removed)).toBe(true);
  });

  it("marks encoding and line-ending conversions dirty without changing the LF buffer", () => {
    let state = withDocs(doc("one"));
    const originalText = state.docs[0].text;
    const originalRevision = state.docs[0].revision;
    state = editorReducer(state, {
      type: "setEncoding",
      docId: "one",
      encoding: { encodingKind: "cp949", bom: false },
    });
    state = editorReducer(state, { type: "setLineEnding", docId: "one", lineEnding: "crlf" });

    expect(state.docs[0].text).toBe(originalText);
    expect(state.docs[0].encoding).toEqual({ encodingKind: "cp949", bom: false });
    expect(state.docs[0].lineEnding).toBe("crlf");
    expect(state.docs[0].dirty).toBe(true);
    expect(state.docs[0].revision).toBe(originalRevision + 2);
  });

  it("keeps persisted bookmark lines unique and within the current buffer", () => {
    let state = withDocs(doc("one"));
    state = editorReducer(state, {
      type: "setBookmarks",
      docId: "one",
      bookmarks: [99, 1, 0, 1],
    });
    expect(state.docs[0].bookmarks).toEqual([0, 1]);
    expect(stateToSession(state).docs[0].bookmarks).toEqual([0, 1]);
  });
});
