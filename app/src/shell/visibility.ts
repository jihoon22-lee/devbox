import { useEffect, useState, useSyncExternalStore } from "react";

const subscribe = (changed: () => void) => {
  document.addEventListener("visibilitychange", changed);
  return () => document.removeEventListener("visibilitychange", changed);
};
export function usePageVisible() {
  return useSyncExternalStore(
    subscribe,
    () => document.visibilityState === "visible",
    () => false,
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
