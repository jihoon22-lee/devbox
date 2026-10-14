import { afterEach, expect, test, vi } from "vitest";
import { openPopout, popoutLabel } from "./popout";

const native = vi.hoisted(() => ({
  get: vi.fn(),
  focus: vi.fn(),
  created: vi.fn(),
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  stops: vi.fn(),
}));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  WebviewWindow: class {
    static getByLabel = native.get;
    setFocus = native.focus;
    constructor(label: string, options: unknown) {
      native.created(label, options);
    }
    once(event: string, callback: (message: { payload: unknown }) => void) {
      native.listeners.set(event, callback);
      return Promise.resolve(native.stops);
    }
  },
}));
afterEach(() => {
  delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  native.get.mockReset();
  native.focus.mockReset();
  native.created.mockReset();
  native.listeners.clear();
  native.stops.mockReset();
  history.replaceState({}, "", "/");
});
test("browser popouts reuse/focus the same target, preserve validated search, and report blockers", async () => {
  history.replaceState({}, "", "/terminal?gateway=ws%3A%2F%2Ffixture&renderer=dom&other=ignored");
  const popup = { closed: false, focus: vi.fn() };
  const open = vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
  await openPopout("terminal", "browser/id");
  await openPopout("terminal", "browser/id");
  expect(open).toHaveBeenCalledTimes(1);
  expect(open.mock.calls[0]?.[0]).toBe("/popout/terminal/browser%2Fid?gateway=ws%3A%2F%2Ffixture&renderer=dom");
  expect(popup.focus).toHaveBeenCalledTimes(2);
  expect(popoutLabel("terminal", "a/b")).not.toBe(popoutLabel("terminal", "a-b"));
  popup.closed = true;
  await openPopout("terminal", "browser/id");
  expect(open).toHaveBeenCalledTimes(2);
  open.mockReturnValue(null);
  await expect(openPopout("agent", "blocked")).rejects.toThrow("blocked");
});
test("native creation deduplicates pending requests, waits for creation, then reuses its window", async () => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  native.get.mockResolvedValue(null);
  native.focus.mockResolvedValue(undefined);
  const first = openPopout("terminal", "native");
  const second = openPopout("terminal", "native");
  expect(second).toBe(first);
  await vi.waitFor(() => expect(native.listeners.has("tauri://created")).toBe(true));
  expect(native.created).toHaveBeenCalledTimes(1);
  native.listeners.get("tauri://created")!({ payload: null });
  await first;
  expect(native.focus).toHaveBeenCalledTimes(1);
  expect(native.stops).toHaveBeenCalledTimes(2);
  native.get.mockResolvedValue({ setFocus: native.focus });
  await openPopout("terminal", "native");
  expect(native.created).toHaveBeenCalledTimes(1);
  expect(native.focus).toHaveBeenCalledTimes(2);
});
test("native creation errors reject and clear the pending operation for retry", async () => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  native.get.mockResolvedValue(null);
  const first = openPopout("terminal", "native-failed");
  const rejected = expect(first).rejects.toThrow("native failure");
  await vi.waitFor(() => expect(native.listeners.has("tauri://error")).toBe(true));
  native.listeners.get("tauri://error")!({ payload: "native failure" });
  await rejected;
  native.get.mockResolvedValue({ setFocus: native.focus });
  await expect(openPopout("terminal", "native-failed")).resolves.toBeUndefined();
});
