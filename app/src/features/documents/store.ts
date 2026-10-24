import { create } from "zustand";
import type { RpcClient } from "../../rpc/client";
import type { DocKey, DocPolicy } from "../../rpc/gen/rpc";
import { browserTimers, createEffectRunner, type EffectRunner, movedKey, type Timers } from "./effects";
import { type CloseChoice, closedDoc, type Doc, type DocEvent, reduce } from "./machine";
import { indexedDbJournal, keyId, type OfflineJournal } from "./offlineJournal";
import { windowId } from "./windowId";

export type DocumentsClient = Pick<RpcClient, "state" | "call" | "subscribe" | "onState">;
export interface DocumentsDeps {
  journal?: OfflineJournal;
  windowId?: string;
  timers?: Timers;
  /** Lease renewal period; the daemon expires a lease after 60 s without use. */
  renewMs?: number;
}
export interface DocumentsStore {
  docs: Record<string, Doc>;
  /** Binds the store to one daemon session. Returns the disconnect function, which forgets every document. */
  connect(client: DocumentsClient, deps?: DocumentsDeps): () => void;
  open(key: DocKey, policy: DocPolicy): string;
  find(key: DocKey): Doc | undefined;
  dispatch(id: string, event: DocEvent): void;
  edit(key: DocKey, text: string): void;
  /** An edit that did not come from the editor (a daemon rewrite): the buffer and the editor both change. */
  replace(key: DocKey, text: string): void;
  save(key: DocKey): void;
  close(key: DocKey, choice?: CloseChoice): void;
  resolve(key: DocKey, choice: "mine" | "disk" | "merged", text?: string): void;
  restore(key: DocKey): void;
  discard(key: DocKey): void;
  take(key: DocKey): void;
  /** An editor that owns per-document `EditorState`s receives undoable text replacements here. */
  registerEditor(apply: (id: string, text: string) => void): () => void;
  /** Resolves once no effect is running (tests and shutdown). */
  settled(): Promise<void>;
}

const findIn = (docs: Record<string, Doc>, key: DocKey) => {
  const id = keyId(key);
  return Object.values(docs).find((doc) => keyId(doc.key) === id);
};

export const useDocuments = create<DocumentsStore>((set, get) => {
  let runner: EffectRunner | undefined;
  let nextId = 1;
  const editors = new Set<(id: string, text: string) => void>();

  const dispatch = (id: string, event: DocEvent) => {
    const doc = get().docs[id];
    if (!doc) return;
    const step = reduce(doc, event);
    if (step.doc.state === "Closed") {
      const { [id]: _closed, ...rest } = get().docs;
      set({ docs: rest });
      runner?.cancel(id);
    } else if (step.doc !== doc) set({ docs: { ...get().docs, [id]: step.doc } });
    runner?.run(id, step.effects);
  };
  const byKey = (key: DocKey, event: DocEvent) => {
    const doc = findIn(get().docs, key);
    if (doc) dispatch(doc.id, event);
  };
  const all = (event: DocEvent) => {
    for (const id of Object.keys(get().docs)) dispatch(id, event);
  };

  return {
    docs: {},
    connect(client, deps = {}) {
      const journal = deps.journal ?? indexedDbJournal();
      const own = createEffectRunner({
        client,
        journal,
        windowId: deps.windowId ?? windowId(),
        timers: deps.timers ?? browserTimers,
        dispatch,
        applyText: (id, text) => {
          for (const apply of editors) apply(id, text);
        },
      });
      runner = own;
      const offs = [
        client.subscribe(
          "documents.changed",
          (_rev, p) => byKey(p.key, { type: "external", rev: p.rev }),
          () => all({ type: "resync" }),
        ),
        // A folder rename is announced once with the folder key: every open document below it moves.
        client.subscribe(
          "documents.moved",
          (_rev, p) => {
            for (const doc of Object.values(get().docs)) {
              const to = movedKey(doc.key, p.from, p.to);
              if (to) dispatch(doc.id, { type: "moved", to });
            }
            own.track(own.moveOffline(p.from, p.to));
          },
          () => {},
        ),
        // The topic carries no reason: reclaim with `claim`, which answers read-only when another window took it.
        client.subscribe(
          "documents.lease_lost",
          (_rev, p) => {
            const doc = Object.values(get().docs).find((d) => d.leaseId === p.leaseId);
            if (doc) dispatch(doc.id, { type: "leaseLost", reason: "expired" });
          },
          () => {},
        ),
      ];
      let ready = client.state === "ready";
      // Documents opened before the session existed claim now.
      const early = Object.keys(get().docs);
      if (ready)
        own.track(
          own.flushTombstones().then(() => {
            for (const id of early) dispatch(id, { type: "reconnected" });
          }),
        );
      offs.push(
        client.onState((state) => {
          if (state !== "ready") {
            ready = false;
            return;
          }
          if (ready) return;
          ready = true;
          // Pending daemon drops go first so a dropped draft is never saved again (AR-F13).
          own.track(own.flushTombstones().then(() => all({ type: "reconnected" })));
        }),
      );
      const renew = setInterval(() => {
        const leased = Object.values(get().docs).filter((d) => d.leaseId);
        if (!leased.length || client.state !== "ready") return;
        const leaseIds = leased.map((d) => d.leaseId as string);
        own.track(
          client.call("documents.renew", { leaseIds }).then(
            ({ lost }) => {
              for (const doc of leased)
                if (lost.includes(doc.leaseId as string)) dispatch(doc.id, { type: "leaseLost", reason: "expired" });
            },
            () => {},
          ),
        );
      }, deps.renewMs ?? 20_000);
      return () => {
        for (const off of offs) off();
        clearInterval(renew);
        own.cancelAll();
        if (runner === own) {
          runner = undefined;
          set({ docs: {} });
        }
      };
    },
    open(key, policy) {
      const existing = findIn(get().docs, key);
      if (existing) return existing.id;
      const id = `doc-${nextId++}`;
      set({ docs: { ...get().docs, [id]: closedDoc(id, key, policy) } });
      dispatch(id, { type: "open", key, policy });
      return id;
    },
    find: (key) => findIn(get().docs, key),
    dispatch,
    edit: (key, text) => byKey(key, { type: "edit", text }),
    replace(key, text) {
      const doc = findIn(get().docs, key);
      if (!doc) return;
      dispatch(doc.id, { type: "edit", text });
      if (get().docs[doc.id]?.text !== text) return;
      for (const apply of editors) apply(doc.id, text);
    },
    save: (key) => byKey(key, { type: "save" }),
    close: (key, choice) => byKey(key, { type: "close", choice }),
    resolve: (key, choice, text) => byKey(key, { type: "resolve", choice, text }),
    restore: (key) => byKey(key, { type: "restore" }),
    discard: (key) => byKey(key, { type: "discard" }),
    take: (key) => byKey(key, { type: "take" }),
    registerEditor(apply) {
      editors.add(apply);
      return () => {
        editors.delete(apply);
      };
    },
    settled: async () => {
      await runner?.settled();
    },
  };
});
