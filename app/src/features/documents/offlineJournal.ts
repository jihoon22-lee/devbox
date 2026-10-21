import { createStore, del, entries, get, set, type UseStore } from "idb-keyval";
import type { DiskRev, DocKey } from "../../rpc/gen/rpc";

/** This device's copy of a document journal: the latest draft, or a drop the server has not confirmed. */
export type OfflineEntry =
  | { kind: "draft"; seq: number; baseRev: DiskRev | null; text: string }
  | { kind: "tombstone"; dropUpto: number };

export interface OfflineJournal {
  put(key: DocKey, entry: OfflineEntry): Promise<void>;
  /** Writes `entry` only when `replace(existing)` allows it, in one step with the read. */
  putIf(key: DocKey, entry: OfflineEntry, replace: (existing: OfflineEntry | undefined) => boolean): Promise<boolean>;
  get(key: DocKey): Promise<OfflineEntry | undefined>;
  /** Removes the entry only when its seq (or a tombstone's `dropUpto`) is at or below `uptoSeq`. */
  drop(key: DocKey, uptoSeq: number): Promise<void>;
  list(): Promise<{ key: DocKey; entry: OfflineEntry }[]>;
  /** Re-keys every entry `rekey` maps (a rename), each read and moved inside its key's queue. */
  move(rekey: (key: DocKey) => DocKey | undefined): Promise<void>;
}

export const keyId = (key: DocKey) => JSON.stringify([key.root, key.path]);
export const entrySeq = (entry: OfflineEntry) => (entry.kind === "draft" ? entry.seq : entry.dropUpto);

type Stored = { key: DocKey; entry: OfflineEntry };
interface Backend {
  get(id: string): Promise<Stored | undefined>;
  set(id: string, value: Stored): Promise<void>;
  del(id: string): Promise<void>;
  all(): Promise<Stored[]>;
}

function journalOver(backend: Backend): OfflineJournal {
  // One document's operations run in order so a drop never races the put before it.
  const chains = new Map<string, Promise<unknown>>();
  let moves: Promise<unknown> = Promise.resolve();
  const serial = <T>(id: string, task: () => Promise<T>): Promise<T> => {
    const next = (chains.get(id) ?? Promise.resolve()).then(task, task);
    const settled = next.catch(() => {});
    chains.set(id, settled);
    void settled.then(() => {
      if (chains.get(id) === settled) chains.delete(id);
    });
    return next;
  };
  const journal: OfflineJournal = {
    put: (key, entry) => serial(keyId(key), () => backend.set(keyId(key), { key, entry })),
    putIf: (key, entry, replace) =>
      serial(keyId(key), async () => {
        if (!replace((await backend.get(keyId(key)))?.entry)) return false;
        await backend.set(keyId(key), { key, entry });
        return true;
      }),
    get: (key) => serial(keyId(key), async () => (await backend.get(keyId(key)))?.entry),
    drop: (key, uptoSeq) =>
      serial(keyId(key), async () => {
        const stored = await backend.get(keyId(key));
        if (stored && entrySeq(stored.entry) <= uptoSeq) await backend.del(keyId(key));
      }),
    list: async () => backend.all(),
    // Moves run one at a time: a move holds its source key's queue while it waits on the target's,
    // so two concurrent moves in opposite directions could otherwise wait on each other.
    move: (rekey) => {
      const run = moves.then(() => moveAll(rekey));
      moves = run.catch(() => {});
      return run;
    },
  };
  async function moveAll(rekey: (key: DocKey) => DocKey | undefined) {
    for (const { key } of await backend.all()) {
      const to = rekey(key);
      // A key mapped onto itself would wait on its own queue.
      if (!to || keyId(to) === keyId(key)) continue;
      await serial(keyId(key), async () => {
        const stored = await backend.get(keyId(key));
        if (!stored) return;
        await serial(keyId(to), async () => {
          const existing = (await backend.get(keyId(to)))?.entry;
          if (!existing || entrySeq(existing) < entrySeq(stored.entry))
            await backend.set(keyId(to), { key: to, entry: stored.entry });
        });
        await backend.del(keyId(key));
      });
    }
  }
  return journal;
}

/** In-memory journal for tests and for environments without IndexedDB. */
export function memoryJournal(): OfflineJournal {
  const map = new Map<string, Stored>();
  return journalOver({
    get: async (id) => map.get(id),
    set: async (id, value) => {
      map.set(id, value);
    },
    del: async (id) => {
      map.delete(id);
    },
    all: async () => [...map.values()],
  });
}

/** IndexedDB journal (latest entry per document). The database opens on first use. */
export function indexedDbJournal(): OfflineJournal {
  let store: UseStore | undefined;
  const db = () => {
    store ??= createStore("devbox-documents", "journal");
    return store;
  };
  return journalOver({
    get: (id) => get<Stored>(id, db()),
    set: (id, value) => set(id, value, db()),
    del: (id) => del(id, db()),
    all: async () => (await entries<string, Stored>(db())).map(([, value]) => value),
  });
}
