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
  fireEvent.keyDown(dialog, { key: "Escape", isComposing: true });
  expect(invoke.mock.calls.some(([method]) => method === "decide_quit")).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "버리고 종료" }));
  expect(invoke.mock.calls.some(([method]) => method === "decide_quit")).toBe(false);
  await act(async () => {
    finish();
    await pending;
  });
  expect(invoke).toHaveBeenCalledWith("decide_quit", { id: "quit-1", quit: true });
});
