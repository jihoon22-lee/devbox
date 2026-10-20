import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";
import type { LogSource } from "../../rpc/gen/rpc";
import { sourceKey } from "./model";

interface PinnedSources {
  byProject: Record<string, LogSource[]>;
  set(projectId: string, sources: LogSource[]): void;
  clear(projectId: string): void;
}
function readSources(value: unknown): LogSource[] {
  if (!Array.isArray(value)) return [];
  const sources = new Map<string, LogSource>();
  for (const item of value) {
    if (!item || typeof item !== "object") continue;
    const id =
      item.kind === "run" ? item.runId : item.kind === "file" ? item.path : item.kind === "container" ? item.id : null;
    if (typeof id !== "string" || !id.trim()) continue;
    const source: LogSource =
      item.kind === "run"
        ? { kind: "run", runId: id }
        : item.kind === "file"
          ? { kind: "file", path: id }
          : { kind: "container", id };
    sources.set(sourceKey(source), source);
    if (sources.size === 8) break;
  }
  return [...sources.values()];
}
const storage = createJSONStorage<{ byProject: Record<string, LogSource[]> }>(() => ({
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
      /* Pins still work in memory. */
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
export function createPinnedLogSourcesStore() {
  return create<PinnedSources>()(
    persist(
      (set) => ({
        byProject: {},
        set: (projectId, sources) =>
          set((state) => {
            const next = readSources(sources);
            const byProject = { ...state.byProject };
            if (next.length)
              Object.defineProperty(byProject, projectId, {
                value: next,
                enumerable: true,
                configurable: true,
                writable: true,
              });
            else delete byProject[projectId];
            return { byProject };
          }),
        clear: (projectId) =>
          set((state) => {
            const byProject = { ...state.byProject };
            delete byProject[projectId];
            return { byProject };
          }),
      }),
      {
        name: "devbox.pinnedLogSources",
        storage,
        partialize: ({ byProject }) => ({ byProject }),
        merge: (saved, current) => {
          const byProject: Record<string, LogSource[]> = {};
          if (
            saved &&
            typeof saved === "object" &&
            "byProject" in saved &&
            saved.byProject &&
            typeof saved.byProject === "object"
          ) {
            for (const [project, value] of Object.entries(saved.byProject)) {
              const sources = readSources(value);
              if (sources.length)
                Object.defineProperty(byProject, project, {
                  value: sources,
                  enumerable: true,
                  configurable: true,
                  writable: true,
                });
            }
          }
          return { ...current, byProject };
        },
      },
    ),
  );
}
export const usePinnedLogSources = createPinnedLogSourcesStore();
