import { create } from "zustand";
import { persist } from "zustand/middleware";
import { safeStorage, stringMap } from "./storage";

const key = (id: string, path: string) => JSON.stringify([id, path]);
export const useSeen = create<{
  entries: Record<string, string>;
  mark: (agentId: string, path: string, sig: string) => void;
  isSeen: (agentId: string, path: string, sig: string) => boolean;
}>()(
  persist(
    (set, get) => ({
      entries: {},
      mark: (id, path, sig) => set({ entries: { ...get().entries, [key(id, path)]: sig } }),
      isSeen: (id, path, sig) => get().entries[key(id, path)] === sig,
    }),
    {
      name: "devbox.agentSeen",
      storage: safeStorage,
      partialize: (state) => ({ entries: state.entries }),
      merge: (saved, current) => ({
        ...current,
        entries: Object.fromEntries(
          Object.entries(stringMap((saved as { entries?: unknown } | null)?.entries)).filter(([entry, sig]) => {
            try {
              const pair: unknown = JSON.parse(entry);
              return (
                Array.isArray(pair) &&
                pair.length === 2 &&
                pair.every((value) => typeof value === "string") &&
                /^sha256:[a-f0-9]{64}$/.test(sig)
              );
            } catch {
              return false;
            }
          }),
        ),
      }),
    },
  ),
);
