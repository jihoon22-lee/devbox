import { journalWriter, sameEntry } from "./journalWriter";
import type { Doc } from "./machine";
import { entrySeq, type OfflineJournal } from "./offlineJournal";
import type { DocumentsClient } from "./store";

/** Closing needs an acknowledged durable write, unlike best-effort background journaling. */
export function durableClose(client: DocumentsClient, journal: OfflineJournal, writer = journalWriter(journal)) {
  return {
    async keep(doc: Doc): Promise<boolean> {
      // Once another window owns the lease, neither shared journal is ours to replace.
      if (doc.state === "Recovering" || doc.state === "ReadOnly") return false;
      const draft = { kind: "draft", seq: doc.editSeq, baseRev: doc.diskRev, text: doc.text } as const;
      if (client.state === "ready") {
        if (!doc.leaseId) return false;
        try {
          // Online writes use the daemon's lease CAS, never an unconditional shared IndexedDB put.
          await client
            .call("documents.journal", {
              leaseId: doc.leaseId,
              seq: draft.seq,
              baseRev: draft.baseRev,
              text: draft.text,
            })
            .catch(() => {});
          const saved = await client.call("documents.journal_read", { leaseId: doc.leaseId });
          return (
            saved.seq === draft.seq &&
            saved.text === draft.text &&
            JSON.stringify(saved.baseRev) === JSON.stringify(draft.baseRev)
          );
        } catch {
          return false;
        }
      }
      try {
        if (!(await writer.put(doc.key, draft, (current) => !current || entrySeq(current) <= draft.seq))) return false;
        const saved = await journal.get(doc.key);
        return (
          saved?.kind === "draft" &&
          saved.seq === draft.seq &&
          saved.text === draft.text &&
          JSON.stringify(saved.baseRev) === JSON.stringify(draft.baseRev)
        );
      } catch {
        return false;
      }
    },
    async discard(doc: Doc): Promise<boolean> {
      const uptoSeq = Math.max(doc.editSeq, doc.journalSeq, doc.recovery?.seq ?? 0, doc.recovery?.other?.seq ?? 0);
      try {
        // After a take, both shared journals belong to the new owner. Only forget this buffer.
        if (doc.state === "ReadOnly" && !doc.leaseId) return true;
        const tombstone = { kind: "tombstone", dropUpto: uptoSeq } as const;
        if (!(await writer.put(doc.key, tombstone, (current) => !current || entrySeq(current) <= uptoSeq)))
          return false;
        if (!sameEntry(await journal.get(doc.key), tombstone)) return false;
        await client.call("documents.journal_drop", {
          key: doc.key,
          uptoSeq,
          ...(doc.leaseId ? { leaseId: doc.leaseId } : {}),
        });
        await writer.drop(doc.key, tombstone);
        return !(await journal.get(doc.key));
      } catch {
        return false;
      }
    },
    async release(doc: Doc): Promise<boolean> {
      if (!doc.leaseId || client.state !== "ready") return true; // Closed channels release their own leases.
      try {
        await client.call("documents.release", { leaseId: doc.leaseId });
        return true;
      } catch {
        return false;
      }
    },
  };
}
