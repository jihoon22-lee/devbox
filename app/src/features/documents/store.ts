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
export type ClosePrompt = (docs: Doc[], signal: AbortSignal) => Promise<Record<string, CloseChoice> | undefined>;
export const needsCloseChoice = (doc: Doc) =>
  doc.editSeq > doc.savedSeq || doc.state === "Recovering" || doc.state === "Conflict" || doc.state === "Orphaned";

export interface DocumentsStore {
  docs: Record<string, Doc>;
  windowClosing: boolean;
  closingDocs: Record<string, boolean>;
  closeSafely(key: DocKey, choice?: CloseChoice): Promise<boolean>;
  abortWindowClose(): void;
  /** Freeze edits and await actual saves, including older in-flight writes. False keeps buffers visible. */
  prepareWindowClose(choose?: ClosePrompt): Promise<boolean>;
  cancelWindowClose(): void;
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
  let closeRequest: AbortController | undefined;
  let closedForWindow: { key: DocKey; policy: DocPolicy }[] = [];
  const editors = new Set<(id: string, text: string) => void>();

  const dispatch = (id: string, event: DocEvent) => {
    const doc = get().docs[id];
    if (!doc || ((get().windowClosing || get().closingDocs[id]) && event.type === "edit")) return;
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
    windowClosing: false,
    closingDocs: {},
    abortWindowClose() {
      closeRequest?.abort();
    },
    async closeSafely(key, choice) {
      const initial = get().find(key);
      if (!initial) return true;
      if (get().closingDocs[initial.id]) return false;
      const own = runner;
      if (!own) return false;
      const id = initial.id;
      set({ closingDocs: { ...get().closingDocs, [id]: true } });
      own.cancel(id);
      try {
        await own.settled();
        const doc = get().docs[id];
        if (!doc) return true;
        if (runner !== own || keyId(doc.key) !== keyId(key)) return false;
        if (needsCloseChoice(doc)) {
          if (!choice) return false;
          if (doc.state === "Recovering" && doc.editSeq > doc.savedSeq && choice !== "discardAll") return false;
          if (choice === "save") {
            if (doc.state !== "Dirty" && doc.state !== "Orphaned" && doc.state !== "Clean") return false;
            if (doc.state !== "Clean") dispatch(id, { type: "save" });
            await own.settled();
            const saved = get().docs[id];
            if (!saved || saved.state !== "Clean" || saved.editSeq > saved.savedSeq) return false;
          } else {
            const durable = choice === "keep" ? await own.durable.keep(doc) : await own.durable.discard(doc);
            if (!durable) return false;
          }
        }
        const current = get().docs[id];
        if (
          !current ||
          runner !== own ||
          keyId(current.key) !== keyId(doc.key) ||
          current.editSeq !== doc.editSeq ||
          current.leaseId !== doc.leaseId
        )
          return false;
        if (!(await own.durable.release(current))) return false;
        const latest = get().docs[id];
        if (runner !== own || !latest || keyId(latest.key) !== keyId(current.key) || latest.editSeq !== current.editSeq)
          return false;
        const { [id]: _closed, ...rest } = get().docs;
        set({ docs: rest });
        own.cancel(id);
        return true;
      } finally {
        const { [id]: _closing, ...rest } = get().closingDocs;
        set({ closingDocs: rest });
      }
    },
    cancelWindowClose() {
      set({ windowClosing: false });
      const reopen = closedForWindow;
      closedForWindow = [];
      for (const doc of reopen) get().open(doc.key, doc.policy);
    },
    async prepareWindowClose(choose) {
      if (get().windowClosing) return false;
      set({ windowClosing: true });
      const request = new AbortController();
      closeRequest = request;
      const own = runner;
      const fail = () => {
        get().cancelWindowClose();
        return false;
      };
      // settle does not flush debounce timers. Drain older writes first, then explicitly
      // save the latest buffer; never use journalSeq (scheduled, not durable) as an ACK.
      await own?.settled();
      if (runner !== own) return fail();
      const code = Object.values(get().docs).filter((doc) => doc.policy.kind === "code");
      const dirty = code.filter(needsCloseChoice);
      const choices = dirty.length && choose ? await choose(dirty, request.signal).catch(() => undefined) : undefined;
      if ((dirty.length && !choices) || request.signal.aborted || runner !== own) return fail();
      for (const doc of code) {
        if (request.signal.aborted || (needsCloseChoice(doc) && !choices?.[doc.id])) return fail();
        if (!(await get().closeSafely(doc.key, choices?.[doc.id]))) return fail();
        closedForWindow.push({ key: doc.key, policy: doc.policy });
      }
      if (request.signal.aborted) return fail();
      for (const doc of Object.values(get().docs)) {
        if (doc.policy.kind === "note" && (doc.state === "Dirty" || doc.state === "Orphaned"))
          dispatch(doc.id, { type: "save" });
      }
      await own?.settled();
      if (runner !== own) return fail();
      const safe = Object.values(get().docs).every(
        (doc) => (doc.state === "Clean" || doc.state === "ReadOnly") && doc.editSeq <= doc.savedSeq,
      );
      if (!safe) return fail();
      own?.cancelAll();
      closedForWindow.push(...Object.values(get().docs).map(({ key, policy }) => ({ key, policy })));
      for (const doc of Object.values(get().docs)) dispatch(doc.id, { type: "close" });
      await own?.settled();
      if (runner !== own || Object.keys(get().docs).length) return fail();
      // Remain frozen until the native window is destroyed (or destroy explicitly fails).
      return true;
    },
    connect(client, deps = {}) {
      closedForWindow = [];
      closeRequest?.abort();
      closeRequest = undefined;
      const journal = deps.journal ?? indexedDbJournal();
      const own = createEffectRunner({
        client,
        journal,
        windowId: deps.windowId ?? windowId(),
        timers: deps.timers ?? browserTimers,
        dispatch,
        isLoading: (id) => get().docs[id]?.state === "Loading",
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
          closeRequest?.abort();
          set({ docs: {}, windowClosing: false, closingDocs: {} });
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
