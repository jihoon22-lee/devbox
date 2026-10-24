import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, expect, test, vi } from "vitest";
import { MAX_IMAGE_BYTES, pasteImage } from "./pasteImage";

const PNG = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
let view: EditorView | undefined;
afterEach(() => view?.destroy());

function editor(doc: string, cursor: number) {
  view = new EditorView({ state: EditorState.create({ doc, selection: { anchor: cursor } }) });
  return view;
}

test("a pasted PNG is attached once and its markdown lands at the cursor", async () => {
  const v = editor("before  after", 7);
  const attach = vi.fn(async () => ({
    path: "attachments/abc-shot.png",
    markdown: "![shot](attachments/abc-shot.png)",
  }));
  const onError = vi.fn();
  const file = new File([new Uint8Array(PNG)], "shot.png", { type: "image/png" });
  await pasteImage(v, file, { notePath: "a.md", attach, onError });
  expect(attach).toHaveBeenCalledTimes(1);
  expect(attach).toHaveBeenCalledWith({ notePath: "a.md", name: "shot.png", data: btoa(String.fromCharCode(...PNG)) });
  expect(v.state.doc.toString()).toBe("before ![shot](attachments/abc-shot.png) after");
  expect(onError).not.toHaveBeenCalled();
});

test("an image over 2 MiB is refused before any call and the error is shown", async () => {
  const v = editor("text", 4);
  const attach = vi.fn();
  const onError = vi.fn();
  const big = new File([new Uint8Array(MAX_IMAGE_BYTES + 1)], "big.png", { type: "image/png" });
  await pasteImage(v, big, { notePath: "a.md", attach, onError });
  expect(MAX_IMAGE_BYTES).toBe(2 * 1024 * 1024);
  expect(attach).not.toHaveBeenCalled();
  expect(onError).toHaveBeenCalledWith(expect.stringContaining("2 MiB"));
  expect(v.state.doc.toString()).toBe("text");
});

test("a failed attach inserts nothing and reports the error", async () => {
  const v = editor("text", 4);
  const attach = vi.fn(async () => {
    throw new Error("nope");
  });
  const onError = vi.fn();
  await pasteImage(v, new File([new Uint8Array(PNG)], "a.png", { type: "image/png" }), {
    notePath: "a.md",
    attach,
    onError,
  });
  expect(attach).toHaveBeenCalledTimes(1);
  expect(onError).toHaveBeenCalledTimes(1);
  expect(v.state.doc.toString()).toBe("text");
});
