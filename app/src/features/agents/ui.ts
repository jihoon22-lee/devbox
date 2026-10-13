import { create } from "zustand";
export const useAgentsUi = create<{ creating: boolean; setCreating: (creating: boolean) => void }>((set) => ({
  creating: false,
  setCreating: (creating) => set({ creating }),
}));
