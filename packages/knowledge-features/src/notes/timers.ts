export interface Timers {
  set(callback: () => void, ms: number): number;
  clear(handle: number): void;
}
export const browserTimers: Timers = {
  set: (callback, ms) => window.setTimeout(callback, ms),
  clear: (handle) => window.clearTimeout(handle),
};
