import type { Terminal } from "@xterm/xterm";

/** The first tmux redraw must not erase history left in a resized viewport. */
export function preserveInitialHistory(term: Terminal) {
  let armed = true;
  let disposed = false;
  const disable = () => {
    if (!disposed && term.options.scrollOnEraseInDisplay) term.options.scrollOnEraseInDisplay = false;
  };
  const parser = term.parser.registerCsiHandler({ final: "J" }, (params) => {
    const initialErase = armed && params[0] === 2;
    if (params[0] === 2) armed = false;
    // Returning false continues xterm's built-in erase, which uses its current
    // viewport size. A second erase in the same write must clear normally.
    term.options.scrollOnEraseInDisplay = initialErase;
    return false;
  });
  return {
    written: disable,
    reset() {
      disable();
      armed = true;
    },
    dispose() {
      disable();
      disposed = true;
      parser.dispose();
    },
  };
}
