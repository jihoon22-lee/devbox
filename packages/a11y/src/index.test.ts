import { describe, expect, it, vi } from "vitest";
import {
  focusFirst,
  focusableElements,
  isImeComposing,
  isKeyboardActivation,
  restoreFocus,
  trapDialogKeyDown,
} from "./index";

describe("keyboard helpers", () => {
  it("recognizes browser and React IME composition signals", () => {
    expect(isImeComposing({ key: "Enter", isComposing: true })).toBe(true);
    expect(isImeComposing({ key: "Enter", keyCode: 229 })).toBe(true);
    expect(isImeComposing({ key: "Enter", nativeEvent: { isComposing: true } })).toBe(true);
    expect(isImeComposing({ key: "Enter" })).toBe(false);
  });

  it("accepts Enter and Space only outside composition", () => {
    expect(isKeyboardActivation({ key: "Enter" })).toBe(true);
    expect(isKeyboardActivation({ key: " " })).toBe(true);
    expect(isKeyboardActivation({ key: "Enter", nativeEvent: { keyCode: 229 } })).toBe(false);
    expect(isKeyboardActivation({ key: "ArrowDown" })).toBe(false);
  });
});

describe("dialog focus helpers", () => {
  it("filters hidden and disabled controls and focuses the first eligible control", () => {
    document.body.innerHTML = `
      <section id="dialog">
        <button disabled>disabled</button>
        <fieldset disabled><button id="fieldset-disabled">fieldset disabled</button></fieldset>
        <div hidden><button>hidden</button></div>
        <div inert><button id="inert">inert</button></div>
        <input id="first" />
        <button id="last">last</button>
      </section>`;
    const dialog = document.querySelector("#dialog")!;
    expect(focusableElements(dialog).map((node) => node.id)).toEqual(["first", "last"]);
    expect(focusFirst(dialog)?.id).toBe("first");
    expect(document.activeElement?.id).toBe("first");
  });

  it("wraps Tab inside the dialog and handles Escape", () => {
    document.body.innerHTML = `
      <section id="dialog"><input id="first" /><button id="last">last</button></section>`;
    const dialog = document.querySelector("#dialog")!;
    const first = document.querySelector<HTMLElement>("#first")!;
    const last = document.querySelector<HTMLElement>("#last")!;
    const preventDefault = vi.fn();
    const stopPropagation = vi.fn();
    last.focus();
    expect(trapDialogKeyDown({ key: "Tab", shiftKey: false, preventDefault, stopPropagation }, dialog)).toBe(true);
    expect(document.activeElement).toBe(first);
    expect(stopPropagation).toHaveBeenCalledOnce();

    const onEscape = vi.fn();
    expect(trapDialogKeyDown({ key: "Escape", shiftKey: false, preventDefault, stopPropagation }, dialog, onEscape)).toBe(true);
    expect(onEscape).toHaveBeenCalledOnce();
  });

  it("restores focus only to a connected element", () => {
    const button = document.createElement("button");
    expect(restoreFocus(button)).toBe(false);
    document.body.append(button);
    expect(restoreFocus(button)).toBe(true);
    expect(document.activeElement).toBe(button);
  });
});
