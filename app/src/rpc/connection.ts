import { create } from "zustand";
import type { ClientState } from "./client";

interface ConnectionStore {
  state: ClientState;
  daemonVersion?: string;
  lastReadyMs?: number;
  set(state: ClientState, daemonVersion?: string): void;
  reset(): void;
}
export const useConnection = create<ConnectionStore>((set) => ({
  state: "connecting",
  set: (state, daemonVersion) =>
    set((previous) => ({
      state,
      daemonVersion: daemonVersion ?? previous.daemonVersion,
      lastReadyMs: state === "ready" ? Date.now() : previous.lastReadyMs,
    })),
  reset: () => set({ state: "connecting", daemonVersion: undefined, lastReadyMs: undefined }),
}));
