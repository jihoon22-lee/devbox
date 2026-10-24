import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { NativeQuitPreparation } from "./NativeQuitPreparation";

const mocks = vi.hoisted(() => ({
  label: "main",
  invoke: vi.fn(),
  listen: vi.fn(),
  prepare: vi.fn(),
  cancel: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ label: mocks.label }) }));
vi.mock("../features/documents/store", () => ({
  useDocuments: {
    getState: () => ({
      prepareWindowClose: mocks.prepare,
      cancelWindowClose: mocks.cancel,
    }),
  },
}));
const listeners = new Map<string, (event: { payload: number }) => void>();
beforeEach(() => {
  vi.clearAllMocks();
  listeners.clear();
  mocks.label = "main";
  mocks.invoke.mockImplementation(async (command) => (command === "quit_window_ready" ? 42 : undefined));
  mocks.listen.mockImplementation(async (name, cb) => {
    listeners.set(name, cb);
    return () => listeners.delete(name);
  });
  mocks.prepare.mockResolvedValue(true);
});
afterEach(cleanup);

test.each(["main", "popout-note-owned"])(
  "%s listens before registering and ACKs only after persistence",
  async (label) => {
    mocks.label = label;
    let finish!: (value: boolean) => void;
    mocks.prepare.mockImplementation(
      () =>
        new Promise<boolean>((resolve) => {
          finish = resolve;
        }),
    );
    render(<NativeQuitPreparation />);
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("quit_window_ready"));
    expect(listeners.size).toBe(2);
    act(() => listeners.get("devbox://prepare-quit")?.({ payload: 7 }));
    expect(mocks.prepare).toHaveBeenCalledTimes(1);
    expect(mocks.invoke).not.toHaveBeenCalledWith("quit_prepared", expect.anything());
    await act(async () => finish(true));
    expect(mocks.invoke).toHaveBeenCalledWith("quit_prepared", { round: 7, lease: 42, safe: true });
    act(() => listeners.get("devbox://cancel-quit")?.({ payload: 6 }));
    expect(mocks.cancel).not.toHaveBeenCalled();
    act(() => listeners.get("devbox://cancel-quit")?.({ payload: 7 }));
    expect(mocks.cancel).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("alert")).toHaveTextContent("종료를 취소");
  },
);

test("terminal popouts do not claim document readiness", async () => {
  mocks.label = "popout-terminal-owned";
  render(<NativeQuitPreparation />);
  await act(async () => {});
  expect(mocks.listen).not.toHaveBeenCalled();
  expect(mocks.invoke).not.toHaveBeenCalled();
});
