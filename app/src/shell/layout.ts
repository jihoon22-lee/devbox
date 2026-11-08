import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

interface Layout {
  listWidth: number;
  /** The user's choice, saved across restarts. */
  listCollapsed: boolean;
  /** A narrow frame hid the list. Never saved, so widening the frame shows the list again. */
  listAutoCollapsed: boolean;
  bottomOpen: boolean;
  bottomHeight: number;
  toggleList(): void;
  toggleBottom(): void;
  setListWidth(px: number): void;
  setBottomHeight(px: number): void;
}
export function clampSize(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
/** Whether the list is hidden, by the user or by a narrow frame. */
export function isListCollapsed(layout: { listCollapsed: boolean; listAutoCollapsed: boolean }): boolean {
  return layout.listCollapsed || layout.listAutoCollapsed;
}
const storage = createJSONStorage(() => ({
  getItem: (name: string) => {
    try {
      return localStorage.getItem(name);
    } catch {
      return null;
    }
  },
  setItem: (name: string, value: string) => {
    try {
      localStorage.setItem(name, value);
    } catch {
      /* Layout still works without storage. */
    }
  },
  removeItem: (name: string) => {
    try {
      localStorage.removeItem(name);
    } catch {
      /* Storage is optional. */
    }
  },
}));
export const useLayout = create<Layout>()(
  persist(
    (set) => ({
      listWidth: 260,
      listCollapsed: false,
      listAutoCollapsed: false,
      bottomOpen: false,
      bottomHeight: 260,
      // A hidden list always opens, even one the user collapsed before the frame narrowed.
      toggleList: () =>
        set((s) => (isListCollapsed(s) ? { listCollapsed: false, listAutoCollapsed: false } : { listCollapsed: true })),
      toggleBottom: () => set((s) => ({ bottomOpen: !s.bottomOpen })),
      setListWidth: (px) => {
        if (Number.isFinite(px)) set({ listWidth: clampSize(px, 180, 480) });
      },
      setBottomHeight: (px) => {
        if (Number.isFinite(px)) set({ bottomHeight: clampSize(px, 120, 600) });
      },
    }),
    {
      name: "devbox.layout",
      storage,
      partialize: ({ listWidth, listCollapsed, bottomOpen, bottomHeight }) => ({
        listWidth,
        listCollapsed,
        bottomOpen,
        bottomHeight,
      }),
      merge: (persisted, current) => {
        if (!persisted || typeof persisted !== "object") return current;
        const saved = persisted as Record<string, unknown>;
        return {
          ...current,
          ...(typeof saved.listWidth === "number" && Number.isFinite(saved.listWidth)
            ? { listWidth: clampSize(saved.listWidth, 180, 480) }
            : {}),
          ...(typeof saved.bottomHeight === "number" && Number.isFinite(saved.bottomHeight)
            ? { bottomHeight: clampSize(saved.bottomHeight, 120, 600) }
            : {}),
          ...(typeof saved.listCollapsed === "boolean" ? { listCollapsed: saved.listCollapsed } : {}),
          ...(typeof saved.bottomOpen === "boolean" ? { bottomOpen: saved.bottomOpen } : {}),
        };
      },
    },
  ),
);
export function autoCollapse(width: number, height: number) {
  return { list: width < 1100, bottom: height < 760 };
}
