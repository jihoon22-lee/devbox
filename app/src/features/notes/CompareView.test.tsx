import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test } from "vitest";
import { keyId, memoryJournal } from "../documents/offlineJournal";
import { useDocuments } from "../documents/store";
import { FakeDocumentsDaemon } from "../documents/test-daemon";
import { CompareView } from "./CompareView";

const key = { root: "vault", path: "a.md" };
const docs = () => useDocuments.getState();
const doc = () => docs().find(key);
let daemon: FakeDocumentsDaemon;
let stop: () => void;

beforeEach(async () => {
  daemon = new FakeDocumentsDaemon();
  daemon.write(key, "line one\nline two\n");
  stop = docs().connect(daemon, { journal: memoryJournal(), windowId: "main" });
  docs().open(key, { kind: "note" });
  await docs().settled();
  docs().edit(key, "line one\nmine\n");
  docs().dispatch(doc()?.id as string, { type: "journalTick" });
  await docs().settled();
  const rev = daemon.write(key, "line one\ndisk\n");
  act(() => daemon.emit("documents.changed", { key, rev }));
  await docs().settled();
  expect(doc()).toMatchObject({ state: "Conflict", disk: { text: "line one\ndisk\n" } });
});
afterEach(() => {
  cleanup();
  stop();
});

test("[디스크 것] resolves to the disk and drops both journals (AR-F13)", async () => {
  render(<CompareView docKey={key} />);
  expect(screen.getByText("mine")).toBeVisible();
  expect(screen.getByText("disk")).toBeVisible();
  await userEvent.click(screen.getByRole("button", { name: "디스크 것" }));
  await docs().settled();
  expect(daemon.of("documents.journal_drop")).toEqual([expect.objectContaining({ key, uptoSeq: 1 })]);
  expect(daemon.journals.has(keyId(key))).toBe(false);
  expect(doc()).toMatchObject({ state: "Clean", text: "line one\ndisk\n" });
  docs().close(key);
  await docs().settled();
  docs().open(key, { kind: "note" });
  await docs().settled();
  expect(doc()?.state).toBe("Clean");
});

test("[내 것 유지] saves the buffer over the newer disk", async () => {
  render(<CompareView docKey={key} />);
  await userEvent.click(screen.getByRole("button", { name: "내 것 유지" }));
  await docs().settled();
  expect(daemon.files.get(keyId(key))?.text).toBe("line one\nmine\n");
  expect(doc()?.state).toBe("Clean");
});

test("[직접 병합] saves the merged text", async () => {
  render(<CompareView docKey={key} />);
  await userEvent.click(screen.getByRole("button", { name: "직접 병합" }));
  const box = screen.getByRole("textbox", { name: "병합한 내용" });
  await userEvent.clear(box);
  await userEvent.type(box, "merged");
  await userEvent.click(screen.getByRole("button", { name: "병합한 내용 저장" }));
  docs().save(key);
  await docs().settled();
  expect(daemon.files.get(keyId(key))?.text).toBe("merged");
});

test("manual merge is discarded when switching to another conflicted note", async () => {
  const b = { root: "vault", path: "b.md" };
  daemon.write(b, "original b");
  docs().open(b, { kind: "note" });
  await docs().settled();
  docs().edit(b, "mine b");
  const rev = daemon.write(b, "disk b");
  daemon.emit("documents.changed", { key: b, rev });
  await docs().settled();
  const view = render(<CompareView docKey={key} />);
  await userEvent.click(screen.getByRole("button", { name: "직접 병합" }));
  view.rerender(<CompareView docKey={b} />);
  expect(screen.queryByRole("textbox", { name: "병합한 내용" })).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "직접 병합" }));
  expect(screen.getByRole("textbox", { name: "병합한 내용" })).toHaveValue("mine b");
  await userEvent.click(screen.getByRole("button", { name: "병합한 내용 저장" }));
  await act(async () => {
    docs().save(b);
    await docs().settled();
  });
  expect(daemon.files.get(keyId(b))?.text).toBe("mine b");
});
