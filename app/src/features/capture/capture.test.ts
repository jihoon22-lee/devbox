import { expect, test, vi } from "vitest";
import { captureKey, submitCapture } from "./capture";

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
