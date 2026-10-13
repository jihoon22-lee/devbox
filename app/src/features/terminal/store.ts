import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

export type Pane =
  | { kind: "leaf"; session: string }
  | {
      kind: "split";
      dir: "row" | "column";
      ratio: number;
      a: Pane;
      b: Pane;
    };
export interface Tab {
  id: string;
  root: Pane;
}
export interface Layout {
  tabs: Tab[];
  active?: string;
}
export const emptyLayout = (): Layout => ({ tabs: [] });
export const EMPTY_LAYOUT: Layout = emptyLayout();
export const projectKey = (id?: string | null) => (id ? `project:${id}` : "none");
const clamp = (ratio: number) => (Number.isFinite(ratio) ? Math.max(0.15, Math.min(0.85, ratio)) : 0.5);
export function sessionsIn(pane: Pane): string[] {
  return pane.kind === "leaf" ? [pane.session] : [...sessionsIn(pane.a), ...sessionsIn(pane.b)];
}
export function openTab(layout: Layout, session: string): Layout {
  const existing = layout.tabs.find((tab) => sessionsIn(tab.root).includes(session));
  if (existing) return layout.active === existing.id ? layout : { ...layout, active: existing.id };
  const tab: Tab = { id: crypto.randomUUID(), root: { kind: "leaf", session } };
  return { tabs: [...layout.tabs, tab], active: tab.id };
}
function mapPane(pane: Pane, leaf: (pane: Extract<Pane, { kind: "leaf" }>) => Pane | null): Pane | null {
  if (pane.kind === "leaf") return leaf(pane);
  const a = mapPane(pane.a, leaf),
    b = mapPane(pane.b, leaf);
  return a && b ? (a === pane.a && b === pane.b ? pane : { ...pane, a, b }) : (a ?? b);
}
export function splitActive(layout: Layout, target: string, session: string, dir: "row" | "column"): Layout {
  if (layout.tabs.some((tab) => sessionsIn(tab.root).includes(session))) return openTab(layout, session);
  const tab = layout.tabs.find((tab) => sessionsIn(tab.root).includes(target));
  if (!tab) return openTab(layout, session);
  return {
    active: tab.id,
    tabs: layout.tabs.map((item) =>
      item !== tab
        ? item
        : {
            ...item,
            root: mapPane(item.root, (leaf) =>
              leaf.session === target
                ? { kind: "split", dir, ratio: 0.5, a: leaf, b: { kind: "leaf", session } }
                : leaf,
            )!,
          },
    ),
  };
}
export function closePane(layout: Layout, session: string): Layout {
  const tabs = layout.tabs.flatMap((tab) => {
    const root = mapPane(tab.root, (leaf) => (leaf.session === session ? null : leaf));
    return root ? [root === tab.root ? tab : { ...tab, root }] : [];
  });
  if (tabs.length === layout.tabs.length && tabs.every((tab, index) => tab === layout.tabs[index])) return layout;
  return { tabs, active: tabs.some((tab) => tab.id === layout.active) ? layout.active : tabs.at(-1)?.id };
}
export function prune(layout: Layout, alive: Set<string>): Layout {
  return layout.tabs
    .flatMap((tab) => sessionsIn(tab.root))
    .filter((id) => !alive.has(id))
    .reduce(closePane, layout);
}
export function setRatio(layout: Layout, tabId: string, path: ("a" | "b")[], ratio: number): Layout {
  const walk = (pane: Pane, rest: ("a" | "b")[]): Pane => {
    if (pane.kind === "leaf") return pane;
    if (!rest.length) return { ...pane, ratio: clamp(ratio) };
    const [head, ...tail] = rest;
    return head === "a" ? { ...pane, a: walk(pane.a, tail) } : { ...pane, b: walk(pane.b, tail) };
  };
  return {
    ...layout,
    tabs: layout.tabs.map((tab) => (tab.id === tabId ? { ...tab, root: walk(tab.root, path) } : tab)),
  };
}
export function titleOf(tab: Tab, sessions: { id: string; title: string }[] = []): string {
  const ids = sessionsIn(tab.root);
  const first = sessions.find((session) => session.id === ids[0])?.title ?? "터미널";
  return ids.length > 1 ? `${first} +${ids.length - 1}` : first;
}
const record = (value: unknown): value is Record<string, unknown> =>
  !!value && typeof value === "object" && !Array.isArray(value);
const validId = (value: unknown): value is string => typeof value === "string" && !!value.trim() && value.length <= 256;
export function sanitizeProjects(value: unknown): Record<string, Layout> {
  if (!record(value)) return {};
  const output: Record<string, Layout> = {};
  for (const [key, saved] of Object.entries(value).slice(0, 200)) {
    if (!validId(key) || !record(saved) || !Array.isArray(saved.tabs)) continue;
    const sessions = new Set<string>(),
      ids = new Set<string>();
    const parse = (input: unknown, seen: Set<string>, depth = 0): Pane | null => {
      if (depth > 16 || !record(input) || seen.size >= 128) return null;
      if (input.kind === "leaf" && validId(input.session) && !seen.has(input.session)) {
        seen.add(input.session);
        return { kind: "leaf", session: input.session };
      }
      if (input.kind !== "split" || (input.dir !== "row" && input.dir !== "column") || typeof input.ratio !== "number")
        return null;
      const a = parse(input.a, seen, depth + 1),
        b = parse(input.b, seen, depth + 1);
      return a && b ? { kind: "split", dir: input.dir, ratio: clamp(input.ratio), a, b } : null;
    };
    const tabs = saved.tabs.slice(0, 64).flatMap((input) => {
      if (!record(input) || !validId(input.id) || ids.has(input.id)) return [];
      const seen = new Set(sessions),
        root = parse(input.root, seen);
      if (!root) return [];
      seen.forEach((id) => sessions.add(id));
      ids.add(input.id);
      return [{ id: input.id, root }];
    });
    Object.defineProperty(output, key, {
      enumerable: true,
      configurable: true,
      writable: true,
      value: { tabs, active: validId(saved.active) && ids.has(saved.active) ? saved.active : tabs[0]?.id },
    });
  }
  return output;
}
interface Store {
  byProject: Record<string, Layout>;
  update(project: string, change: (layout: Layout) => Layout): void;
}
export const useTerminalLayout = create<Store>()(
  persist(
    (set, get) => ({
      byProject: {},
      update: (project, change) => {
        const current = get().byProject[project] ?? EMPTY_LAYOUT;
        const next = change(current);
        if (next !== current) set({ byProject: { ...get().byProject, [project]: next } });
      },
    }),
    {
      name: "devbox.terminalLayout",
      storage: createJSONStorage(() => ({
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
            /* In-memory layouts remain available. */
          }
        },
        removeItem: (key) => {
          try {
            localStorage.removeItem(key);
          } catch {
            /* Optional storage. */
          }
        },
      })),
      partialize: ({ byProject }) => ({ byProject }),
      merge: (saved, current) => ({
        ...current,
        byProject: sanitizeProjects(record(saved) ? saved.byProject : undefined),
      }),
    },
  ),
);
