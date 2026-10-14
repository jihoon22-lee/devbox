import { createJSONStorage } from "zustand/middleware";
export const safeStorage = createJSONStorage(() => ({
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
      /* Memory draft remains available. */
    }
  },
  removeItem: (name: string) => {
    try {
      localStorage.removeItem(name);
    } catch {
      /* Storage may be unavailable. */
    }
  },
}));
export function stringMap(value: unknown): Record<string, string> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  return Object.fromEntries(Object.entries(value).filter(([, item]) => typeof item === "string"));
}
