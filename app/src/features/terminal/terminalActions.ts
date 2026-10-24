import type { Terminal } from "@xterm/xterm";
import { useLayout } from "../../shell/layout";
import { setTerminalFont } from "../settings/preferences";
import { terminalKey } from "./keys";

const DEFAULT_SIZE = 13;
interface TerminalActions {
  isDisposed: () => boolean;
  preview: boolean;
  warn: (title: string) => void;
  fitView: () => void;
  search: () => void;
}

/** Install the view's key actions while reading its current renderer lifetime for async clipboard work. */
export function attachTerminalActions(
  term: Terminal,
  { isDisposed, preview, warn, fitView, search }: TerminalActions,
): void {
  term.attachCustomKeyEventHandler((event) => {
    if (isDisposed()) return false;
    if (event.type !== "keydown" || event.defaultPrevented) return true;
    const action = terminalKey(event, term.hasSelection());
    if (!action) return true;
    event.preventDefault();
    event.stopPropagation();
    if (action === "copy") {
      void Promise.resolve()
        .then(() => {
          if (!isDisposed()) return navigator.clipboard.writeText(term.getSelection());
        })
        .catch(() => warn("클립보드에 복사하지 못했습니다"));
    }
    if (action === "paste" && !preview) {
      void Promise.resolve()
        .then(() => {
          if (!isDisposed()) return navigator.clipboard.readText();
        })
        .then((text) => {
          if (!isDisposed() && text !== undefined) term.paste(text);
        })
        .catch(() => warn("클립보드에서 붙여넣지 못했습니다"));
    }
    if (action === "zoomIn" || action === "zoomOut" || action === "zoomReset") {
      const size =
        action === "zoomReset"
          ? DEFAULT_SIZE
          : Math.min(28, Math.max(9, (term.options.fontSize ?? DEFAULT_SIZE) + (action === "zoomIn" ? 1 : -1)));
      term.options.fontSize = size;
      setTerminalFont(size, false);
      fitView();
    }
    if (action === "search") search();
    if (action === "bottom") useLayout.getState().toggleBottom();
    return false;
  });
}
