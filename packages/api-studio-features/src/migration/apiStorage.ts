/** API's legacy storage codec, used only against a consistent owned copy.
 * No function here reads or writes the live legacy browser storage. */
import { migrateCollections, parseStore as parseCollections, type CollectionStore } from "../requests/lib/collections";
import { parseStore as parseEnvironments, type EnvironmentStore } from "../requests/lib/environments";
import { parseGrpcHistory, type GrpcHistoryStore } from "../requests/lib/grpc";
import { parseHistoryStore, saveHistoryStore, type HistoryStore } from "../requests/lib/persistence";

import { API_LEGACY_KEYS, type LegacyKey, type LegacyStorage } from "./keys";
export { API_LEGACY_KEYS, type LegacyKey, type LegacyStorage } from "./keys";
export type ApiStore = "collections" | "history" | "environments" | "grpc-history";
export interface ImportNotice {
  store: ApiStore;
  code: "legacy-history-excluded" | "records-excluded" | "values-redacted" | "secret-reconnect-required";
  count: number;
}
export interface LegacyApiExport {
  schemaVersion: 1;
  collections: CollectionStore | null;
  history: HistoryStore | null;
  environments: EnvironmentStore | null;
  grpcHistory: GrpcHistoryStore | null;
  notices: ImportNotice[];
}
export interface ExportOwner {
  /** Native API owner checks DPAPI; returns ciphertext or empty reconnect slots. */
  prepareEnvironment(raw: string | null): Promise<{ serialized: string | null; missingSecrets: number }>;
  /** Native API owner redacts using the verified environment for this export. */
  sanitize(serialized: string): Promise<string>;
}
const MAX_BYTES = 20 * 1024 * 1024;
const MAX_RECORDS = 10_000;
const invalid = () => new Error("legacy_api_storage_invalid");

class CopyStorage implements Storage {
  private readonly values = new Map<string, string>();
  get length() { return this.values.size; }
  clear() { this.values.clear(); }
  getItem(key: string) { return this.values.get(key) ?? null; }
  key(index: number) { return [...this.values.keys()][index] ?? null; }
  removeItem(key: string) { this.values.delete(key); }
  setItem(key: string, value: string) { this.values.set(key, value); }
}
function envelope(raw: string, version: number, field: string): Record<string, unknown> {
  let value: unknown;
  try { value = JSON.parse(raw); } catch { throw invalid(); }
  if (!value || typeof value !== "object" || Array.isArray(value)) throw invalid();
  const record = value as Record<string, unknown>;
  if (record.version !== version || !Array.isArray(record[field]) || record[field].length > MAX_RECORDS) throw invalid();
  return record;
}
export function readLegacyApiStorage(storage: Pick<Storage, "getItem">): LegacyStorage {
  return Object.fromEntries(API_LEGACY_KEYS.map((key) => [key, storage.getItem(key)])) as LegacyStorage;
}
export async function normalizeLegacyApiStorage(raw: LegacyStorage, owner: ExportOwner): Promise<LegacyApiExport> {
  if (Object.keys(raw).length !== API_LEGACY_KEYS.length || Object.keys(raw).some((key) => !API_LEGACY_KEYS.includes(key as LegacyKey))) throw invalid();
  let bytes = 0;
  for (const value of Object.values(raw)) {
    if (value !== null && typeof value !== "string") throw invalid();
    bytes += new TextEncoder().encode(value ?? "").length;
    if (bytes > MAX_BYTES) throw invalid();
  }
  const source = new CopyStorage();
  for (const [key, value] of Object.entries(raw)) if (value !== null) source.setItem(key, value);
  const notices: ImportNotice[] = [];
  if (raw["apip-environments"] !== null) {
    envelope(raw["apip-environments"], 1, "environments");
    if (!parseEnvironments(raw["apip-environments"])) throw invalid();
  }
  const checked = await owner.prepareEnvironment(raw["apip-environments"]);
  const environments = checked.serialized === null ? null : parseEnvironments(checked.serialized);
  if ((checked.serialized !== null && !environments) || (raw["apip-environments"] !== null && !environments)
    || !Number.isSafeInteger(checked.missingSecrets) || checked.missingSecrets < 0) throw invalid();
  if (checked.missingSecrets) notices.push({ store: "environments", code: "secret-reconnect-required", count: checked.missingSecrets });

  let collections: CollectionStore | null = null;
  const collectionSource = raw["apip-collections-v2"] ?? raw["apip-collections"];
  if (collectionSource !== null) {
    const version = raw["apip-collections-v2"] !== null ? 2 : 1;
    const entries = envelope(collectionSource, version, "collections").collections as unknown[];
    if (version === 2 && !parseCollections(collectionSource)) throw invalid();
    // Unknown source secrets cannot be reliably removed from an older raw v1
    // collection. Preserve the source and require reconnection before importing.
    if (version === 1 && checked.missingSecrets > 0) throw new Error("legacy_api_secret_reconnect_required");
    const result = await migrateCollections(owner.sanitize, source);
    if (result.failed) throw invalid();
    collections = result.store;
    if (result.removedLegacyEntries > 0) notices.push({ store: "collections", code: "values-redacted", count: result.removedLegacyEntries });
    if (entries.length > collections.collections.length) notices.push({ store: "collections", code: "records-excluded", count: entries.length - collections.collections.length });
  }
  let history: HistoryStore | null = null;
  if (raw["apip-history-v2"] !== null) {
    const entries = envelope(raw["apip-history-v2"], 2, "history").history as unknown[];
    const parsed = parseHistoryStore(raw["apip-history-v2"]);
    if (!parsed) throw invalid();
    history = await saveHistoryStore(parsed, owner.sanitize, source);
    if (entries.length > history.history.length) notices.push({ store: "history", code: "records-excluded", count: entries.length - history.history.length });
  }
  // Match the preserved v0.7 policy: never return raw v1 History for replay.
  if (raw["apip-history"] !== null) notices.push({ store: "history", code: "legacy-history-excluded", count: 1 });
  const grpcRaw = raw["devbox.api-playground.grpc-history/v1"];
  const grpcHistory = grpcRaw === null ? null : parseGrpcHistory(grpcRaw);
  if (grpcRaw !== null && !grpcHistory) throw invalid();
  return { schemaVersion: 1, collections, history, environments, grpcHistory, notices };
}
