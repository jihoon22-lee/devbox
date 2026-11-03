export type TerminalKey = "copy" | "paste" | "zoomIn" | "zoomOut" | "zoomReset" | "search" | "bottom";
export function terminalKey(e: KeyboardEvent, hasSelection: boolean): TerminalKey | null {
  if (e.isComposing || e.keyCode === 229 || !e.ctrlKey || e.altKey || e.metaKey) return null;
  if (e.code === "Equal") return "zoomIn";
  if (e.shiftKey) {
    if (e.code === "KeyC") return "copy";
    if (e.code === "KeyV") return "paste";
    if (e.code === "KeyF") return "search";
    return null;
  }
  if (e.code === "KeyC" && hasSelection) return "copy";
  if (e.code === "Minus") return "zoomOut";
  if (e.code === "Digit0") return "zoomReset";
  if (e.code === "Backquote") return "bottom";
  return null;
}
/** CSI u for Enter (13) with Shift (2): the newline key of programs that asked tmux for extended keys. */
export const SHIFT_ENTER = "\x1b[13;2u";
/** Shift+Enter alone, outside IME composition (the guard `terminalKey` applies too). */
export function isShiftEnter(e: KeyboardEvent): boolean {
  if (e.isComposing || e.keyCode === 229) return false;
  return e.key === "Enter" && e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey;
}
