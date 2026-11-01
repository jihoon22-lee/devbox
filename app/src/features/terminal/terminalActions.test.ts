import type { Terminal } from "@xterm/xterm";
import { attachTerminalActions } from "./terminalActions";

/** The key handler `attachTerminalActions` installs, on a terminal that records what it sends. */
function install(extendedKeys: () => boolean, preview = false) {
  let handler: (event: KeyboardEvent) => boolean = () => true;
  const term = {
    options: { fontSize: 13 },
    input: vi.fn(),
    paste: vi.fn(),
    hasSelection: () => false,
    getSelection: () => "",
    attachCustomKeyEventHandler: (cb: (event: KeyboardEvent) => boolean) => {
      handler = cb;
    },
  };
  attachTerminalActions(term as unknown as Terminal, {
    isDisposed: () => false,
    preview,
    extendedKeys,
    warn: vi.fn(),
    fitView: vi.fn(),
    search: vi.fn(),
  });
  const press = (init: KeyboardEventInit) => {
    const event = new KeyboardEvent("keydown", { key: "Enter", code: "Enter", cancelable: true, ...init });
    const stop = vi.spyOn(event, "stopPropagation");
    return { handled: !handler(event), prevented: event.defaultPrevented, stopped: stop.mock.calls.length > 0 };
  };
  return { term, press };
}
const taken = { handled: true, prevented: true, stopped: true };
const untouched = { handled: false, prevented: false, stopped: false };

test("Shift+Enter is sent as CSI u only while the attached tmux carries extended keys", () => {
  let extended = true;
  const { term, press } = install(() => extended);
  expect(press({ shiftKey: true })).toEqual(taken);
  expect(term.input).toHaveBeenCalledExactlyOnceWith("\x1b[13;2u", true);
  // tmux 3.2–3.4 would drop CSI u for a program that did not ask for it, so xterm.js keeps its Enter.
  extended = false;
  expect(press({ shiftKey: true })).toEqual(untouched);
  expect(term.input).toHaveBeenCalledOnce();
  // Read on every key: a reattach to another tmux needs no new handler.
  extended = true;
  expect(press({ shiftKey: true })).toEqual(taken);
  expect(term.input).toHaveBeenCalledTimes(2);
});

test("Enter, modified Enter and Shift+Enter during IME composition stay xterm's either way", () => {
  for (const extended of [true, false]) {
    const { term, press } = install(() => extended);
    for (const init of [
      {},
      { ctrlKey: true },
      { altKey: true },
      { shiftKey: true, isComposing: true },
      { shiftKey: true, keyCode: 229 },
    ])
      expect(press(init), JSON.stringify({ extended, init })).toEqual(untouched);
    expect(term.input).not.toHaveBeenCalled();
  }
});

test("a preview never takes Shift+Enter", () => {
  const { term, press } = install(() => true, true);
  expect(press({ shiftKey: true })).toEqual(untouched);
  expect(term.input).not.toHaveBeenCalled();
});
