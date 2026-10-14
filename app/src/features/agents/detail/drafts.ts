import { create } from "zustand";
import { persist } from "zustand/middleware";
import { safeStorage, stringMap } from "./storage";
export const useDrafts = create<{
  text: Record<string, string>;
  revisions: Record<string, number>;
  set: (id: string, text: string) => void;
  append: (id: string, text: string) => void;
}>()(
  persist(
    (set, get) => ({
      text: {},
      revisions: {},
      set: (id, text) =>
        set({
          text: { ...get().text, [id]: text },
          revisions: { ...get().revisions, [id]: (get().revisions[id] ?? 0) + 1 },
        }),
      append: (id, text) => {
        const previous = get().text[id] ?? "";
        get().set(id, `${previous}${previous && !previous.endsWith("\n") ? "\n" : ""}${text}`);
      },
    }),
    {
      name: "devbox.agentDrafts",
      storage: safeStorage,
      partialize: (state) => ({ text: state.text }),
      merge: (saved, current) => ({ ...current, text: stringMap((saved as { text?: unknown } | null)?.text) }),
    },
  ),
);
