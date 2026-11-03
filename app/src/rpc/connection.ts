import { create } from "zustand";
import type { ClientState } from "./client";

interface ConnectionStore {
  state: ClientState;
  daemonVersion?: string;
  lastReadyMs?: number;
  /** `RpcClient.failedAttempts`: attempts without a Welcome since the last one. */
  failedAttempts: number;
  set(state: ClientState, daemonVersion?: string): void;
  setFailedAttempts(failedAttempts: number): void;
  reset(): void;
}
export const useConnection = create<ConnectionStore>((set) => ({
  state: "connecting",
  failedAttempts: 0,
  set: (state, daemonVersion) =>
    set((previous) => ({
      state,
      daemonVersion: daemonVersion ?? previous.daemonVersion,
      lastReadyMs: state === "ready" ? Date.now() : previous.lastReadyMs,
    })),
  setFailedAttempts: (failedAttempts) => set({ failedAttempts }),
  reset: () => set({ state: "connecting", daemonVersion: undefined, lastReadyMs: undefined, failedAttempts: 0 }),
}));
