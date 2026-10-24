import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { RpcError } from "../../rpc/client";
import { confirmPolicy } from "../../rpc/query";
import { useToasts } from "../../ui";
import { ToastHost } from "../../ui/Toast";
import { memoryJournal } from "../documents/offlineJournal";
import { useDocuments } from "../documents/store";
import { NotesWire } from "./test-wire";
import { moveToTrash } from "./trash";

let wire: NotesWire;
let stop: () => void;
const key = { root: "vault", path: "a.md" };
beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  wire = new NotesWire();
  wire.docs.write(key, "alpha");
  wire.handlers["notes.delete"] = () => ({ trashPath: ".trash/2026-10-24/a.md" });
  wire.handlers["notes.restore"] = () => ({ rev: "3" });
  wire.client.start();
  stop = useDocuments.getState().connect(wire.docs, { journal: memoryJournal(), windowId: "main" });
  render(<ToastHost />);
});
afterEach(() => {
  for (const toast of useToasts.getState().items) useToasts.getState().dismiss(toast.id);
  cleanup();
  stop();
  wire.client.stop();
  vi.useRealTimers();
});
const ready = () => vi.waitFor(() => expect(wire.client.state).toBe("ready"));

test("notes.delete is reversible, so trashing shows a 10 s undo that restores the same path", async () => {
  expect(confirmPolicy("notes.delete")).toBe("reversible");
  await ready();
  await act(() => moveToTrash(wire.client, "a.md"));
  expect(wire.requests("notes.delete")).toEqual([{ path: "a.md" }]);
  act(() => screen.getByRole("button", { name: "되돌리기" }).click());
  await vi.waitFor(() =>
    expect(wire.requests("notes.restore")).toEqual([{ trashPath: ".trash/2026-10-24/a.md", path: "a.md" }]),
  );
});

test("after 10 s the undo is gone and nothing is restored", async () => {
  await ready();
  await act(() => moveToTrash(wire.client, "a.md"));
  await act(() => vi.advanceTimersByTimeAsync(9_900));
  expect(screen.getByRole("button", { name: "되돌리기" })).toBeVisible();
  await act(() => vi.advanceTimersByTimeAsync(101));
  expect(screen.queryByRole("button", { name: "되돌리기" })).toBeNull();
  expect(useToasts.getState().undoLatest()).toBe(false);
  expect(wire.requests("notes.restore")).toEqual([]);
});

test("a restore refused because the name came back shows why", async () => {
  await ready();
  wire.handlers["notes.restore"] = () => {
    throw new RpcError("notes.exists");
  };
  await act(() => moveToTrash(wire.client, "a.md"));
  act(() => screen.getByRole("button", { name: "되돌리기" }).click());
  expect(await screen.findByText("같은 이름의 노트나 파일이 이미 있습니다.")).toBeVisible();
});

test("an open note with unsaved edits is not trashed; an open clean note is closed first", async () => {
  await ready();
  const docs = useDocuments.getState();
  docs.open(key, { kind: "note" });
  await docs.settled();
  docs.edit(key, "alpha!");
  await act(() => moveToTrash(wire.client, "a.md"));
  expect(wire.requests("notes.delete")).toEqual([]);
  expect(screen.getByText("저장되지 않은 내용이 있어 휴지통으로 옮기지 않았습니다")).toBeVisible();
  docs.save(key);
  await docs.settled();
  await act(() => moveToTrash(wire.client, "a.md"));
  expect(wire.requests("notes.delete")).toEqual([{ path: "a.md" }]);
  expect(useDocuments.getState().find(key)).toBeUndefined();
  expect(wire.docs.leases.size).toBe(0);
});
