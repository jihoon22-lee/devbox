import { documentSession, documentStorage, type DocumentStorage } from "../../storage/documentStorage";
// Collection v2 저장·조회 및 v1 fail-closed 안전 변환.

import type { GraphqlRequest, PersistedHistoryRequest, RequestTemplate } from "../types";
import { isRequestCookie, normalizeCookies } from "./cookies";
import { isRequestHeader, normalizeHeaders } from "./headers";
import { isMultipartPart, normalizeMultipartParts } from "./multipart";
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

export function emptyStore(): CollectionStore {
  return { version: COLLECTION_VERSION, collections: [] };
}

/** Load only the existing v2 shape; legacy migration belongs to the startup boundary. */
export async function migrateCollections(
  sanitize: PersistenceSanitizer,
  storage: DocumentStorage = documentStorage(),
): Promise<StorageMigration<CollectionStore>> {
  try {
    const session = documentSession("collections", storage);
    const document = await session.load();
    const current = document ? parseStore(document.body) : emptyStore();
    if (!current) throw new Error("안전한 Collection 형식이 아닙니다");
    const safe = await sanitizeStore(current, sanitize);
    if (document && JSON.stringify(safe) !== document.body) await session.save(JSON.stringify(safe), document.revision);
    return { store: safe, migrated: false, failed: false, removedLegacyEntries: 0 };
  } catch {
    return { store: emptyStore(), migrated: false, failed: true, removedLegacyEntries: 0 };
  }
}
export async function saveStore(
  store: CollectionStore,
  sanitize: PersistenceSanitizer,
  storage: DocumentStorage = documentStorage(),
  canCommit: () => boolean = () => true,
): Promise<CollectionStore> {
  const session = documentSession("collections", storage);
  const expected = (await session.snapshot())?.revision ?? null;
  const safe = await sanitizeStore(store, sanitize);
  if (!canCommit()) throw new Error("Collection 변경이 오래되어 저장하지 않았습니다");
  await session.save(JSON.stringify(safe), expected);
  return safe;
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
export function duplicateEntry(store: CollectionStore, id: string, now: number, makeId: () => string): CollectionStore {
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
    collections: store.collections.map((entry) => (entry.id === id ? { ...entry, name: normalized } : entry)),
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

export async function sanitizeStore(store: CollectionStore, sanitize: PersistenceSanitizer): Promise<CollectionStore> {
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
    (request.cookies === undefined || (Array.isArray(request.cookies) && request.cookies.every(isRequestCookie))) &&
    (request.multipart === undefined ||
      (Array.isArray(request.multipart) && request.multipart.every(isMultipartPart))) &&
    Array.isArray(request.params) &&
    request.params.every(isKeyValue) &&
    typeof request.body_kind === "string" &&
    typeof request.body === "string" &&
    (request.graphql === undefined || request.graphql === null || isGraphqlRequest(request.graphql)) &&
    typeof request.timeout_ms === "number"
  );
}

function isGraphqlRequest(value: unknown): value is GraphqlRequest {
  if (!value || typeof value !== "object") return false;
  const request = value as Partial<GraphqlRequest>;
  return (
    typeof request.query === "string" &&
    typeof request.variables === "string" &&
    typeof request.operation_name === "string"
  );
}

function isKeyValue(value: unknown): value is { key: string; value: string } {
  if (!value || typeof value !== "object") return false;
  const pair = value as { key?: unknown; value?: unknown };
  return typeof pair.key === "string" && typeof pair.value === "string";
}

function isPersistedRequest(value: unknown): value is PersistedHistoryRequest {
  return (
    isRequestTemplate(value) && typeof (value as Partial<PersistedHistoryRequest>).requiresSecretReview === "boolean"
  );
}

function copyName(name: string): string {
  const suffix = " 복사본";
  return `${name.slice(0, 120 - suffix.length)}${suffix}`;
}

function normalizeName(name: string): string {
  return name
    .replace(/[\r\n]+/g, " ")
    .trim()
    .slice(0, 120);
}

function clonePersistedRequest(request: PersistedHistoryRequest): PersistedHistoryRequest {
  return {
    ...request,
    headers: normalizeHeaders(request.headers),
    cookies: normalizeCookies(request.cookies),
    multipart: normalizeMultipartParts(request.multipart),
    params: request.params.map((param) => ({ ...param })),
    auth: request.auth ? { ...request.auth } : null,
    ...(request.body_kind === "graphql" && request.graphql
      ? {
          graphql: {
            query: request.graphql.query,
            variables: request.graphql.variables,
            operation_name: request.graphql.operation_name,
          },
        }
      : {}),
  };
}
