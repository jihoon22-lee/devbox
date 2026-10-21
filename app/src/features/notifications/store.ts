import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

export interface NoticeTarget {
  to: "/agents" | "/terminal" | "/runs" | "/source" | "/notes" | "/tools" | "/settings";
  search?: Record<string, string>;
}
export interface Notice {
  key: string;
  id?: string;
  kind: "agent" | "run" | "error";
  title: string;
  body: string;
  atMs: number;
  read: boolean;
  target?: NoticeTarget;
}
export type NoticeInput = Omit<Notice, "atMs" | "read">;
const paths = new Set(["/agents", "/terminal", "/runs", "/source", "/notes", "/tools", "/settings"]);
export function safeTarget(value: unknown): NoticeTarget | undefined {
  if (!value || typeof value !== "object") return;
  const { to, search } = value as Record<string, unknown>;
  if (typeof to !== "string" || !paths.has(to)) return;
  const cleaned: Record<string, string> = {};
  if (search && typeof search === "object" && !Array.isArray(search)) {
    const candidate = search as Record<string, unknown>;
    if (typeof candidate.id === "string" && candidate.id.trim()) cleaned.id = candidate.id;
    if (to === "/runs") {
      for (const key of ["projectId", "runId", "task"])
        if (typeof candidate[key] === "string" && candidate[key].trim()) cleaned[key] = candidate[key];
    }
    if (to === "/agents" && (candidate.view === "list" || candidate.view === "grid")) cleaned.view = candidate.view;
  }
  return { to: to as NoticeTarget["to"], ...(Object.keys(cleaned).length ? { search: cleaned } : {}) };
}
function notices(value: unknown): Notice[] {
  if (!Array.isArray(value)) return [];
  const keys = new Set<string>();
  const items: Notice[] = [];
  for (const item of value) {
    if (
      !item ||
      typeof item !== "object" ||
      keys.has(item.key) ||
      typeof item.key !== "string" ||
      !item.key.trim() ||
      (item.kind !== "agent" && item.kind !== "run" && item.kind !== "error") ||
      typeof item.title !== "string" ||
      typeof item.body !== "string" ||
      typeof item.atMs !== "number" ||
      !Number.isFinite(item.atMs) ||
      item.atMs < 0 ||
      item.atMs > 8_640_000_000_000_000 ||
      typeof item.read !== "boolean"
    )
      continue;
    keys.add(item.key);
    items.push({
      key: item.key,
      kind: item.kind,
      title: item.title,
      body: item.body,
      atMs: item.atMs,
      read: item.read,
      ...(typeof item.id === "string" ? { id: item.id } : {}),
      target: safeTarget(item.target),
    });
    if (items.length === 100) break;
  }
  return items;
}
const storageKey = "devbox.notifications";
let unsaved = false;
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
      unsaved = false;
    } catch {
      unsaved = true; /* A readable old value must not overwrite newer memory. */
    }
  },
  removeItem: (name: string) => {
    try {
      localStorage.removeItem(name);
    } catch {
      /* Optional persistence. */
    }
  },
}));
function latestItems(fallback: Notice[]) {
  if (unsaved) return fallback;
  try {
    const raw = localStorage.getItem(storageKey);
    if (raw === null) return fallback;
    const saved: unknown = JSON.parse(raw);
    return notices((saved as { state?: { items?: unknown } } | null)?.state?.items);
  } catch {
    return fallback;
  }
}
const unread = (items: Notice[]) => items.filter((item) => !item.read).length;
interface Store {
  items: Notice[];
  unread: number;
  push: (notice: NoticeInput) => void;
  pushOnce: (notice: NoticeInput) => boolean;
  markRead: (key: string) => void;
  markAllRead: () => void;
  clear: () => void;
}
export const useNotifications = create<Store>()(
  persist(
    (set, get) => ({
      items: [],
      unread: 0,
      push: (notice) => {
        const items = notices([
          { ...notice, atMs: Date.now(), read: false },
          ...latestItems(get().items).filter((item) => item.key !== notice.key),
        ]);
        set({ items, unread: unread(items) });
      },
      pushOnce: (notice) => {
        const previous = latestItems(get().items);
        if (previous.some((item) => item.key === notice.key)) return false;
        const items = notices([{ ...notice, atMs: Date.now(), read: false }, ...previous]);
        set({ items, unread: unread(items) });
        return true;
      },
      markRead: (key) => {
        const items = latestItems(get().items).map((item) => (item.key === key ? { ...item, read: true } : item));
        set({ items, unread: unread(items) });
      },
      markAllRead: () => set({ items: latestItems(get().items).map((item) => ({ ...item, read: true })), unread: 0 }),
      clear: () => set({ items: [], unread: 0 }),
    }),
    {
      name: storageKey,
      storage,
      partialize: (state) => ({ items: state.items }),
      merge: (saved, current) => {
        const items = notices((saved as { items?: unknown } | null)?.items);
        return { ...current, items, unread: unread(items) };
      },
    },
  ),
);

// A popout may retain errors too. Read the latest persisted list before mutations,
// then refresh other windows on storage events so they do not overwrite stale history.
window.addEventListener("storage", (event) => {
  if (unsaved || (event.key !== storageKey && event.key !== null)) return;
  if (event.storageArea && event.storageArea !== localStorage) return;
  const current = useNotifications.getState();
  const items = latestItems([]);
  if (JSON.stringify(items) !== JSON.stringify(current.items))
    useNotifications.setState({ items, unread: unread(items) });
});
