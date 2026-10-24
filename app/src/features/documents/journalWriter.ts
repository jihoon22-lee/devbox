import type { DocKey } from "../../rpc/gen/rpc";
import { keyId, type OfflineEntry, type OfflineJournal } from "./offlineJournal";

export const sameEntry = (a: OfflineEntry | undefined, b: OfflineEntry | undefined) =>
  JSON.stringify(a) === JSON.stringify(b);

/** A renderer may replace only its own last write or the exact recovery snapshot it claimed.
 * Sequences alone cannot order drafts from independent lease sessions. */
export function journalWriter(journal: OfflineJournal) {
  const known = new Map<string, OfflineEntry>();
  return {
    observed(key: DocKey, entry: OfflineEntry | undefined) {
      if (entry) known.set(keyId(key), entry);
      else known.delete(keyId(key));
    },
    async put(key: DocKey, entry: OfflineEntry, allow: (current: OfflineEntry | undefined) => boolean) {
      const expected = known.get(keyId(key));
      const written = await journal.putIf(key, entry, (current) => sameEntry(current, expected) && allow(current));
      if (written) known.set(keyId(key), entry);
      return written;
    },
    async drop(key: DocKey, expected: OfflineEntry) {
      const removed = await journal.dropIf(key, (current) => sameEntry(current, expected));
      if (removed) known.delete(keyId(key));
      return removed;
    },
    move(from: DocKey, to: DocKey) {
      const entry = known.get(keyId(from));
      known.delete(keyId(from));
      if (entry) known.set(keyId(to), entry);
    },
  };
}
