import { afterEach, expect, test, vi } from "vitest";
import { captureKey, submitCapture } from "./capture";

const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
/** A newly loaded window: native has heard nothing from it yet, as after a reload. */
async function freshWindow(tauri = true) {
  if (tauri) Object.assign(window, { __TAURI_INTERNALS__: {} });
  vi.resetModules();
  return import("./capture");
}
afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  native.invoke.mockReset();
  sessionStorage.clear();
});

test("IME composition and Shift+Enter do not submit; Esc keeps the draft", () => {
  expect(captureKey({ key: "Enter", shiftKey: false, isComposing: true, keyCode: 13 })).toBe("none");
  expect(captureKey({ key: "Enter", shiftKey: false, isComposing: false, keyCode: 229 })).toBe("none");
  expect(captureKey({ key: "Enter", shiftKey: true, isComposing: false, keyCode: 13 })).toBe("none");
  expect(captureKey({ key: "Escape", shiftKey: false, isComposing: false, keyCode: 27 })).toBe("hide");
});

test("Enter sends one inbox capture and hides only after success", async () => {
  let finish!: () => void;
  const append = vi.fn(
    () =>
      new Promise<void>((r) => {
        finish = r;
      }),
  );
  const hide = vi.fn(async () => {});
  const saved: string[] = [];
  const pending = submitCapture("owned draft", append, hide, (text) => saved.push(text));
  expect(append).toHaveBeenCalledWith({ target: "inbox", format: "capture", text: "owned draft" });
  expect(hide).not.toHaveBeenCalled();
  finish();
  await pending;
  expect(append).toHaveBeenCalledTimes(1);
  expect(saved).toEqual([""]);
  expect(hide).toHaveBeenCalledTimes(1);
});

test("failed append preserves draft and never hides the window", async () => {
  const hide = vi.fn();
  const clear = vi.fn();
  await expect(
    submitCapture(
      "keep",
      async () => {
        throw Error("failed");
      },
      hide,
      clear,
    ),
  ).rejects.toThrow("failed");
  expect(hide).not.toHaveBeenCalled();
  expect(clear).not.toHaveBeenCalled();
});

test("native hears whether a draft exists only when that changes, and blank text is no draft", async () => {
  native.invoke.mockResolvedValue(undefined);
  const { reportCaptureDraft, saveCaptureDraft } = await freshWindow();
  reportCaptureDraft("kept across a reload");
  for (const text of ["kept across a reload!", " \n ", "", "a", "ab", "\t"]) saveCaptureDraft(text);
  expect(native.invoke.mock.calls).toEqual([
    ["capture_draft", { hasDraft: true }],
    ["capture_draft", { hasDraft: false }],
    ["capture_draft", { hasDraft: true }],
    ["capture_draft", { hasDraft: false }],
  ]);
  expect(sessionStorage.getItem("devbox.capture.draft")).toBe("\t");
});

test("a draft the session cannot keep still counts, and a failed report is ignored", async () => {
  native.invoke.mockRejectedValue(new Error("owned fixture failure"));
  const { saveCaptureDraft } = await freshWindow();
  const failed = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("quota");
  });
  try {
    expect(saveCaptureDraft("unsaved")).toBe(false);
  } finally {
    failed.mockRestore();
  }
  expect(native.invoke.mock.calls).toEqual([["capture_draft", { hasDraft: true }]]);
  // The rejected report settles here; an unhandled rejection would fail the run.
  await Promise.resolve();
});

test("outside the desktop app the draft stays in the session and nothing is reported", async () => {
  const { saveCaptureDraft } = await freshWindow(false);
  expect(saveCaptureDraft("browser draft")).toBe(true);
  expect(sessionStorage.getItem("devbox.capture.draft")).toBe("browser draft");
  expect(native.invoke).not.toHaveBeenCalled();
});
