import { create } from "zustand";
import { reportPalettePaint } from "./paintTrace";
/** `traceId` is set only by the native global shortcut; `deferred` marks a wait behind another dialog. */
export type NativeIntent =
  | { kind: "palette"; traceId?: number; deferred?: boolean }
  | { kind: "bottom-terminal"; wasFocused: boolean };
/** This main-host state survives gate/transport remounts; unknown is never a zero snapshot. */
export const useNativeRuntime = create<{
  openTasks: number | null;
  snapshotCurrent: boolean;
  intents: NativeIntent[];
  bottomFocus: number;
  push: (intent: NativeIntent) => void;
}>((set) => ({
  openTasks: null,
  snapshotCurrent: false,
  intents: [],
  bottomFocus: 0,
  push: (intent) => set((state) => ({ intents: [...state.intents, intent].slice(-16) })),
}));
export function consumeNativeIntents(run: (intent: NativeIntent) => void) {
  let processing = false;
  let next: ReturnType<typeof setTimeout> | undefined;
  const flush = () => {
    if (processing) return;
    // A palette request is already fulfilled while our own palette is open.
    // Other dialogs, including ones with the same title, still defer new requests.
    if (document.querySelector('#devbox-command-palette[role="dialog"][data-state="open"]')) {
      const intents = useNativeRuntime.getState().intents;
      const remaining = intents.filter((intent) => intent.kind !== "palette");
      if (remaining.length !== intents.length) {
        processing = true;
        useNativeRuntime.setState({ intents: remaining });
        processing = false;
        for (const intent of intents)
          if (intent.kind === "palette" && intent.traceId !== undefined)
            reportPalettePaint(intent.traceId, "alreadyOpen", intent.deferred === true);
      }
    }
    if (document.querySelector('[role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"]')) {
      const intents = useNativeRuntime.getState().intents;
      if (intents.some((intent) => intent.kind === "palette" && intent.traceId !== undefined && !intent.deferred)) {
        processing = true;
        useNativeRuntime.setState({
          intents: intents.map((intent) =>
            intent.kind === "palette" && intent.traceId !== undefined ? { ...intent, deferred: true } : intent,
          ),
        });
        processing = false;
      }
      return;
    }
    const [intent, ...remaining] = useNativeRuntime.getState().intents;
    if (!intent) return;
    processing = true;
    useNativeRuntime.setState({ intents: remaining });
    run(intent);
    processing = false;
    if (remaining.length) next = setTimeout(flush, 0);
  };
  const stop = useNativeRuntime.subscribe(flush);
  const observer = new MutationObserver(flush);
  observer.observe(document.body, { childList: true, subtree: true });
  flush();
  return () => {
    stop();
    observer.disconnect();
    clearTimeout(next);
  };
}
