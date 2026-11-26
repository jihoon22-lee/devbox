import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState, useSyncExternalStore } from "react";

// The one answer to "can the user see this window": the document's own state and what the host
// says about the window. A window closed to the tray or minimized may keep
// `document.visibilityState` "visible" (inferred from the tray traffic of #678, not confirmed on
// Windows), so the host reports it. With no host answer (browser, an older shell) the document
// decides alone.
let hostShown = true;
/** Host events so far: an answer to a question asked before a newer event is dropped. */
let events = 0;
/** The latest question; only its answer counts. */
let asking = 0;
let disconnect: (() => void) | undefined;
const listeners = new Set<() => void>();

function receive(shown: boolean) {
  if (hostShown === shown) return;
  hostShown = shown;
  for (const listener of [...listeners]) listener();
}
function connect() {
  let label: string;
  try {
    if (!("__TAURI_INTERNALS__" in window)) return () => {};
    label = getCurrentWindow().label;
  } catch {
    return () => {};
  }
  let live = true;
  let off: (() => void) | undefined;
  const ask = async () => {
    const seen = events;
    const question = ++asking;
    try {
      const shown = await invoke<unknown>("window_shown");
      if (live && seen === events && question === asking && typeof shown === "boolean") receive(shown);
    } catch {
      // An older shell has no such command: the document keeps deciding alone.
    }
  };
  void (async () => {
    try {
      // Events reach every window's listener, so each carries the label it is about.
      const unlisten = await listen<{ label?: unknown; shown?: unknown }>("devbox://window-shown", ({ payload }) => {
        if (!live || payload?.label !== label || typeof payload.shown !== "boolean") return;
        events += 1;
        receive(payload.shown);
      });
      if (live) off = unlisten;
      else unlisten?.();
    } catch {
      // No event channel: the question below and each focus still ask.
    }
    // Asked only after listening, so a change in between still arrives as an event. A window
    // that starts in the tray gets no event at all and learns its state from this answer.
    if (live) void ask();
  })();
  // A focused window is on screen; asking again corrects a report the host missed.
  const focused = () => void ask();
  window.addEventListener("focus", focused);
  return () => {
    live = false;
    off?.();
    window.removeEventListener("focus", focused);
    hostShown = true;
  };
}
const subscribe = (changed: () => void) => {
  if (listeners.size === 0) disconnect = connect();
  listeners.add(changed);
  document.addEventListener("visibilitychange", changed);
  return () => {
    document.removeEventListener("visibilitychange", changed);
    listeners.delete(changed);
    if (listeners.size === 0) {
      disconnect?.();
      disconnect = undefined;
    }
  };
};
const unwatched = () => () => {};
const always = () => true;
export const pageVisible = () => document.visibilityState === "visible" && hostShown;
/** `watch: false` neither subscribes nor renders again: for a caller that does not depend on it now. */
export function usePageVisible(watch = true) {
  return useSyncExternalStore(
    watch ? subscribe : unwatched,
    watch ? pageVisible : always,
    watch ? () => false : always,
  );
}
/** Visible time updates age labels and thresholds even when list revisions do not change. */
export function useVisibleClock() {
  const visible = usePageVisible();
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (!visible) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [visible]);
  return now;
}
