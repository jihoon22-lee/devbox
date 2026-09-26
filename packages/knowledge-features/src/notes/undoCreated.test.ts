import { describe, expect, it, vi } from "vitest";
import { NoteDocument } from "./noteDocument";
import { undoCreated } from "./undoCreated";

const receipt = { path: "Inbox/new.md", revision: "r1" };
function document() {
  return new NoteDocument(
    async () => ({ content: "created", revision: "r1" }),
    async () => ({ content: "", revision: "r2" }),
  );
}
describe("created-note undo", () => {
  it("refuses an unsaved editor draft before invoking native deletion", async () => {
    const editor = document();
    await editor.open(
      async () => ({ ...receipt, content: "created" }),
      () => true,
    );
    editor.edit("unsaved");
    const remove = vi.fn(async () => ({ removed: true }));
    await expect(undoCreated(receipt, editor, remove)).rejects.toThrow("이미 수정되어 되돌리지 않았습니다.");
    expect(remove).not.toHaveBeenCalled();
    expect(editor.snapshot().content).toBe("unsaved");
  });
  it("clears an unchanged open buffer only after native deletion succeeds", async () => {
    const editor = document();
    await editor.open(
      async () => ({ ...receipt, content: "created" }),
      () => true,
    );
    const remove = vi.fn(async () => ({ removed: true }));
    await undoCreated(receipt, editor, remove);
    expect(remove).toHaveBeenCalledWith(receipt.path, receipt.revision);
    expect(editor.snapshot().path).toBeNull();
  });
  it("preserves edits made while deletion is in flight", async () => {
    const editor = document();
    await editor.open(
      async () => ({ ...receipt, content: "created" }),
      () => true,
    );
    await undoCreated(receipt, editor, async () => {
      editor.edit("concurrent draft");
      return { removed: true };
    });
    expect(editor.snapshot()).toMatchObject({ content: "concurrent draft", dirty: true });
  });
  it("keeps the open document on native revision refusal", async () => {
    const editor = document();
    await editor.open(
      async () => ({ ...receipt, content: "created" }),
      () => true,
    );
    await expect(undoCreated(receipt, editor, async () => ({ removed: false }))).rejects.toThrow(
      "이미 수정되어 되돌리지 않았습니다.",
    );
    expect(editor.snapshot().path).toBe(receipt.path);
  });
});
