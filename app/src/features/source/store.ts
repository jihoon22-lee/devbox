import { create } from "zustand";
import type { DocKey } from "../../rpc/gen/rpc";
import { movedKey } from "../documents/effects";
import type { CloseChoice } from "../documents/machine";
import { keyId } from "../documents/offlineJournal";
import { needsCloseChoice, useDocuments } from "../documents/store";
import { type CodeLocation, validCodeKey } from "./editor/location";
import { emptyTabs, reduceTabs, type TabEvent, type Tabs } from "./editor/tabs";

interface SourceState {
  layout: Tabs;
  recent: DocKey[];
  locations: Record<string, CodeLocation & { request: number }>;
  quickOpen: boolean;
  send(event: TabEvent): void;
  open(key: DocKey, options?: { pinned?: boolean; location?: CodeLocation }): Promise<void>;
  close(key: DocKey, choice?: CloseChoice): Promise<boolean>;
}
function recentFiles(): DocKey[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem("devbox.source.recent") ?? "[]");
    return Array.isArray(value) ? value.filter(validCodeKey).slice(0, 100) : [];
  } catch {
    return [];
  }
}
let sequence = 0;
let opening = Promise.resolve();
export const useSource = create<SourceState>((set, get) => ({
  layout: emptyTabs(),
  recent: recentFiles(),
  locations: {},
  quickOpen: false,
  send(event) {
    const layout = reduceTabs(get().layout, event);
    if (event.type !== "rename") {
      set({ layout });
      return;
    }
    const recent = get().recent.map((key) => movedKey(key, event.from, event.to) ?? key);
    const locations = Object.fromEntries(
      Object.entries(get().locations).map(([id, value]) => {
        const [root, path] = JSON.parse(id) as [string, string];
        const key = movedKey({ root, path }, event.from, event.to);
        return [key ? keyId(key) : id, value];
      }),
    );
    set({ layout, recent, locations });
    try {
      localStorage.setItem("devbox.source.recent", JSON.stringify(recent));
    } catch {
      /* Memory state remains current. */
    }
  },
  open(key, options = {}) {
    const run = async () => {
      if (!validCodeKey(key)) throw new Error("파일 경로가 올바르지 않습니다.");
      if (useDocuments.getState().windowClosing) return;
      const current = get().layout;
      const exists = current.tabs.some((t) => keyId(t.key) === keyId(key));
      if (!exists) {
        const preview = current.tabs.find((t) => t.group === current.group && !t.pinned);
        if (preview) {
          const doc = useDocuments.getState().find(preview.key);
          if (doc && needsCloseChoice(doc)) get().send({ type: "pin", key: preview.key });
          else if (!(await useDocuments.getState().closeSafely(preview.key)))
            get().send({ type: "pin", key: preview.key });
        }
      }
      if (useDocuments.getState().windowClosing) return;
      useDocuments.getState().open(key, { kind: "code" });
      get().send({ type: "open", key, pinned: options.pinned });
      const recent = [key, ...get().recent.filter((known) => keyId(known) !== keyId(key))].slice(0, 100);
      const location = options.location;
      set({
        recent,
        ...(location ? { locations: { ...get().locations, [keyId(key)]: { ...location, request: ++sequence } } } : {}),
      });
      try {
        localStorage.setItem("devbox.source.recent", JSON.stringify(recent));
      } catch {
        /* Still kept in memory. */
      }
    };
    const next = opening.then(run, run);
    opening = next.catch(() => {});
    return next;
  },
  async close(key, choice) {
    if (!(await useDocuments.getState().closeSafely(key, choice))) return false;
    get().send({ type: "closed", key });
    return true;
  },
}));
/** Mounted once per window alongside the existing NotesSession; never reconnect the document store twice. */
export function syncDocumentTabs() {
  return useDocuments.subscribe((now, before) => {
    for (const doc of Object.values(now.docs)) {
      const old = before.docs[doc.id];
      if (old && keyId(old.key) !== keyId(doc.key))
        useSource.getState().send({ type: "rename", from: old.key, to: doc.key });
      if (needsCloseChoice(doc)) useSource.getState().send({ type: "pin", key: doc.key });
    }
    if (!now.windowClosing)
      for (const doc of Object.values(before.docs))
        if (!now.docs[doc.id]) useSource.getState().send({ type: "closed", key: doc.key });
  });
}
