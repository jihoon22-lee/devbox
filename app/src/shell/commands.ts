import { create } from "zustand";
export interface Command {
  id: string;
  label: string;
  keys?: string[];
  run: () => void;
}
interface Commands {
  items: Map<string, Command>;
  register: (command: Command) => () => void;
  list: () => Command[];
}
export const useCommands = create<Commands>((set, get) => ({
  items: new Map(),
  list: () => [...get().items.values()],
  register: (command) => {
    const owned = { ...command };
    set({ items: new Map(get().items).set(command.id, owned) });
    return () => {
      if (get().items.get(command.id) !== owned) return;
      const items = new Map(get().items);
      items.delete(command.id);
      set({ items });
    };
  },
}));
