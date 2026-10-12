import { fireEvent, render } from "@testing-library/react";
import { allowedWhileFocused, type Binding, matches, useKeymap } from "./keymap";
const binding: Binding = {
  id: "palette",
  code: "KeyP",
  ctrl: true,
  shift: true,
  scope: "app",
  label: "명령",
  run: vi.fn(),
};
test("physical keys reject composition, 229 and extra meta", () => {
  expect(
    matches(new KeyboardEvent("keydown", { code: "KeyP", key: "ㅔ", ctrlKey: true, shiftKey: true }), binding),
  ).toBe(true);
  for (const extra of [{ isComposing: true }, { keyCode: 229 }, { metaKey: true }])
    expect(
      matches(new KeyboardEvent("keydown", { code: "KeyP", ctrlKey: true, shiftKey: true, ...extra }), binding),
    ).toBe(false);
});
test("terminal and editor preserve editing chords", () => {
  for (const scope of ["terminal", "editor"] as const) {
    expect(allowedWhileFocused(binding, scope)).toBe(true);
    expect(allowedWhileFocused({ ...binding, code: "Space" }, scope)).toBe(false);
    expect(allowedWhileFocused({ ...binding, shift: false }, scope)).toBe(false);
    expect(allowedWhileFocused({ ...binding, code: "Digit2", shift: false }, scope)).toBe(true);
    expect(allowedWhileFocused({ ...binding, code: "F1", ctrl: false, shift: false }, scope)).toBe(true);
    expect(allowedWhileFocused({ ...binding, code: "ArrowLeft", ctrl: false, shift: false, alt: true }, scope)).toBe(
      false,
    );
  }
});
test("modal editing and navigation stay inside dialog", () => {
  const run = vi.fn();
  function Test() {
    useKeymap([{ ...binding, run }]);
    return (
      <div role="dialog">
        <input aria-label="검색" />
      </div>
    );
  }
  const view = render(<Test />);
  const input = view.getByRole("textbox");
  input.focus();
  fireEvent.keyDown(input, { code: "KeyP", ctrlKey: true, shiftKey: true });
  expect(run).not.toHaveBeenCalled();
});
test("capture owns allowed terminal chords even if the terminal stops propagation", () => {
  const run = vi.fn(),
    terminal = vi.fn();
  function Test() {
    useKeymap([{ ...binding, run }]);
    return (
      <div data-keyscope="terminal">
        <input
          aria-label="터미널"
          onKeyDown={(event) => {
            event.stopPropagation();
            terminal();
          }}
        />
      </div>
    );
  }
  const view = render(<Test />);
  const input = view.getByRole("textbox");
  input.focus();
  fireEvent.keyDown(input, { code: "KeyP", ctrlKey: true, shiftKey: true });
  expect(run).toHaveBeenCalledOnce();
  expect(terminal).not.toHaveBeenCalled();
  fireEvent.keyDown(input, { code: "KeyP", ctrlKey: true });
  expect(run).toHaveBeenCalledOnce();
  expect(terminal).toHaveBeenCalledOnce();
});
