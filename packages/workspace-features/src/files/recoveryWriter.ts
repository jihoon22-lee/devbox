import type { LoadedRecovery, RecoveryEntry } from "./api";
interface Ports {
  load(): Promise<LoadedRecovery>;
  save(entries: RecoveryEntry[], revision?: string): Promise<string | undefined>;
  discard(path: string, revision?: string): Promise<string | undefined>;
}
/** One context owns one writer. All revision updates and deletions share its queue. */
export class RecoveryWriter {
  private revision: string | undefined;
  private baseline: RecoveryEntry[] = [];
  private desired: RecoveryEntry[] = [];
  private generation = 0;
  private written = 0;
  private loaded = false;
  private tail: Promise<void> = Promise.resolve();
  constructor(private readonly ports: Ports) {}
  update(entries: RecoveryEntry[]) {
    this.desired = structuredClone(entries);
    this.generation++;
  }
  private enqueue(operation: () => Promise<void>): Promise<void> {
    const next = this.tail.catch(() => {}).then(operation);
    this.tail = next;
    return next;
  }
  private async load() {
    const state = await this.ports.load();
    this.revision = state.nativeRevision;
    this.baseline = state.entries;
    this.loaded = true;
  }
  flush(): Promise<void> {
    return this.enqueue(async () => {
      if (!this.loaded) await this.load();
      while (this.written !== this.generation) {
        const generation = this.generation;
        const entries = structuredClone(this.desired);
        if (entries.length) {
          try {
            this.revision = await this.ports.save(entries, this.revision);
          } catch (cause) {
            const code =
              cause instanceof Error
                ? `${cause.message} ${(cause as Error & { code?: string }).code ?? ""}`
                : String(cause);
            if (!/revision|stale/.test(code)) throw cause;
            const old = this.baseline;
            await this.load();
            for (const entry of entries) {
              const current = this.baseline.find((item) => item.path === entry.path);
              const previous = old.find((item) => item.path === entry.path);
              if (
                current &&
                JSON.stringify(current) !== JSON.stringify(previous) &&
                JSON.stringify(current) !== JSON.stringify(entry)
              ) {
                throw new Error("다른 편집기가 같은 문서의 복구 내용을 변경했습니다. 복구 내용을 다시 확인해 주세요.");
              }
            }
            this.revision = await this.ports.save(entries, this.revision);
          }
          for (const entry of entries)
            this.baseline = [...this.baseline.filter((item) => item.path !== entry.path), entry];
        }
        this.written = generation;
      }
    });
  }
  discard(path: string): Promise<void> {
    this.desired = this.desired.filter((entry) => entry.path !== path);
    this.generation++;
    return this.enqueue(async () => {
      if (!this.loaded) await this.load();
      this.revision = await this.ports.discard(path, this.revision);
      this.baseline = this.baseline.filter((entry) => entry.path !== path);
    });
  }
}
