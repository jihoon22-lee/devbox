import { afterEach, expect, test, vi } from "vitest";
import { openPopout, popoutLabel } from "./popout";

const native = vi.hoisted(() => ({ invoke: vi.fn(), created: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  WebviewWindow: class {
    constructor() {
      native.created();
      throw new Error("Renderer creation is forbidden");
    }
    static getByLabel() {
      throw new Error("Renderer lookup is forbidden");
    }
  },
}));
afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  native.invoke.mockReset();
  native.created.mockReset();
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
test("native creation delegates typed identity and pending deduplication without URL or renderer creation", async () => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  history.replaceState({}, "", "/terminal?gateway=wss://private.invalid&renderer=dom&url=https://wrong.invalid");
  let finish!: (label: string) => void;
  native.invoke.mockImplementationOnce(
    () =>
      new Promise<string>((resolve) => {
        finish = resolve;
      }),
  );
  const first = openPopout("terminal", "native/id");
  const second = openPopout("terminal", "native/id");
  expect(second).toBe(first);
  await vi.waitFor(() =>
    expect(native.invoke).toHaveBeenCalledWith("popout_create", { kind: "terminal", id: "native/id" }),
  );
  expect(native.invoke).toHaveBeenCalledOnce();
  finish(popoutLabel("terminal", "native/id"));
  await first;
  native.invoke.mockResolvedValue(popoutLabel("terminal", "native/id"));
  await openPopout("terminal", "native/id");
  expect(native.invoke).toHaveBeenCalledTimes(2);
  expect(native.created).not.toHaveBeenCalled();
});

test.each(["agents-grid", "capture"] as const)(
  "native %s omits arbitrary URLs and sends an explicit null identity",
  async (kind) => {
    Object.assign(window, { __TAURI_INTERNALS__: {} });
    native.invoke.mockResolvedValue(popoutLabel(kind));
    await openPopout(kind);
    expect(native.invoke).toHaveBeenCalledWith("popout_create", { kind, id: null });
    expect(native.created).not.toHaveBeenCalled();
  },
);

test("native creation rejection clears the pending request and never exposes raw payload details", async () => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  native.invoke.mockRejectedValueOnce({ url: "https://private.invalid", body: "secret body" });
  await expect(openPopout("agent", "retry")).rejects.toThrow(
    "새 창을 열지 못했습니다. 앱 상태를 확인하고 다시 시도하세요.",
  );
  native.invoke.mockResolvedValue(popoutLabel("agent", "retry"));
  await expect(openPopout("agent", "retry")).resolves.toBeUndefined();
  expect(native.invoke).toHaveBeenCalledTimes(2);
});

test("native labels preserve nested Unicode identity and malformed success responses are rejected", async () => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  const id = "work/2026 계획.md";
  native.invoke.mockResolvedValueOnce(popoutLabel("note", id));
  await openPopout("note", id);
  expect(native.invoke).toHaveBeenCalledWith("popout_create", { kind: "note", id });
  native.invoke.mockResolvedValueOnce("https://wrong.invalid");
  await expect(openPopout("note", id)).rejects.toThrow("새 창을 열지 못했습니다");
});
