import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useMemo, useRef } from "react";
import { useRpcSession } from "../../rpc/RpcProvider";
import { useCommands } from "../../shell/commands";
import { type Binding, useKeymap } from "../../shell/keymap";
import { createNote, openDaily } from "./actions";
import { NotesSession } from "./session";

/** The same window test as the shell's `ownsAttention`: popouts leave app shortcuts to the main window. */
function mainWindow(): boolean {
  if ("__TAURI_INTERNALS__" in window) return getCurrentWindow().label === "main";
  return !window.name.startsWith("popout-") && !window.location.pathname.startsWith("/popout/");
}

/**
 * Notes shortcuts and palette commands, plus this window's document session (`NotesSession`).
 * Ctrl+Alt+D opens today's daily note from anywhere in the main window; Ctrl+N makes a note in
 * the notes section, outside the editor (the keymap does not pass Ctrl+letter into editors).
 * Ctrl+S is the editor's own CodeMirror keymap.
 */
export function NotesCommands({ navigate }: { navigate: () => void }) {
  const { client } = useRpcSession();
  const latest = useRef({ client, navigate });
  latest.current = { client, navigate };
  const busy = useRef(false);
  const register = useCommands((state) => state.register);
  const actions = useMemo(() => {
    const once = (run: () => Promise<unknown>) => async () => {
      if (busy.current) return;
      busy.current = true;
      try {
        await run();
      } finally {
        busy.current = false;
      }
    };
    return {
      daily: once(async () => {
        if (await openDaily(latest.current.client)) latest.current.navigate();
      }),
      create: once(() => createNote(latest.current.client)),
    };
  }, []);
  const bindings = useMemo<Binding[]>(
    () => [
      {
        id: "notes.daily",
        code: "KeyD",
        ctrl: true,
        alt: true,
        scope: "app",
        label: "일일 기록 열기",
        enabled: mainWindow,
        run: () => void actions.daily(),
      },
      {
        id: "notes.new",
        code: "KeyN",
        ctrl: true,
        scope: "section",
        label: "새 노트",
        enabled: () => mainWindow() && window.location.pathname === "/notes",
        run: () => void actions.create(),
      },
    ],
    [actions],
  );
  useKeymap(bindings);
  useEffect(() => {
    if (!mainWindow()) return;
    const cleanups = [
      register({
        id: "notes.daily",
        label: "일일 기록 열기",
        keys: ["Ctrl", "Alt", "D"],
        run: () => void actions.daily(),
      }),
      register({
        id: "notes.new",
        label: "새 노트",
        keys: ["Ctrl", "N"],
        run: () =>
          void actions.create().then(() => {
            if (window.location.pathname !== "/notes") latest.current.navigate();
          }),
      }),
    ];
    return () => cleanups.forEach((cleanup) => cleanup());
  }, [register, actions]);
  return <NotesSession />;
}
