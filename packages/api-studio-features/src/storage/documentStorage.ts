import { storeCall } from "../calls";
import type { DocumentKind } from "../generated/DocumentKind";
import type { Stored } from "../generated/Stored";
import { storeMessages } from "../issues/catalog";
import type { StoreIssue } from "../generated/StoreIssue";
import { isTauri } from "../requests/lib/isTauri";
export type { DocumentKind, Stored };
export interface DocumentStorage {
  load(kind: DocumentKind): Promise<Stored | null>;
  save(kind: DocumentKind, body: string, expectedRevision: number | null): Promise<number>;
}
export function storageError(code: StoreIssue): Error {
  const error = new Error(storeMessages[code]);
  error.name = code;
  return error;
}
const MAX_DOCUMENT_BYTES = 16 * 1024 * 1024;
function validBody(body: string): void {
  if (new TextEncoder().encode(body).byteLength > MAX_DOCUMENT_BYTES) throw storageError("store_document_too_large");
  try {
    JSON.parse(body);
  } catch {
    throw storageError("store_document_invalid");
  }
}
function revision(value: number): boolean {
  return Number.isSafeInteger(value) && value > 0;
}
const blocked = new Set<DocumentKind>();
export function blockFailedMigrations(kinds: DocumentKind[]): void {
  blocked.clear();
  for (const kind of kinds) blocked.add(kind);
}
function requireReady(kind: DocumentKind): void {
  if (blocked.has(kind)) throw storageError("store_unavailable");
}
/** Direct migration target; ordinary document sessions also enforce failed-migration barriers. */
export const nativeDocumentStorage: DocumentStorage = {
  async load(kind) {
    const value = await storeCall("load", { kind });
    if (value !== null && (!value || !revision(value.revision) || typeof value.body !== "string"))
      throw storageError("store_document_invalid");
    return value;
  },
  async save(kind, body, expectedRevision) {
    validBody(body);
    const value = await storeCall("save", { kind, body, expectedRevision });
    if (!revision(value)) throw storageError("store_document_invalid");
    return value;
  },
};
const browsers = new WeakMap<Storage, DocumentStorage>();
export function browserDocumentStorage(storage: Storage = localStorage): DocumentStorage {
  const existing = browsers.get(storage);
  if (existing) return existing;
  const key = (kind: DocumentKind) => `devbox.api-studio.document.${kind}`;
  const load = (kind: DocumentKind): Stored | null => {
    const body = storage.getItem(key(kind));
    const rawRevision = storage.getItem(`${key(kind)}.revision`);
    if (body === null && rawRevision === null) return null;
    const parsed = Number(rawRevision);
    if (body === null || rawRevision === null || !revision(parsed)) throw storageError("store_document_invalid");
    validBody(body);
    return { body, revision: parsed };
  };
  const result: DocumentStorage = {
    async load(kind) {
      return load(kind);
    },
    async save(kind, body, expected) {
      validBody(body);
      const save = () => {
        const previous = load(kind);
        if ((previous?.revision ?? null) !== expected) throw storageError("store_revision_conflict");
        const next = (expected ?? 0) + 1;
        if (!revision(next)) throw storageError("store_unavailable");
        try {
          storage.setItem(key(kind), body);
          storage.setItem(`${key(kind)}.revision`, String(next));
          const retained = load(kind);
          if (retained?.revision !== next || retained.body !== body) throw storageError("store_unavailable");
          return next;
        } catch (cause) {
          // Roll back this preview write only if its body still owns the slot.
          try {
            if (storage.getItem(key(kind)) === body) {
              if (previous) {
                storage.setItem(key(kind), previous.body);
                storage.setItem(`${key(kind)}.revision`, String(previous.revision));
              } else {
                storage.removeItem(key(kind));
                storage.removeItem(`${key(kind)}.revision`);
              }
            }
          } catch {
            /* Preserve the original write failure. */
          }
          throw cause;
        }
      };
      // Preview tabs share an origin. Web Locks serialize their two-key CAS updates.
      if (typeof navigator !== "undefined" && navigator.locks) return navigator.locks.request(key(kind), save);
      return save();
    },
  };
  browsers.set(storage, result);
  return result;
}
export function documentStorage(): DocumentStorage {
  return isTauri() ? nativeDocumentStorage : browserDocumentStorage();
}
class DocumentSession {
  private current: Stored | null = null;
  private loaded = false;
  private generation = 0;
  constructor(
    private kind: DocumentKind,
    private storage: DocumentStorage,
  ) {}
  get expectedRevision(): number | null {
    if (!this.loaded) throw storageError("store_unavailable");
    return this.current?.revision ?? null;
  }
  async snapshot(): Promise<Stored | null> {
    requireReady(this.kind);
    return this.loaded ? this.current : this.load();
  }
  async load(): Promise<Stored | null> {
    requireReady(this.kind);
    const generation = this.generation;
    const next = await this.storage.load(this.kind);
    // A slow read must not regress the revision/body observed from a later write.
    if (generation !== this.generation && (next?.revision ?? 0) <= (this.current?.revision ?? 0)) return this.current;
    this.current = next;
    this.loaded = true;
    this.generation++;
    return next;
  }
  async save(body: string, expected = this.expectedRevision): Promise<number> {
    requireReady(this.kind);
    try {
      const next = await this.storage.save(this.kind, body, expected);
      const retained = await this.storage.load(this.kind);
      if (!retained || retained.revision !== next || retained.body !== body)
        throw storageError("store_revision_conflict");
      if ((this.current?.revision ?? 0) <= next) {
        this.current = retained;
        this.loaded = true;
        this.generation++;
      }
      return next;
    } catch (cause) {
      if (cause instanceof Error && cause.name === "store_revision_conflict") {
        try {
          await this.load();
        } catch {
          /* The caller still receives the conflict. */
        }
      }
      throw cause;
    }
  }
}
const sessions = new WeakMap<DocumentStorage, Map<DocumentKind, DocumentSession>>();
export function documentSession(kind: DocumentKind, storage: DocumentStorage = documentStorage()): DocumentSession {
  let kinds = sessions.get(storage);
  if (!kinds) {
    kinds = new Map();
    sessions.set(storage, kinds);
  }
  let session = kinds.get(kind);
  if (!session) {
    session = new DocumentSession(kind, storage);
    kinds.set(kind, session);
  }
  return session;
}

export function createDocumentSession(
  kind: DocumentKind,
  storage: DocumentStorage = documentStorage(),
): DocumentSession {
  return new DocumentSession(kind, storage);
}

export function storageFailureMessage(cause: unknown, fallback: string): string {
  if (cause instanceof Error && Object.prototype.hasOwnProperty.call(storeMessages, cause.name))
    return storeMessages[cause.name as StoreIssue];
  return fallback;
}
