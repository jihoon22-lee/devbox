import { beforeEach, expect, it, vi } from "vitest";
const native = vi.hoisted(() => ({ invoke: vi.fn(), enabled: true }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ readText: vi.fn() }));
vi.mock("./lib/isTauri", () => ({ isTauri: () => native.enabled }));
import { saveNoteJournal, clearNoteJournal, loadNoteJournal, discardOtherVaultJournal } from "./api";
beforeEach(() => {
  native.invoke.mockReset();
  native.enabled = true;
});
it("keeps write arguments relative and loads only the projected view", async () => {
  await saveNoteJournal("a.md", "draft", "r");
  expect(native.invoke).toHaveBeenLastCalledWith("save_note_journal", {
    path: "a.md",
    content: "draft",
    baseRevision: "r",
  });
  await clearNoteJournal("a.md");
  expect(native.invoke).toHaveBeenLastCalledWith("clear_note_journal", { path: "a.md" });
  native.invoke.mockResolvedValueOnce({ entries: [], otherVaultCount: 2 });
  expect(await loadNoteJournal()).toEqual({ entries: [], otherVaultCount: 2 });
  await discardOtherVaultJournal();
  expect(native.invoke).toHaveBeenLastCalledWith("discard_other_vault_journal", {});
});
it("has an empty browser view and no native side effects", async () => {
  native.enabled = false;
  expect(await loadNoteJournal()).toEqual({ entries: [], otherVaultCount: 0 });
  await saveNoteJournal("a.md", "draft", "r");
  await clearNoteJournal("a.md");
  await discardOtherVaultJournal();
  expect(native.invoke).not.toHaveBeenCalled();
});
