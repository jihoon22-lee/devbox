import type { DocumentKind, DocumentStorage } from "./documentStorage";
export const LEGACY_KEYS: Record<DocumentKind, string> = {
  collections: "apip-collections-v2",
  history: "apip-history-v2",
  environments: "apip-environments",
  grpc_history: "devbox.api-playground.grpc-history/v1",
  workflows: "devbox.developer-toolbox.smart-workflows.v1",
};
const RETIRED_KEYS: [string, DocumentKind][] = [
  ["apip-collections", "collections"],
  ["apip-collections-v1-migrated", "collections"],
  ["apip-history", "history"],
  ["apip-history-v1-migrated", "history"],
];
export async function migrateLocalDocuments(
  target: DocumentStorage,
  source: Storage = localStorage,
  prepare: (kind: DocumentKind, body: string) => Promise<string> = async (_kind, body) => body,
): Promise<{ migrated: DocumentKind[]; failed: DocumentKind[] }> {
  const migrated: DocumentKind[] = [],
    failed = new Set<DocumentKind>();
  for (const kind of Object.keys(LEGACY_KEYS) as DocumentKind[]) {
    const key = LEGACY_KEYS[kind];
    try {
      const original = source.getItem(key);
      if (original === null) continue;
      if (kind === "environments" && (failed.has("collections") || failed.has("history"))) {
        failed.add(kind);
        continue; // Keep the sealed source needed to validate a retry.
      }
      const existing = await target.load(kind);
      if (existing === null) {
        const body = await prepare(kind, original);
        const revision = await target.save(kind, body, null);
        const retained = await target.load(kind);
        if (retained?.body !== body || retained.revision !== revision) throw new Error("read-back mismatch");
        migrated.push(kind);
      } else {
        await prepare(kind, existing.body); // Invalid native data cannot justify deleting a valid source.
      }
      // A concurrent edit of the migration source is never erased.
      if (source.getItem(key) !== original) throw new Error("source changed");
      source.removeItem(key);
      if (source.getItem(key) !== null) throw new Error("source deletion failed");
    } catch {
      failed.add(kind);
    }
  }
  for (const [key, kind] of RETIRED_KEYS) {
    try {
      source.removeItem(key);
      if (source.getItem(key) !== null) failed.add(kind);
    } catch {
      failed.add(kind);
    }
  }
  return { migrated, failed: [...failed] };
}
