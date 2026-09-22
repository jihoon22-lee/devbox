import type { InboundNote, NoteSnapshot } from "./api";

export interface NoteView {
  sourceVersion: number; path: string | null; content: string; revision: string; dirty: boolean;
  saving: boolean; conflict: NoteSnapshot | null; error: string | null;
}
type Writer = (path: string, content: string, revision: string) => Promise<NoteSnapshot>;
const empty: NoteView = { sourceVersion: 0, path: null, content: "", revision: "", dirty: false, saving: false, conflict: null, error: null };

/** One editor, one ordered writer. Native revisions are independent of edit revisions. */
export class NoteDocument {
  private view: NoteView = empty;
  private listeners = new Set<() => void>();
  private document = 0;
  private edits = 0;
  private opening = 0;
  private inspecting = 0;
  private saves = 0;
  private writing: Promise<boolean> | null = null;
  constructor(private read: (path: string) => Promise<NoteSnapshot>, private write: Writer) {}
  snapshot = () => this.view;
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; };
  private publish(change: Partial<NoteView>) {
    // Reopening identical bytes is still a new source. Save/inspect status alone
    // does not invalidate a preview of unchanged editor contents.
    const sourceVersion = this.view.sourceVersion + ("path" in change || "content" in change ? 1 : 0);
    this.view = { ...this.view, ...change, sourceVersion };
    for (const listener of this.listeners) listener();
  }
  edit(content: string) { this.edits++; this.publish({ content, dirty: true }); }
  clear() { this.opening++; this.document++; this.edits++; this.publish({ ...empty, saving: !!this.writing }); }
  /** Capture the exact buffer approved for deletion; completion is one-shot. */
  approveRemoval(target: string): () => boolean {
    const affected = (path: string | null) => path === target || !!path?.startsWith(`${target}/`);
    const { path } = this.view;
    const document = this.document, edits = this.edits, opening = this.opening, saves = this.saves;
    const saving = !!this.writing;
    let completed = false;
    return () => {
      if (completed) return false;
      completed = true;
      if (!affected(path) || document !== this.document || path !== this.view.path) return false;
      if (edits !== this.edits || opening !== this.opening || saves !== this.saves || saving || this.writing) {
        // A pending save must not mark this retained buffer clean afterward.
        if (this.writing) this.edits++;
        this.publish({ dirty: true, error: "파일은 삭제했지만 삭제 승인 이후의 노트 내용은 유지했습니다. 디스크 상태를 확인해 주세요." });
        return false;
      }
      this.clear();
      return true;
    };
  }
  async open(load: () => Promise<InboundNote>, discard: () => boolean): Promise<boolean> {
    const request = ++this.opening;
    if (this.view.dirty && !discard()) return false;
    const edits = this.edits;
    this.publish({ error: null });
    try {
      const note = await load();
      if (request !== this.opening) return false;
      if (edits !== this.edits) {
        this.publish({ error: "파일을 읽는 동안 편집한 내용을 유지했습니다. 노트를 다시 선택해 주세요." });
        return false;
      }
      this.document++;
      this.publish({ path: note.path, content: note.content, revision: note.revision, dirty: false, conflict: null, error: null });
      return true;
    } catch (error) {
      if (request === this.opening) this.publish({ error: error instanceof Error ? error.message : "노트를 열지 못했습니다." });
      return false;
    }
  }
  openPath(path: string, discard: () => boolean) {
    return this.open(async () => {
      const note = await this.read(path);
      if (note.content === null) throw new Error("파일이 삭제되었습니다. 현재 편집 내용은 유지됩니다.");
      return { ...note, path, content: note.content };
    }, discard);
  }
  async renamed(from: string, to: string): Promise<void> {
    const current = this.view.path;
    if (!current) return;
    const path = current === from ? to : current.startsWith(`${from}/`) ? to + current.slice(from.length) : current;
    const document = this.document, edits = this.edits;
    this.opening++;
    this.inspecting++;
    this.publish({ path, revision: "" });
    try {
      const saved = await this.read(path);
      if (document !== this.document) return;
      if (saved.content === null) throw new Error("missing");
      if (edits !== this.edits || this.view.dirty) {
        this.publish({ conflict: saved });
      } else {
        this.publish({ content: saved.content, revision: saved.revision, conflict: null });
      }
    } catch {
      if (document === this.document) this.publish({
        ...(this.view.dirty ? {} : { content: "" }),
        error: "이름은 변경했지만 현재 노트를 다시 읽지 못했습니다",
      });
    }
  }
  async inspect(): Promise<void> {
    const { path, revision } = this.view, document = this.document;
    const inspection = ++this.inspecting;
    const current = () => inspection === this.inspecting && document === this.document
      && path === this.view.path && revision === this.view.revision;
    if (!path) return;
    try {
      const disk = await this.read(path);
      if (current()) {
        this.publish({ conflict: disk.revision === revision ? null : disk });
      }
    } catch { if (current()) this.publish({ error: "파일의 현재 상태를 확인하지 못했습니다. 편집 내용은 유지됩니다." }); }
  }
  save(overwriteRevision?: string): Promise<boolean> {
    // Duplicate clicks and keyboard saves share one write; late replies cannot
    // reorder writes on disk. Edits made in flight remain dirty for the next save.
    if (this.writing) return this.writing;
    const { path, content, revision } = this.view;
    if (!path) return Promise.resolve(true);
    const document = this.document, edits = this.edits;
    this.saves++;
    this.publish({ saving: true, error: null });
    const task = (async () => {
      try {
        const saved = await this.write(path, content, overwriteRevision ?? revision);
        if (document !== this.document) return false;
        const outcome = saved.saveOutcome;
        const recovery = outcome?.recoveryDirectory ? ` 같은 폴더의 ${outcome.recoveryDirectory}에 보존된 파일을 확인해 주세요.` : "";
        if (outcome && outcome.state !== "applied") {
          const message = (outcome.state === "appliedWithConflict"
            ? "저장 중 외부 변경이 발견되었습니다. 교체된 파일과 현재 편집 내용을 보존했습니다."
            : "저장 반영 상태를 확정하지 못했습니다. 현재 편집 내용을 유지합니다.") + recovery;
          this.publish({ dirty: true });
          await this.inspect();
          if (document === this.document) this.publish({ error: message });
          return false;
        }
        this.inspecting++;
        this.publish({ revision: saved.revision, dirty: edits !== this.edits, conflict: null,
          error: outcome ? "파일은 반영했지만 일부 후처리를 완료하지 못했습니다." + recovery : null });
        return edits === this.edits;
      } catch (error) {
        if (document === this.document) {
          this.publish({ dirty: true, error: error instanceof Error ? error.message : "저장하지 못했습니다. 편집 내용은 유지됩니다." });
          await this.inspect();
        }
        return false;
      } finally { this.writing = null; this.publish({ saving: false }); }
    })();
    this.writing = task;
    return task;
  }
  unsaved = () => this.view.dirty || this.view.saving;
  settleBeforeQuit = async () => { if (this.writing) await this.writing; };
  saveBeforeQuit = async () => {
    if (this.writing) await this.writing;
    if (this.view.conflict) return false;
    return !this.view.dirty || await this.save();
  };
}
