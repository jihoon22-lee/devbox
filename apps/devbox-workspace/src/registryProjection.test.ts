import { expect, it, vi } from "vitest";
import { RegistryProjection } from "./registryProjection";
import type { Registry } from "./RegistryGate";
const snapshot = (revision: number): Registry => ({ revision, projects: [], worktrees: [] });
it("ignores an older native response after a newer snapshot", () => {
  const changed = vi.fn();
  const projection = new RegistryProjection(async () => snapshot(11), changed);
  projection.publish(snapshot(12));
  expect(projection.publish(snapshot(11)).revision).toBe(12);
  expect(changed).toHaveBeenCalledTimes(1);
});
it("returns the canonical snapshot when overlapping refresh completes late", async () => {
  let release!: (value: Registry) => void;
  const projection = new RegistryProjection(
    () =>
      new Promise((resolve) => {
        release = resolve;
      }),
    vi.fn(),
  );
  const read = projection.refresh();
  projection.publish(snapshot(12));
  release(snapshot(11));
  expect((await read).revision).toBe(12);
});
