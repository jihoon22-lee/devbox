import { syncEditorDocument } from "./api";

interface Snapshot { id: string; path: string; nativeRevision?: string | null; text: string }
type Send = typeof syncEditorDocument;

/** All editor buffers participate, independently of language-server support. */
export class NativeEditorMirror {
  private context = "";
  private generation = 0;
  private documents = new Map<string, { snapshot: Snapshot; pending: Promise<unknown> }>();

  constructor(private readonly send: Send = syncEditorDocument) {}

  setContext(context: string): void {
    if (context === this.context) return;
    this.context = context;
    this.generation += 1;
    this.documents.clear();
  }

  update(documents: readonly Snapshot[]): void {
    const ids = new Set(documents.map(document => document.id));
    for (const id of this.documents.keys()) if (!ids.has(id)) this.documents.delete(id);
    for (const document of documents) {
      if (!document.nativeRevision) continue;
      const previous = this.documents.get(document.id);
      if (previous?.snapshot.path === document.path
        && previous.snapshot.nativeRevision === document.nativeRevision
        && previous.snapshot.text === document.text) continue;
      const snapshot = { ...document };
      const generation = this.generation;
      const current = { snapshot, pending: Promise.resolve() as Promise<unknown> };
      this.documents.set(document.id, current);
      current.pending = (previous?.pending ?? Promise.resolve()).catch(() => undefined).then(() => {
        if (generation !== this.generation || this.documents.get(document.id) !== current) return;
        return this.send(snapshot.path, snapshot.nativeRevision!, snapshot.text);
      });
      // Background edits remain usable on failure; flush keeps the rejection
      // so a rename cannot silently proceed with an unacknowledged buffer.
      void current.pending.catch(() => undefined);
    }
  }

  async flush(documents: readonly Snapshot[]): Promise<void> {
    const generation = this.generation;
    this.update(documents);
    for (;;) {
      const pending = [...this.documents.values()];
      await Promise.all(pending.map(document => document.pending));
      if (generation !== this.generation) throw new Error("프로젝트가 변경되어 문서 확인을 취소했습니다.");
      const current = [...this.documents.values()];
      if (current.length === pending.length && current.every((document, index) => document === pending[index])) return;
    }
  }
}
