// Collection v2 저장·조회 및 v1 fail-closed 안전 변환.

import type { PersistedHistoryRequest, RequestTemplate } from "../types";
import { isRequestHeader, normalizeHeaders } from "./headers";
import {
  type PersistenceSanitizer,
  type StorageMigration,
  normalizePersistedRequest,
  sanitizeRequestForPersistence,
} from "./persistence";

export const COLLECTION_V1_LS_KEY = "apip-collections";
export const COLLECTION_V2_LS_KEY = "apip-collections-v2";
export const COLLECTION_V1_MARKER_KEY = "apip-collections-v1-migrated";
export const COLLECTION_VERSION = 2;

export interface CollectionEntry {
  id: string;
  name: string;
  folder: string;
  saved_at: number;
  request: PersistedHistoryRequest;
  requiresSecretReview: boolean;
}

export interface CollectionStore {
  version: 2;
  collections: CollectionEntry[];
}

interface LegacyCollectionEntry {
  id?: unknown;
  name?: unknown;
  folder?: unknown;
  saved_at?: unknown;
  request?: unknown;
}

export function emptyStore(): CollectionStore {
  return { version: COLLECTION_VERSION, collections: [] };
}

/** v2는 backend 검증 전까지 반환하지 않으며, v1 raw는 어떤 경우에도 UI에 노출하지 않는다. */
export async function migrateCollections(
  sanitize: PersistenceSanitizer,
  storage: Storage = localStorage,
): Promise<StorageMigration<CollectionStore>> {
  const rawV1 = storage.getItem(COLLECTION_V1_LS_KEY);
  try {
    const current = parseStore(storage.getItem(COLLECTION_V2_LS_KEY));
    const legacy = current ? null : parseLegacyStore(rawV1);
    const candidate = current ?? legacy?.store ?? emptyStore();
    const safe = await sanitizeStore(candidate, sanitize);

    storage.setItem(COLLECTION_V2_LS_KEY, JSON.stringify(safe));
    const readBack = parseStore(storage.getItem(COLLECTION_V2_LS_KEY));
    if (!readBack) throw new Error("collection v2 read-back failed");

    if (rawV1 !== null) {
      storage.removeItem(COLLECTION_V1_LS_KEY);
      if (storage.getItem(COLLECTION_V1_LS_KEY) !== null) {
        throw new Error("legacy collection deletion failed");
      }
    }
    storage.setItem(COLLECTION_V1_MARKER_KEY, "2");
    if (storage.getItem(COLLECTION_V1_MARKER_KEY) !== "2") {
      throw new Error("collection marker write failed");
    }
    return {
      store: readBack,
      migrated: rawV1 !== null,
      failed: false,
      removedLegacyEntries: legacy?.removedUnsafeValues ?? 0,
    };
  } catch {
    return { store: emptyStore(), migrated: false, failed: true, removedLegacyEntries: 0 };
  }
}

export async function saveStore(
  store: CollectionStore,
  sanitize: PersistenceSanitizer,
  storage: Storage = localStorage,
): Promise<CollectionStore> {
  const safe = await sanitizeStore(store, sanitize);
  storage.setItem(COLLECTION_V2_LS_KEY, JSON.stringify(safe));
  const readBack = parseStore(storage.getItem(COLLECTION_V2_LS_KEY));
  if (!readBack) throw new Error("Collection 안전 저장을 확인할 수 없습니다");
  return readBack;
}

export function addEntry(
  store: CollectionStore,
  input: { name: string; folder: string; request: RequestTemplate },
  now: number,
  makeId: () => string,
): CollectionStore {
  const request = sanitizeRequestForPersistence(input.request);
  const entry: CollectionEntry = {
    id: makeId(),
    name: input.name.trim() || input.request.url || "untitled",
    folder: input.folder.trim(),
    saved_at: now,
    request,
    requiresSecretReview: request.requiresSecretReview,
  };
  return { ...store, collections: [entry, ...store.collections] };
}

export function removeEntry(store: CollectionStore, id: string): CollectionStore {
  return { ...store, collections: store.collections.filter((entry) => entry.id !== id) };
}

/** 저장된 마스킹 request를 다시 원본 template로 만들지 않고 그대로 복제한다. */
export function duplicateEntry(
  store: CollectionStore,
  id: string,
  now: number,
  makeId: () => string,
): CollectionStore {
  const source = store.collections.find((entry) => entry.id === id);
  if (!source) return store;
  const duplicate: CollectionEntry = {
    ...source,
    id: makeId(),
    name: copyName(source.name),
    saved_at: now,
    request: clonePersistedRequest(source.request),
  };
  return { ...store, collections: [duplicate, ...store.collections] };
}

export function renameEntry(store: CollectionStore, id: string, name: string): CollectionStore {
  const normalized = normalizeName(name);
  if (!normalized) return store;
  return {
    ...store,
    collections: store.collections.map((entry) =>
      entry.id === id ? { ...entry, name: normalized } : entry
    ),
  };
}

export function foldersOf(store: CollectionStore): string[] {
  const folders = new Set(store.collections.map((entry) => entry.folder).filter(Boolean));
  return [...folders].sort();
}

export function parseStore(raw: string | null): CollectionStore | null {
  try {
    const parsed = JSON.parse(raw ?? "null") as Partial<CollectionStore> | null;
    if (parsed?.version !== COLLECTION_VERSION || !Array.isArray(parsed.collections)) return null;
    if (!parsed.collections.every(isCollectionEntry)) return null;
    return {
      version: COLLECTION_VERSION,
      collections: parsed.collections.map((entry) => ({
        ...entry,
        request: normalizePersistedRequest(entry.request),
      })),
    };
  } catch {
    return null;
  }
}

async function sanitizeStore(
  store: CollectionStore,
  sanitize: PersistenceSanitizer,
): Promise<CollectionStore> {
  const original = JSON.stringify(store);
  const serialized = await sanitize(original);
  const parsed = parseStore(serialized);
  if (!parsed) throw new Error("안전한 Collection 형식이 아닙니다");
  if (serialized === original) return parsed;
  return {
    ...parsed,
    collections: parsed.collections.map((entry) => ({
      ...entry,
      requiresSecretReview: true,
      request: { ...entry.request, requiresSecretReview: true },
    })),
  };
}

function parseLegacyStore(raw: string | null): { store: CollectionStore; removedUnsafeValues: number } | null {
  if (raw === null) return null;
  try {
    const parsed = JSON.parse(raw) as { version?: unknown; collections?: unknown };
    if (parsed?.version !== 1 || !Array.isArray(parsed.collections)) return { store: emptyStore(), removedUnsafeValues: 1 };
    let removedUnsafeValues = 0;
    const collections = parsed.collections.flatMap((candidate: LegacyCollectionEntry, index) => {
      if (!isRequestTemplate(candidate?.request)) {
        removedUnsafeValues += 1;
        return [];
      }
      const request = sanitizeRequestForPersistence(candidate.request);
      if (request.requiresSecretReview) removedUnsafeValues += 1;
      return [{
        id: typeof candidate.id === "string" ? candidate.id : `migrated-${index}`,
        name: typeof candidate.name === "string" ? candidate.name : candidate.request.url || "untitled",
        folder: typeof candidate.folder === "string" ? candidate.folder : "",
        saved_at: typeof candidate.saved_at === "number" ? candidate.saved_at : 0,
        request,
        requiresSecretReview: request.requiresSecretReview,
      }];
    });
    return { store: { version: COLLECTION_VERSION, collections }, removedUnsafeValues };
  } catch {
    return { store: emptyStore(), removedUnsafeValues: 1 };
  }
}

function isCollectionEntry(value: unknown): value is CollectionEntry {
  if (!value || typeof value !== "object") return false;
  const entry = value as Partial<CollectionEntry>;
  return (
    typeof entry.id === "string" &&
    typeof entry.name === "string" &&
    typeof entry.folder === "string" &&
    typeof entry.saved_at === "number" &&
    typeof entry.requiresSecretReview === "boolean" &&
    isPersistedRequest(entry.request)
  );
}

function isRequestTemplate(value: unknown): value is RequestTemplate {
  if (!value || typeof value !== "object") return false;
  const request = value as Partial<RequestTemplate>;
  return (
    typeof request.method === "string" &&
    typeof request.url === "string" &&
    Array.isArray(request.headers) &&
    request.headers.every(isRequestHeader) &&
    Array.isArray(request.params) &&
    request.params.every(isKeyValue) &&
    typeof request.body_kind === "string" &&
    typeof request.body === "string" &&
    typeof request.timeout_ms === "number"
  );
}

function isKeyValue(value: unknown): value is { key: string; value: string } {
  if (!value || typeof value !== "object") return false;
  const pair = value as { key?: unknown; value?: unknown };
  return typeof pair.key === "string" && typeof pair.value === "string";
}

function isPersistedRequest(value: unknown): value is PersistedHistoryRequest {
  return isRequestTemplate(value) && typeof (value as Partial<PersistedHistoryRequest>).requiresSecretReview === "boolean";
}

function copyName(name: string): string {
  const suffix = " 복사본";
  return `${name.slice(0, 120 - suffix.length)}${suffix}`;
}

function normalizeName(name: string): string {
  return name.replace(/[\r\n]+/g, " ").trim().slice(0, 120);
}

function clonePersistedRequest(request: PersistedHistoryRequest): PersistedHistoryRequest {
  return {
    ...request,
    headers: normalizeHeaders(request.headers),
    params: request.params.map((param) => ({ ...param })),
    auth: request.auth ? { ...request.auth } : null,
  };
}
