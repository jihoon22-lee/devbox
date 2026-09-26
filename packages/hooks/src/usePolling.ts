import { useCallback, useEffect, useRef } from "react";

export interface PollingOptions {
  intervalMs: number;
  active?: boolean;
  pauseWhenHidden?: boolean;
  immediate?: boolean;
}

/** One request at a time, including across visibility and effect generations. */
export function usePolling(
  callback: () => Promise<void> | void,
  { intervalMs, active = true, pauseWhenHidden = true, immediate = true }: PollingOptions,
): { refresh: () => void } {
  const latest = useRef(callback);
  latest.current = callback;
  const flight = useRef({ running: false, requested: false });
  const cycle = useRef<{ refresh(): void; settled(): void } | null>(null);

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const hidden = () => pauseWhenHidden && typeof document !== "undefined" && document.hidden;
    const clearTimer = () => {
      clearTimeout(timer);
      timer = undefined;
    };
    const schedule = () => {
      clearTimer();
      if (!cancelled && !hidden()) timer = setTimeout(() => void run(), intervalMs);
    };
    const run = async () => {
      clearTimer();
      if (cancelled || hidden()) return;
      if (flight.current.running) {
        flight.current.requested = true;
        return;
      }
      flight.current.running = true;
      flight.current.requested = false;
      try {
        await latest.current();
      } catch {
        // The feature owns its error state. A failed read must not stop polling.
      } finally {
        flight.current.running = false;
        cycle.current?.settled();
      }
    };
    const current = {
      refresh: () => {
        void run();
      },
      settled: () => {
        if (flight.current.requested) void run();
        else schedule();
      },
    };
    cycle.current = current;
    const onVisibility = () => {
      if (hidden()) clearTimer();
      else current.refresh();
    };
    if (typeof document !== "undefined") document.addEventListener("visibilitychange", onVisibility);
    if (immediate) current.refresh();
    else schedule();
    return () => {
      cancelled = true;
      clearTimer();
      if (cycle.current === current) cycle.current = null;
      flight.current.requested = false;
      if (typeof document !== "undefined") document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [active, intervalMs, pauseWhenHidden, immediate]);

  return { refresh: useCallback(() => cycle.current?.refresh(), []) };
}
