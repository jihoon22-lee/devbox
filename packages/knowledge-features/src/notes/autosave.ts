import type { NoteView } from "./noteDocument";
import { browserTimers, type Timers } from "./timers";

export const AUTOSAVE_DELAY_MS = 1500;
export const AUTOSAVE_KEY = "devbox.knowledge.notes.autosave";
export function readAutosavePreference(storage?: Pick<Storage, "getItem">): boolean {
  try { return (storage ?? localStorage).getItem(AUTOSAVE_KEY) !== "off"; } catch { return true; }
}
export function writeAutosavePreference(enabled: boolean, storage?: Pick<Storage, "setItem">): void {
  try { (storage ?? localStorage).setItem(AUTOSAVE_KEY, enabled ? "on" : "off"); } catch { /* preference only */ }
}
export interface AutosaveTarget {
  snapshot(): NoteView;
  subscribe(listener: () => void): () => void;
  save(): Promise<boolean>;
}
interface SaveState { paused: boolean; attempted: number }
const states = new WeakMap<AutosaveTarget, SaveState>();

export class NoteAutosave {
  private timer: number | null = null;
  private lastSource = -1;
  private wasSaving = false;
  private disposed = false;
  private readonly state: SaveState;
  private readonly stop: () => void;
  constructor(
    private readonly target: AutosaveTarget,
    private enabled: boolean,
    private readonly onSaved: () => void = () => {},
    private readonly timers: Timers = browserTimers,
    private readonly delayMs = AUTOSAVE_DELAY_MS,
  ) {
    this.state = states.get(target) ?? { paused: false, attempted: -1 };
    states.set(target, this.state);
    this.stop = target.subscribe(() => this.onChange());
    this.onChange();
  }
  setEnabled(enabled: boolean) {
    if (enabled && !this.enabled) this.state.attempted = -1;
    this.enabled = enabled;
    this.cancel();
    if (enabled) this.schedule();
  }
  pause() { this.state.paused = true; this.cancel(); }
  async flush(): Promise<boolean> {
    this.cancel();
    let view = this.target.snapshot();
    if (!this.enabled || this.state.paused || view.conflict) return !view.dirty && !view.saving;
    if (view.saving) {
      const path = view.path;
      await this.target.save();
      view = this.target.snapshot();
      if (view.path !== path) return false;
    }
    if (!this.eligible()) return !view.dirty && !view.saving;
    return this.runSave();
  }
  dispose() { this.disposed = true; this.cancel(); this.stop(); }
  private eligible(): boolean {
    const view = this.target.snapshot();
    return !this.disposed && this.enabled && !this.state.paused && !!view.path && view.dirty && !view.saving && view.conflict === null;
  }
  private async runSave(): Promise<boolean> {
    this.state.attempted = this.target.snapshot().sourceVersion;
    try {
      const saved = await this.target.save();
      if (saved && !this.disposed) this.onSaved();
      return saved;
    } catch { return false; } // NoteDocument owns the error and retains the buffer.
  }
  private onChange() {
    const view = this.target.snapshot();
    const changed = view.sourceVersion !== this.lastSource;
    const finished = this.wasSaving && !view.saving;
    this.lastSource = view.sourceVersion;
    this.wasSaving = view.saving;
    if (!view.dirty) { this.state.paused = false; this.cancel(); return; }
    if (changed || finished) this.schedule();
  }
  private schedule() {
    this.cancel();
    if (!this.eligible() || this.state.attempted === this.target.snapshot().sourceVersion) return;
    this.timer = this.timers.set(() => {
      this.timer = null;
      if (this.eligible()) void this.runSave();
    }, this.delayMs);
  }
  private cancel() {
    if (this.timer !== null) { this.timers.clear(this.timer); this.timer = null; }
  }
}
