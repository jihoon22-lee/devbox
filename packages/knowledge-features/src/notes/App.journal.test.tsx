import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import App from "./App";
import { NoteSessionProvider, useNoteSession } from "./lifecycle";
import type { NoteDocument } from "./noteDocument";
import type { NoteJournalView } from "./api";
const mock = vi.hoisted(() => ({
  load: vi.fn(),
  save: vi.fn(),
  clear: vi.fn(),
  discard: vi.fn(),
  read: vi.fn(),
  write: vi.fn(),
  changed: undefined as undefined | (() => void),
}));
vi.mock("./api", async (original) => ({
  ...(await original<typeof import("./api")>()),
  loadNoteJournal: mock.load,
  saveNoteJournal: mock.save,
  clearNoteJournal: mock.clear,
  discardOtherVaultJournal: mock.discard,
  readFile: mock.read,
  writeFile: mock.write,
  listTree: vi.fn(async () => []),
  listTags: vi.fn(async () => []),
  onDocsChanged: vi.fn(async (callback: () => void) => {
    mock.changed = callback;
    return () => {};
  }),
}));
vi.mock("./components/MarkdownEditor", () => ({
  default: ({ value, onChange }: { value: string; onChange: (s: string) => void }) => (
    <textarea aria-label="노트 본문" value={value} onChange={(e) => onChange(e.currentTarget.value)} />
  ),
}));
const entry = { path: "a.md", content: "recovered", baseRevision: "r1", savedAtMs: Date.UTC(2026, 8, 23) };
let document: NoteDocument;
function Capture() {
  document = useNoteSession()!;
  return <App />;
}
function mount() {
  return render(
    <NoteSessionProvider>
      <Capture />
    </NoteSessionProvider>,
  );
}
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
beforeEach(() => {
  vi.clearAllMocks();
  mock.load.mockReset().mockResolvedValue({ entries: [entry], otherVaultCount: 1 });
  mock.read.mockReset().mockResolvedValue({ content: "disk", revision: "r1" });
  mock.write.mockReset().mockImplementation(async (_p: string, content: string) => ({ content, revision: "r2" }));
  mock.save.mockReset().mockResolvedValue(undefined);
  mock.clear.mockReset().mockResolvedValue(undefined);
  mock.discard.mockReset().mockResolvedValue(undefined);
  localStorage.clear();
  vi.spyOn(window, "confirm").mockReturnValue(true);
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
it("confirms other-vault discard inline and refreshes the count", async () => {
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "다른 폴더 복구본 버리기" }));
  expect(mock.discard).not.toHaveBeenCalled();
  const confirm = screen.getByRole("region", { name: "복구본 삭제 확인" });
  await assertNoA11yViolations(confirm);
  fireEvent.click(screen.getByRole("button", { name: "복구본 삭제 취소" }));
  expect(mock.discard).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "다른 폴더 복구본 버리기" }));
  mock.load
    .mockResolvedValueOnce({ entries: [entry], otherVaultCount: 1 })
    .mockResolvedValueOnce({ entries: [entry], otherVaultCount: 0 });
  fireEvent.click(screen.getByRole("button", { name: "복구본 삭제 확인" }));
  await waitFor(() => expect(mock.discard).toHaveBeenCalledOnce());
  await waitFor(() => expect(screen.queryByText(/다른 노트 폴더에서 저장하지 않은/)).toBeNull());
});
it("keeps the list on failed explicit deletion", async () => {
  mock.discard.mockRejectedValueOnce(new Error("private failure"));
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "다른 폴더 복구본 버리기" }));
  fireEvent.click(screen.getByRole("button", { name: "복구본 삭제 확인" }));
  expect(await screen.findByText("복구본을 버리지 못했습니다. 복구본은 유지됩니다.")).toBeTruthy();
  expect(screen.getByText(/다른 노트 폴더에서 저장하지 않은 복구본 1개/)).toBeTruthy();
});
it("restores matching content and keeps its journal until a successful save", async () => {
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "a.md 열어서 확인" }));
  await waitFor(() => expect(document.snapshot().content).toBe("recovered"));
  expect(mock.clear).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "저장" }));
  await waitFor(() => expect(mock.clear).toHaveBeenCalledWith("a.md"));
  expect(mock.write).toHaveBeenCalledWith("a.md", "recovered", "r1");
});
it("keeps a comparison across metadata refresh but invalidates it when the document changes", async () => {
  mock.read.mockResolvedValue({ content: "external", revision: "r9" });
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "a.md 열어서 확인" }));
  await screen.findByRole("region", { name: "복구본 비교" });
  await act(async () => {
    mock.changed?.();
  });
  expect(screen.getByRole("region", { name: "복구본 비교" })).toBeTruthy();
  expect(mock.load).toHaveBeenCalledTimes(1);
  await act(async () => {
    await document.openPath("b.md", () => true);
  });
  expect(screen.queryByRole("region", { name: "복구본 비교" })).toBeNull();
  expect(document.snapshot().content).toBe("external");
});
it("pauses autosave after applying a changed-disk recovery", async () => {
  mock.read.mockResolvedValue({ content: "external", revision: "r9" });
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "a.md 열어서 확인" }));
  fireEvent.click(await screen.findByRole("button", { name: /복구본으로 바꾸기/ }));
  await waitFor(() => expect(document.snapshot().content).toBe("recovered"));
  fireEvent(window, new Event("blur"));
  await act(async () => {});
  expect(mock.write).not.toHaveBeenCalled();
});
it("loads the new activated folder and ignores a late response from the prior mount", async () => {
  const pending = deferred<NoteJournalView>();
  mock.load.mockReturnValueOnce(pending.promise);
  const first = mount();
  await waitFor(() => expect(mock.load).toHaveBeenCalledTimes(1));
  first.unmount();
  mock.load.mockResolvedValueOnce({ entries: [], otherVaultCount: 1 });
  mount();
  await screen.findByText(/다른 노트 폴더에서 저장하지 않은 복구본 1개/);
  await act(async () => {
    pending.resolve({ entries: [entry], otherVaultCount: 0 });
  });
  expect(screen.queryByRole("button", { name: "a.md 열어서 확인" })).toBeNull();
});
it("rejects a stale comparison even for a same-path edit", async () => {
  mock.read.mockResolvedValue({ content: "external", revision: "r9" });
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "a.md 열어서 확인" }));
  await screen.findByRole("region", { name: "복구본 비교" });
  act(() => document.edit("new editor content"));
  expect(screen.queryByRole("button", { name: /복구본으로 바꾸기/ })).toBeNull();
  expect(document.snapshot().content).toBe("new editor content");
});
it("does not delete an entry that changed since confirmation", async () => {
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "a.md 복구본 버리기" }));
  mock.load.mockResolvedValueOnce({
    entries: [{ ...entry, content: "newer draft", savedAtMs: entry.savedAtMs + 1 }],
    otherVaultCount: 1,
  });
  fireEvent.click(screen.getByRole("button", { name: "복구본 삭제 확인" }));
  await screen.findByText("복구 목록이 바뀌었습니다. 다시 확인해 주세요.");
  expect(mock.clear).not.toHaveBeenCalled();
});
it("does not resurrect discarded entries from a late reload", async () => {
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "다른 폴더 복구본 버리기" }));
  mock.discard.mockRejectedValueOnce(new Error("unavailable"));
  fireEvent.click(screen.getByRole("button", { name: "복구본 삭제 확인" }));
  await screen.findByText("복구본을 버리지 못했습니다. 복구본은 유지됩니다.");
  const old = deferred<NoteJournalView>();
  mock.load.mockReturnValueOnce(old.promise);
  fireEvent.click(screen.getByRole("button", { name: "복구 목록 다시 읽기" }));
  mock.load
    .mockResolvedValueOnce({ entries: [entry], otherVaultCount: 1 })
    .mockResolvedValueOnce({ entries: [entry], otherVaultCount: 0 });
  fireEvent.click(screen.getByRole("button", { name: "복구본 삭제 확인" }));
  await waitFor(() => expect(screen.queryByText(/다른 노트 폴더에서 저장하지 않은/)).toBeNull());
  await act(async () => old.resolve({ entries: [entry], otherVaultCount: 1 }));
  expect(screen.queryByText(/다른 노트 폴더에서 저장하지 않은/)).toBeNull();
});
