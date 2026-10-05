import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import QuitGuard from "./QuitGuard";
import { registerNoteEditor } from "@devbox/knowledge-features/notes-lifecycle";
const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@devbox/product-shell/api", () => ({ nativeMode: true }));
vi.mock("@devbox/knowledge-features/transport", () => ({ componentInvoke: () => invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
let unregister: (() => void) | undefined;
beforeEach(() => {
  HTMLDialogElement.prototype.showModal = vi.fn(function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  });
  HTMLDialogElement.prototype.close = vi.fn(function (this: HTMLDialogElement) {
    this.removeAttribute("open");
  });
  invoke
    .mockReset()
    .mockImplementation((method: string) => Promise.resolve(method === "pending_quit" ? "quit-1" : null));
  listen.mockReset().mockResolvedValue(() => undefined);
});
afterEach(() => {
  cleanup();
  unregister?.();
  unregister = undefined;
});
it("accepts a clean exit after installing the wake listener", async () => {
  render(<QuitGuard />);
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("decide_quit", { id: "quit-1", quit: true }));
  expect(listen).toHaveBeenCalledWith("knowledge://quit-request", expect.any(Function));
});
it("cancels a dirty exit without saving or terminating collection", async () => {
  const save = vi.fn(async () => true);
  unregister = registerNoteEditor({ unsaved: () => true, saveBeforeQuit: save, settleBeforeQuit: async () => {} });
  render(<QuitGuard />);
  await screen.findByRole("dialog");
  fireEvent.click(screen.getByRole("button", { name: "종료 취소" }));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("decide_quit", { id: "quit-1", quit: false }));
  expect(save).not.toHaveBeenCalled();
});
it("keeps the editor alive on save failure, then accepts a successful retry", async () => {
  const save = vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(true);
  unregister = registerNoteEditor({ unsaved: () => true, saveBeforeQuit: save, settleBeforeQuit: async () => {} });
  render(<QuitGuard />);
  await screen.findByRole("dialog");
  fireEvent.click(screen.getByRole("button", { name: "저장하고 종료" }));
  await screen.findByRole("alert");
  expect(invoke.mock.calls.some(([method]) => method === "decide_quit")).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "저장하고 종료" }));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("decide_quit", { id: "quit-1", quit: true }));
});
it("waits for an in-flight write before explicit discard and ignores composing Escape", async () => {
  let finish!: () => void;
  const pending = new Promise<void>((resolve) => {
    finish = resolve;
  });
  unregister = registerNoteEditor({
    unsaved: () => true,
    saveBeforeQuit: async () => false,
    settleBeforeQuit: () => pending,
  });
  render(<QuitGuard />);
  const dialog = await screen.findByRole("dialog");
  expect(fireEvent.keyDown(dialog, { key: "Escape", isComposing: true })).toBe(false);
  expect(invoke.mock.calls.some(([method]) => method === "decide_quit")).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "저장하지 않고 종료(복구본 유지)" }));
  expect(invoke.mock.calls.some(([method]) => method === "decide_quit")).toBe(false);
  await act(async () => {
    finish();
    await pending;
  });
  expect(invoke).toHaveBeenCalledWith("decide_quit", { id: "quit-1", quit: true });
});

it("requires separate confirmation for permanent discard and rejects failed deletion", async () => {
  const prepare = vi.fn().mockRejectedValueOnce(new Error("journal unavailable"));
  unregister = registerNoteEditor({
    unsaved: () => true,
    saveBeforeQuit: async () => true,
    settleBeforeQuit: async () => {},
    prepareQuit: prepare,
  });
  render(<QuitGuard />);
  fireEvent.click(await screen.findByRole("button", { name: "복구본 영구 삭제…" }));
  expect(prepare).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "영구 삭제하고 종료" }));
  await screen.findByRole("alert");
  expect(prepare).toHaveBeenCalledWith(true);
  expect(invoke.mock.calls.some(([method]) => method === "decide_quit")).toBe(false);
});

it("owns a native modal until cancellation is acknowledged, then restores focus and resets confirmation", async () => {
  let acknowledge!: () => void;
  invoke.mockImplementation((method: string) =>
    method === "pending_quit"
      ? Promise.resolve("quit-1")
      : method === "decide_quit"
        ? new Promise<void>((resolve) => {
            acknowledge = resolve;
          })
        : Promise.resolve(null),
  );
  unregister = registerNoteEditor({
    unsaved: () => true,
    saveBeforeQuit: async () => true,
    settleBeforeQuit: async () => {},
  });
  const origin = document.createElement("button");
  document.body.append(origin);
  origin.focus();
  try {
    render(<QuitGuard />);
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toBeInstanceOf(HTMLDialogElement);
    expect(HTMLDialogElement.prototype.showModal).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "복구본 영구 삭제…" }));
    fireEvent.click(screen.getByRole("button", { name: "종료 취소" }));
    const nativeCancel = new Event("cancel", { cancelable: true });
    fireEvent(dialog, nativeCancel);
    expect(nativeCancel.defaultPrevented).toBe(true);
    expect(dialog.hasAttribute("open")).toBe(true);
    expect(invoke.mock.calls.filter(([method]) => method === "decide_quit")).toHaveLength(1);
    await act(async () => acknowledge());
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(HTMLDialogElement.prototype.close).toHaveBeenCalledTimes(1);
    expect(document.activeElement).toBe(origin);
    await act(async () => listen.mock.calls[0][1]());
    await screen.findByRole("dialog");
    expect(screen.queryByRole("group", { name: "복구본 영구 삭제 확인" })).toBeNull();
  } finally {
    origin.remove();
  }
});

it("keeps the modal on a rejected native cancellation and permits an explicit retry", async () => {
  invoke.mockImplementation((method: string) =>
    method === "pending_quit" ? Promise.resolve("quit-1") : Promise.resolve(null),
  );
  unregister = registerNoteEditor({
    unsaved: () => true,
    saveBeforeQuit: async () => true,
    settleBeforeQuit: async () => {},
  });
  render(<QuitGuard />);
  const dialog = await screen.findByRole("dialog");
  invoke.mockImplementationOnce(() => Promise.reject(new Error("native unavailable")));
  fireEvent(dialog, new Event("cancel", { cancelable: true }));
  await screen.findByRole("alert");
  expect(dialog.hasAttribute("open")).toBe(true);
  fireEvent(dialog, new Event("cancel", { cancelable: true }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(invoke.mock.calls.filter(([method]) => method === "decide_quit")).toHaveLength(2);
});
