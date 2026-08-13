import { describe, expect, it } from "vitest";
import {
  completionOptions,
  diagnosticsForCodeMirror,
  offsetForPosition,
  pathFromFileUri,
  positionForOffset,
} from "./lspFeatures";

describe("LSP editor position and presentation boundaries", () => {
  it("preserves UTF-16 and UTF-8 positions around astral characters", () => {
    const text = "🙂 café\n다음";
    expect(positionForOffset(text, 3, "utf-16")).toEqual({ line: 0, character: 3 });
    expect(positionForOffset(text, 3, "utf-8")).toEqual({ line: 0, character: 5 });
    expect(offsetForPosition(text, { line: 0, character: 3 }, "utf-16")).toBe(3);
    expect(offsetForPosition(text, { line: 0, character: 5 }, "utf-8")).toBe(3);
  });

  it("maps diagnostics to bounded CodeMirror ranges and ignores stale results", () => {
    const result = {
      uri: "file:///work/main.rs",
      version: 2,
      origin: "pull" as const,
      stale: false,
      diagnostics: [{
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 2 } },
        severity: 1,
        message: "bad",
        source: "fake",
        code: "E1",
      }],
    };
    expect(diagnosticsForCodeMirror("fn", result)).toMatchObject([
      { from: 0, to: 2, severity: "error", message: "bad", source: "fake E1" },
    ]);
    expect(diagnosticsForCodeMirror("fn", { ...result, stale: true })).toEqual([]);
  });

  it("normalizes local Windows and UNC file URIs without using non-file URIs", () => {
    expect(pathFromFileUri("file:///C:/work/main.rs")).toBe("C:/work/main.rs");
    expect(pathFromFileUri("file://server/share/main.rs")).toBe("//server/share/main.rs");
    expect(pathFromFileUri("https://example.test/main.rs")).toBeNull();
    expect(pathFromFileUri("not a uri")).toBeNull();
  });

  it("applies a UTF-8 text edit range exactly and rejects stale documents", () => {
    const applied: Array<{ from: number; to: number; insert: string }> = [];
    const options = completionOptions({
      isIncomplete: false,
      items: [{
        label: "replace",
        textEdit: {
          range: { start: { line: 0, character: 5 }, end: { line: 0, character: 7 } },
          newText: "new",
        },
        additionalTextEdits: [{ range: {}, newText: "must be ignored" }],
      }],
    }, {
      text: "🙂 old",
      encoding: "utf-8",
      isCurrent: () => true,
    });
    const view = {
      state: {
        doc: { toString: () => "🙂 old", length: "🙂 old".length },
      },
      dispatch: (transaction: { changes: { from: number; to: number; insert: string } }) => applied.push(transaction.changes),
    } as never;
    if (typeof options[0].apply !== "function") throw new Error("expected range-aware completion");
    options[0].apply(view, options[0], 0, 0);
    expect(applied).toEqual([{ from: 3, to: 5, insert: "new" }]);

    const insertReplace = completionOptions({
      isIncomplete: false,
      items: [{
        label: "replace",
        insertText: "wrong fallback",
        textEdit: {
          insert: { start: { line: 0, character: 5 }, end: { line: 0, character: 7 } },
          replace: { start: { line: 0, character: 5 }, end: { line: 0, character: 7 } },
          newText: "right",
        },
      }],
    }, { text: "🙂 old", encoding: "utf-8", isCurrent: () => true });
    if (typeof insertReplace[0].apply !== "function") throw new Error("expected insert/replace completion");
    insertReplace[0].apply(view, insertReplace[0], 0, 0);
    expect(applied[applied.length - 1]).toEqual({ from: 3, to: 5, insert: "right" });

    const stale = completionOptions({
      isIncomplete: false,
      items: [{ label: "snippet", insertText: "${1:bad}", insertTextFormat: 2 }],
    }, { text: "🙂 old", encoding: "utf-16", isCurrent: () => false });
    const before = applied.length;
    if (typeof stale[0].apply !== "function") throw new Error("expected guarded completion");
    stale[0].apply(view, stale[0], 0, 0);
    expect(applied).toHaveLength(before);
    expect(stale[0].apply).toBeTypeOf("function");
  });
});
