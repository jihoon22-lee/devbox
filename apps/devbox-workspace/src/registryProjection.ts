import type { Registry } from "./RegistryGate";
/** Native revision orders the projection, including overlapping snapshot reads. */
export class RegistryProjection {
  private snapshot: Registry | null = null;
  constructor(
    private readonly load: () => Promise<Registry>,
    private readonly changed: (snapshot: Registry) => void,
  ) {}
  current(): Registry | null {
    return this.snapshot;
  }
  publish(incoming: Registry): Registry {
    if (!this.snapshot || incoming.revision >= this.snapshot.revision) {
      this.snapshot = incoming;
      this.changed(incoming);
    }
    return this.snapshot;
  }
  async refresh(): Promise<Registry> {
    return this.publish(await this.load());
  }
}
