import { terminalKey } from "./keys";

const key = (init: KeyboardEventInit) => new KeyboardEvent("keydown", init);
test("terminal shortcuts preserve shell Ctrl+C and IME/AltGr", () => {
  for (const [code, shiftKey, selection, result] of [
    ["KeyC", true, false, "copy"],
    ["KeyC", false, true, "copy"],
    ["KeyC", false, false, null],
    ["KeyV", true, false, "paste"],
    ["Equal", false, false, "zoomIn"],
    ["Equal", true, false, "zoomIn"],
    ["Minus", false, false, "zoomOut"],
    ["Digit0", false, false, "zoomReset"],
    ["KeyF", true, false, "search"],
    ["Backquote", false, false, "bottom"],
    ["KeyL", false, false, null],
  ] as const)
    expect(terminalKey(key({ code, ctrlKey: true, shiftKey }), selection)).toBe(result);
  expect(terminalKey(key({ code: "KeyC", ctrlKey: true, isComposing: true }), true)).toBeNull();
  expect(terminalKey(key({ code: "KeyV", ctrlKey: true, shiftKey: true, altKey: true }), false)).toBeNull();
  expect(terminalKey(key({ code: "KeyC", ctrlKey: true, metaKey: true }), true)).toBeNull();
});
