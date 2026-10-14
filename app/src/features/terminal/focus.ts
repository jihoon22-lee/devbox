import { create } from "zustand";
export const useTerminalFocus = create<{
  byProject: Record<string, string>;
  cwd: Record<string, string>;
  bells: Record<string, boolean>;
  focus: (project: string, id: string) => void;
  setCwd: (id: string, path: string) => void;
  ring: (id: string) => void;
}>((set) => ({
  byProject: {},
  cwd: {},
  bells: {},
  focus: (project, id) =>
    set((state) => ({ byProject: { ...state.byProject, [project]: id }, bells: { ...state.bells, [id]: false } })),
  setCwd: (id, path) => set((state) => ({ cwd: { ...state.cwd, [id]: path } })),
  ring: (id) => set((state) => ({ bells: { ...state.bells, [id]: true } })),
}));
