import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import { useDocuments } from "../features/documents/store";
import { createQuitPreparation } from "./quitPreparation";

/** Also mounted before ConnectionGate: every note-bearing WebView must explicitly ACK. */
export function NativeQuitPreparation() {
  const [error, setError] = useState(false);
  useEffect(() => {
    const label = getCurrentWindow().label;
    if (label !== "main" && !label.startsWith("popout-note-")) return;
    let disposed = false;
    let lease: number | undefined;
    let currentRound: number | undefined;
    const offs: (() => void)[] = [];
    const preparation = createQuitPreparation(
      {
        prepareWindowClose: () => useDocuments.getState().prepareWindowClose(),
        cancelWindowClose: () => useDocuments.getState().cancelWindowClose(),
      },
      async (round, safe) => {
        if (lease === undefined) throw new Error("quit registration missing");
        await invoke("quit_prepared", { round, lease, safe });
      },
      () => {
        if (!disposed) setError(true);
      },
    );
    void (async () => {
      try {
        for (const [name, receive] of [
          [
            "devbox://prepare-quit",
            (round: number) => {
              currentRound = round;
              setError(false);
              void preparation.prepare(round);
            },
          ],
          ["devbox://cancel-quit", (round: number) => preparation.cancel(round)],
        ] as const) {
          const off = await listen<number>(name, ({ payload }) => {
            if (!disposed) receive(payload);
          });
          if (disposed) {
            off();
            return;
          }
          offs.push(off);
        }
        lease = await invoke<number>("quit_window_ready");
        if (disposed) await invoke("quit_window_not_ready", { lease });
      } catch {
        if (!disposed) setError(true);
      }
    })();
    return () => {
      disposed = true;
      offs.forEach((off) => off());
      if (currentRound !== undefined) preparation.cancel(currentRound);
      if (lease !== undefined) void invoke("quit_window_not_ready", { lease }).catch(() => {});
    };
  }, []);
  return error ? (
    <p role="alert">노트를 안전하게 저장하지 못해 종료를 취소했습니다. 연결과 저장 상태를 확인하고 다시 시도하세요.</p>
  ) : null;
}
