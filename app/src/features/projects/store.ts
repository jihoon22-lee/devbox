import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";
interface CurrentProject {
  id?: string;
  set(id: string | undefined): void;
}
const storage = createJSONStorage<{ id?: string }>(() => ({
  getItem: (key) => {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  },
  setItem: (key, value) => {
    try {
      localStorage.setItem(key, value);
    } catch {
      /* Selection still works in memory. */
    }
  },
  removeItem: (key) => {
    try {
      localStorage.removeItem(key);
    } catch {
      /* Storage may be blocked. */
    }
  },
}));
export function createCurrentProjectStore() {
  return create<CurrentProject>()(
    persist(
      (set) => ({
        id: undefined,
        set: (id) => set({ id }),
      }),
      {
        name: "devbox.currentProject",
        storage,
        partialize: ({ id }) => ({ id }),
        merge: (saved, current) => ({
          ...current,
          id:
            saved && typeof saved === "object" && "id" in saved && typeof saved.id === "string" && saved.id.trim()
              ? saved.id
              : undefined,
        }),
      },
    ),
  );
}
export const useCurrentProject = createCurrentProjectStore();
