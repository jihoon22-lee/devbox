import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect } from "react";

/** SC2 acknowledgements; native logs elapsed time on its own monotonic clock and never reads renderer time. */
const nativeMain = () => "__TAURI_INTERNALS__" in window && getCurrentWindow().label === "main";
/** Runs after the committed frame has been presented; cancellation keeps StrictMode re-runs to one call. */
export function afterPaint(run: () => void): () => void {
  let next = 0;
  const first = requestAnimationFrame(() => {
    next = requestAnimationFrame(run);
  });
  return () => {
    cancelAnimationFrame(first);
    cancelAnimationFrame(next);
  };
}
export function reportPalettePaint(traceId: number, outcome: "painted" | "alreadyOpen", deferred: boolean) {
  if (nativeMain()) void invoke("palette_painted", { traceId, outcome, deferred }).catch(() => {});
}
let firstDataReported = false;
/** Once per renderer process, after the first data view or its empty state is visible. */
export function useFirstDataPaint(painted: boolean) {
  useEffect(() => {
    if (!painted || firstDataReported || !nativeMain()) return;
    return afterPaint(() => {
      if (firstDataReported) return;
      firstDataReported = true;
      void invoke("first_data_painted").catch(() => {});
    });
  }, [painted]);
}
