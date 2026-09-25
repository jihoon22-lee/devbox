/** One active read and one trailing refresh. Issuing intent immediately makes
 * older successes AND failures stale, even before the trailing read starts. */
export class MetadataRefresh<T> {
  private generation = 0;
  private enabled = false;
  private pending = false;
  private running: Promise<void> | null = null;

  constructor(
    private readonly read: () => Promise<T>,
    private readonly apply: (value: T) => void,
    private readonly fail: (error: unknown) => void,
  ) {}

  start() {
    this.enabled = true;
  }
  stop() {
    this.enabled = false;
    this.pending = false;
    this.generation += 1;
  }

  request = (): Promise<void> => {
    if (!this.enabled) return Promise.resolve();
    this.generation += 1;
    this.pending = true;
    if (!this.running) {
      this.running = Promise.resolve().then(async () => {
        try {
          while (this.enabled && this.pending) {
            this.pending = false;
            const issued = this.generation;
            try {
              const value = await this.read();
              if (this.enabled && issued === this.generation) this.apply(value);
            } catch (error) {
              if (this.enabled && issued === this.generation) this.fail(error);
            }
          }
        } finally {
          // No await between the loop's final check and clearing ownership:
          // a request cannot become stranded behind a completed promise.
          this.running = null;
        }
      });
    }
    return this.running;
  };
}
