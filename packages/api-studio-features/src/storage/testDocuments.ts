import { storageError, type DocumentKind, type DocumentStorage, type Stored } from "./documentStorage";
/** Synthetic document persistence for feature tests; no WebView or user data. */
export class MemoryDocuments implements DocumentStorage {
  readonly docs = new Map<DocumentKind, Stored>();
  readonly events: string[] = [];
  failWrite = false;
  async load(kind: DocumentKind): Promise<Stored | null> {
    return this.docs.get(kind) ?? null;
  }
  async save(kind: DocumentKind, body: string, expected: number | null): Promise<number> {
    if (this.failWrite) throw new Error("write failed");
    if ((this.docs.get(kind)?.revision ?? null) !== expected) throw storageError("store_revision_conflict");
    const revision = (expected ?? 0) + 1;
    this.events.push(`save:${kind}`);
    this.docs.set(kind, { revision, body });
    return revision;
  }
  seed(kind: DocumentKind, body: string): void {
    this.docs.set(kind, { revision: 1, body });
  }
  body(kind: DocumentKind): string | null {
    return this.docs.get(kind)?.body ?? null;
  }
  entries(): [DocumentKind, Stored][] {
    return [...this.docs.entries()];
  }
}

export const previewDocumentKey = (kind: DocumentKind): string => `devbox.api-studio.document.${kind}`;
export function seedPreviewDocument(kind: DocumentKind, body: string): void {
  localStorage.setItem(previewDocumentKey(kind), body);
  localStorage.setItem(`${previewDocumentKey(kind)}.revision`, "1");
}
