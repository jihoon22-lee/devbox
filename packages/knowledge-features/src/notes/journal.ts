import type { NoteView } from "./noteDocument";
import { browserTimers, type Timers } from "./timers";

export const JOURNAL_DELAY_MS = 1000;
export type JournalIssue = "journal_unavailable" | "journal_limit";
export interface JournalTarget {
  snapshot(): NoteView;
  subscribe(listener: () => void): () => void;
}
export interface JournalApi {
  save(path: string, content: string, baseRevision: string): Promise<void>;
  clear(path: string): Promise<void>;
}
interface QueueState { tail: Promise<void>; owned: Set<string> }
// The document outlives a lazy Notes view. Do not reorder old work when the
// view remounts (including StrictMode); roots change only on the next startup.
const queues = new WeakMap<JournalTarget, QueueState>();

export class NoteJournal {
  private timer: number | null = null;
  private lastPath: string | null;
  private lastSource = -1;
  private lastRevision = "";
  private disposed = false;
  private readonly queue: QueueState;
  private readonly stop: () => void;

  constructor(
    private readonly target: JournalTarget,
    private readonly api: JournalApi,
    private readonly onError: (code: JournalIssue) => void = () => {},
    private readonly timers: Timers = browserTimers,
    private readonly delayMs = JOURNAL_DELAY_MS,
  ) {
    this.queue = queues.get(target) ?? { tail: Promise.resolve(), owned: new Set() };
    queues.set(target, this.queue);
    this.lastPath = target.snapshot().path;
    this.stop = target.subscribe(() => this.onChange());
    this.onChange();
  }

  adoptRestored(path: string) {
    this.enqueue(async () => { this.queue.owned.add(path); });
  }

  async settled(): Promise<void> {
    if (this.timer !== null) {
      this.cancel();
      this.record(this.target.snapshot());
    }
    let tail: Promise<void>;
    do { tail = this.queue.tail; await tail; } while (tail !== this.queue.tail);
  }

  dispose() {
    this.disposed = true;
    this.cancel();
    this.stop();
  }

  private enqueue(action: () => Promise<void>) {
    this.queue.tail = this.queue.tail.then(action).catch(error => {
      if (this.disposed) return;
      const code = error instanceof Error ? error.name : error;
      this.onError(code === "journal_limit" ? "journal_limit" : "journal_unavailable");
    });
  }

  private clear(path: string) {
    this.enqueue(async () => {
      if (!this.queue.owned.has(path)) return;
      await this.api.clear(path);
      this.queue.owned.delete(path);
    });
  }

  private record(view: NoteView) {
    if (!view.dirty || !view.path) return;
    const {path, content, revision} = view;
    this.enqueue(async () => {
      await this.api.save(path, content, revision);
      this.queue.owned.add(path);
    });
  }

  private onChange() {
    const view = this.target.snapshot();
    if (view.path !== this.lastPath) {
      this.cancel();
      if (this.lastPath !== null) this.clear(this.lastPath);
      this.lastPath = view.path;
    }
    if (!view.path || !view.dirty) {
      this.cancel();
      if (view.path && !view.saving) this.clear(view.path);
      return;
    }
    if (view.sourceVersion === this.lastSource && view.revision === this.lastRevision) return;
    this.lastSource = view.sourceVersion;
    this.lastRevision = view.revision;
    this.cancel();
    this.timer = this.timers.set(() => {
      this.timer = null;
      this.record(this.target.snapshot());
    }, this.delayMs);
  }

  private cancel() {
    if (this.timer !== null) {
      this.timers.clear(this.timer);
      this.timer = null;
    }
  }
}
