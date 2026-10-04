import { confirmAction } from "@devbox/product-shell/confirm";
vi.mock("@devbox/product-shell/confirm", () => ({ confirmAction: vi.fn().mockResolvedValue(true) }));
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import RecoveryControls from "./RecoveryControls";
import { NoteDocument } from "../noteDocument";
const mock = vi.hoisted(() => ({ load: vi.fn(), clear: vi.fn(), read: vi.fn(), write: vi.fn(), create: vi.fn() }));
vi.mock("../api", () => ({
  loadNoteJournal: mock.load,
  clearNoteJournal: mock.clear,
  readFile: mock.read,
  writeFile: mock.write,
  createFile: mock.create,
  discardOtherVaultJournal: vi.fn(),
}));
const entry = { path: "missing.md", content: "RECOVERY_ONLY", baseRevision: "old", savedAtMs: 1 };
const copy = vi.fn().mockResolvedValue(undefined);
beforeEach(() => {
  vi.clearAllMocks();
  mock.load.mockResolvedValue({ entries: [entry], otherVaultCount: 0 });
  mock.read.mockResolvedValue({ content: null, revision: "missing-CAS" });
  mock.write.mockResolvedValue({ content: entry.content, revision: "published" });
  mock.clear.mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: copy } });
  vi.mocked(confirmAction).mockResolvedValue(true);
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
function mount() {
  const doc = new NoteDocument(mock.read, mock.write);
  render(
    <RecoveryControls document={doc} autosave={{ current: null }} journal={{ current: null }} onBusy={() => {}} />,
  );
  return doc;
}
it("previews and copies a deleted original without creating it, and uses missing revision only on explicit restore", async () => {
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "missing.md 열어서 확인" }));
  await screen.findByRole("button", { name: "삭제된 노트 재생성" });
  expect(screen.getByText("RECOVERY_ONLY")).toBeTruthy();
  expect(mock.write).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "복구본 복사" }));
  await waitFor(() => expect(copy).toHaveBeenCalledWith(entry.content));
  expect(mock.clear).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "삭제된 노트 재생성" }));
  await waitFor(() => expect(mock.write).toHaveBeenCalledWith(entry.path, entry.content, "missing-CAS"));
  await waitFor(() => expect(mock.clear).toHaveBeenCalledWith(entry.path));
});
it("keeps missing-note journal when recreation collides with an external creator", async () => {
  mock.write.mockRejectedValueOnce(new Error("note_conflict"));
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "missing.md 열어서 확인" }));
  fireEvent.click(await screen.findByRole("button", { name: "삭제된 노트 재생성" }));
  await screen.findByRole("alert");
  expect(mock.clear).not.toHaveBeenCalled();
  expect(screen.getByText("RECOVERY_ONLY")).toBeTruthy();
});
it("keeps preview and copy available offline and offers no mutation", async () => {
  mock.read.mockRejectedValueOnce(new Error("offline"));
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "missing.md 열어서 확인" }));
  await screen.findByRole("alert");
  expect(screen.getByText("RECOVERY_ONLY")).toBeTruthy();
  expect(screen.getByRole("button", { name: "복구본 복사" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "삭제된 노트 재생성" })).toBeNull();
  expect(mock.clear).not.toHaveBeenCalled();
});
it("ignores a late missing source probe after another document opens", async () => {
  let finish!: (value: { content: null; revision: string }) => void;
  mock.read.mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  const doc = mount();
  fireEvent.click(await screen.findByRole("button", { name: "missing.md 열어서 확인" }));
  await waitFor(() => expect(mock.read).toHaveBeenCalled());
  await act(async () => {
    await doc.open(
      async () => ({ path: "B.md", content: "B", revision: "B-r" }),
      () => true,
    );
    finish({ content: null, revision: "missing" });
  });
  expect(doc.snapshot().content).toBe("B");
  expect(screen.queryByRole("button", { name: "삭제된 노트 재생성" })).toBeNull();
});

it("waits for restore confirmation and cancel preserves the journal", async () => {
  let answer!: (value: boolean) => void;
  vi.mocked(confirmAction).mockReturnValueOnce(
    new Promise<boolean>((resolve) => {
      answer = resolve;
    }),
  );
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "missing.md 열어서 확인" }));
  fireEvent.click(await screen.findByRole("button", { name: "삭제된 노트 재생성" }));
  expect(mock.write).not.toHaveBeenCalled();
  expect(mock.clear).not.toHaveBeenCalled();
  await act(async () => {
    answer(false);
  });
  expect(mock.write).not.toHaveBeenCalled();
  expect(mock.clear).not.toHaveBeenCalled();
});
