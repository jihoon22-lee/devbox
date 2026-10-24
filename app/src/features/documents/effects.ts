import { type ErrorCode, RpcError } from "../../rpc/client";
import type { DiskRev, DocKey, LeaseLoss } from "../../rpc/gen/rpc";
import { durableClose } from "./durableClose";
import { journalWriter } from "./journalWriter";
import type { DocEvent, Effect } from "./machine";
import { entrySeq, type OfflineEntry, type OfflineJournal } from "./offlineJournal";
import type { DocumentsClient } from "./store";

export interface Timers {
  set(run: () => void, ms: number): unknown;
  clear(handle: unknown): void;
}
export const browserTimers: Timers = {
  set: (run, ms) => setTimeout(run, ms),
  clear: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
};

export interface SavedDocument {
  id: string;
  leaseId: string;
  seq: number;
  text: string;
}

export interface EffectDeps {
  client: DocumentsClient;
  journal: OfflineJournal;
  windowId: string;
  timers: Timers;
  dispatch(id: string, event: DocEvent): void;
  isLoading(id: string): boolean;
  applyText(id: string, text: string): void;
  saved?(ack: SavedDocument): void;
}

const codeOf = (error: unknown): ErrorCode => (error instanceof RpcError ? error.code : "internal");
const detailOf = (error: unknown): Record<string, unknown> =>
  error instanceof RpcError && error.detail && typeof error.detail === "object"
    ? (error.detail as Record<string, unknown>)
    : {};
const reasonOf = (error: unknown): LeaseLoss | undefined => {
  const reason = detailOf(error).reason;
  return reason === "taken" || reason === "expired" ? reason : undefined;
};
const isRev = (v: unknown): v is DiskRev => {
  const r = v as Partial<DiskRev> | null;
  return (
    !!r &&
    typeof r === "object" &&
    typeof r.hash === "string" &&
    typeof r.size === "number" &&
    typeof r.mtimeNs === "string" &&
    typeof r.ino === "string"
  );
};
const revOf = (error: unknown): DiskRev | null => {
  const rev = detailOf(error).rev;
  return isRev(rev) ? rev : null;
};
const under = (key: DocKey, folder: DocKey) =>
  key.root === folder.root && (key.path === folder.path || key.path.startsWith(`${folder.path}/`));
/** `key` moved with `from` → `to` (a file, or a folder and everything below it). */
export function movedKey(key: DocKey, from: DocKey, to: DocKey): DocKey | undefined {
  return under(key, from) ? { root: to.root, path: to.path + key.path.slice(from.path.length) } : undefined;
}

/** Runs reducer effects against the daemon and this device's journal, feeding results back as events. */
export function createEffectRunner(deps: EffectDeps) {
  const { client, journal, timers, dispatch } = deps;
  const writer = journalWriter(journal);
  const journalTimers = new Map<string, unknown>();
  const saveTimers = new Map<string, unknown>();
  const pending = new Set<Promise<unknown>>();
  const track = (work: Promise<unknown>) => {
    pending.add(work);
    void work.finally(() => pending.delete(work));
  };

  async function claim(id: string, e: Effect & { type: "claim" }) {
    try {
      const params = { key: e.key, policy: e.policy, windowId: deps.windowId };
      const [claimed, offline] = await Promise.all([
        e.take ? client.call("documents.take", params) : client.call("documents.claim", params),
        journal.get(e.key).catch(() => undefined),
      ]);
      // Only Loading consumes this snapshot as recovery. Reclaim keeps an existing buffer
      // and ignores offline, so it cannot grant permission to replace an unseen foreign draft.
      if (claimed.leaseId && deps.isLoading(id)) writer.observed(e.key, offline);
      dispatch(id, { type: "claimed", claimed, offline });
    } catch (error) {
      dispatch(id, { type: "claimFailed", code: codeOf(error) });
    }
  }

  /** Tombstone first, then the daemon drop; the tombstone goes only after the daemon confirmed (AR-F13). */
  async function dropBoth(key: DocKey, uptoSeq: number, leaseId?: string) {
    const tombstone = { kind: "tombstone", dropUpto: uptoSeq } as const;
    try {
      if (!(await writer.put(key, tombstone, (existing) => !existing || entrySeq(existing) <= uptoSeq))) return;
    } catch {
      // Without IndexedDB there is nothing on this device to resurrect; still drop on the daemon.
    }
    await daemonDrop(key, uptoSeq, leaseId, tombstone);
  }
  /** On success, or when another window holds the lease (its journal is not ours to drop), the tombstone goes. */
  async function daemonDrop(key: DocKey, uptoSeq: number, leaseId?: string, expected?: OfflineEntry) {
    try {
      await client.call("documents.journal_drop", leaseId ? { key, uptoSeq, leaseId } : { key, uptoSeq });
    } catch (error) {
      // Offline or failing: the tombstone stays and is retried before any save after the next reconnect.
      // A lease-less retry refused because another window holds the lease also keeps it: a later
      // drop made with a lease clears it.
      if (codeOf(error) !== "documents.lease_lost" || !leaseId) return;
    }
    if (expected) await writer.drop(key, expected).catch(() => undefined);
  }

  async function keepOffline(e: Effect & { type: "offlineJournal" }) {
    try {
      const draft = { kind: "draft", seq: e.seq, baseRev: e.baseRev, text: e.text } as const;
      await writer.put(e.key, draft, (existing) => !existing || entrySeq(existing) < e.seq);
    } catch {
      // The edit stays in the open buffer; the daemon journal is retried on the next tick.
    }
  }

  async function perform(id: string, e: Effect): Promise<void> {
    switch (e.type) {
      case "claim":
        return claim(id, e);
      case "release":
        await client.call("documents.release", { leaseId: e.leaseId }).catch(() => undefined);
        return;
      case "save":
        try {
          const { rev } = await client.call("documents.save", {
            leaseId: e.leaseId,
            seq: e.seq,
            baseRev: e.baseRev,
            expect: e.expect,
            text: e.text,
          });
          dispatch(id, { type: "saveOk", rev });
          // A subscriber cannot turn an acknowledged disk write into saveFailed.
          try {
            deps.saved?.({ id, leaseId: e.leaseId, seq: e.seq, text: e.text });
          } catch {
            // No protocol body or listener exception is logged.
          }
        } catch (error) {
          const code = codeOf(error);
          const rev = code === "documents.conflict" ? revOf(error) : undefined;
          dispatch(id, { type: "saveFailed", code, rev, reason: reasonOf(error) });
        }
        return;
      case "journal":
        try {
          const { leaseId, seq, baseRev, text } = e;
          await client.call("documents.journal", { leaseId, seq, baseRev, text });
          dispatch(id, { type: "journalOk", seq: e.seq });
        } catch (error) {
          dispatch(id, { type: "journalFailed", seq: e.seq, code: codeOf(error), reason: reasonOf(error) });
        }
        return;
      case "journalRead":
        try {
          const j = await client.call("documents.journal_read", { leaseId: e.leaseId });
          dispatch(id, { type: "journalLoaded", seq: j.seq, baseRev: j.baseRev, text: j.text });
        } catch {
          // Recovering stays; the user can still discard the unreadable draft.
        }
        return;
      case "journalDrop":
        return dropBoth(e.key, e.uptoSeq, e.leaseId);
      case "offlineJournal":
        return keepOffline(e);
      case "readDisk":
        try {
          const disk = await client.call("documents.read", { key: e.key, policy: e.policy });
          dispatch(id, { type: "diskLoaded", rev: disk.rev, text: disk.text, readOnly: disk.readOnly });
        } catch {
          // The next change event or reconnect reads again.
        }
        return;
      case "applyText":
        deps.applyText(id, e.text);
        return;
      case "scheduleJournal":
        if (!journalTimers.has(id))
          journalTimers.set(
            id,
            timers.set(() => {
              journalTimers.delete(id);
              dispatch(id, { type: "journalTick" });
            }, e.ms),
          );
        return;
      case "scheduleSave":
        timers.clear(saveTimers.get(id));
        saveTimers.set(
          id,
          timers.set(() => {
            saveTimers.delete(id);
            dispatch(id, { type: "saveTick" });
          }, e.ms),
        );
        return;
    }
  }

  return {
    durable: durableClose(client, journal, writer),
    run(id: string, effects: Effect[]) {
      for (const effect of effects) track(perform(id, effect));
    },
    /** Retries daemon drops that failed earlier; runs before any save after a reconnect. */
    async flushTombstones() {
      const entries = await journal.list().catch(() => []);
      for (const { key, entry } of entries) {
        if (entry.kind === "tombstone") await daemonDrop(key, entry.dropUpto, undefined, entry);
      }
    },
    /** Carries this device's journal entries along a rename (CV-F03): new key first, then the old one goes. */
    async moveOffline(from: DocKey, to: DocKey) {
      await journal
        .move((key) => {
          const moved = movedKey(key, from, to);
          if (moved) writer.move(key, moved);
          return moved;
        })
        .catch(() => undefined);
    },
    track,
    cancel(id: string) {
      for (const map of [journalTimers, saveTimers]) {
        if (map.has(id)) timers.clear(map.get(id));
        map.delete(id);
      }
    },
    cancelAll() {
      for (const id of new Set([...journalTimers.keys(), ...saveTimers.keys()])) this.cancel(id);
    },
    async settled() {
      while (pending.size) await Promise.allSettled([...pending]);
    },
  };
}
export type EffectRunner = ReturnType<typeof createEffectRunner>;
