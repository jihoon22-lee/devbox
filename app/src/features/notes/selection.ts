import { create } from "zustand";
export const useNoteSelection = create<{ path?: string; select(path?: string): void }>((set) => ({
  path: undefined,
  select: (path) => set({ path }),
}));
