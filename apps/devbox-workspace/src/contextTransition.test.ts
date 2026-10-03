import { describe, expect, it, vi } from "vitest";
import { createContextTransition } from "./contextTransition";
describe("context transition", () => {
  it("blocks native selection if a draft appears during flush", async () => {
    let dirty = false;
    const guard = createContextTransition(
      () => (dirty ? ["Source 초안"] : []),
      async () => {
        dirty = true;
      },
    );
    const select = vi.fn();
    await expect(guard(select)).rejects.toThrow("Source");
    expect(select).not.toHaveBeenCalled();
  });
  it("serializes native selection", async () => {
    let release!: () => void;
    const guard = createContextTransition(
      () => [],
      async () => {},
    );
    const first = guard(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    );
    await Promise.resolve();
    await expect(guard(async () => {})).rejects.toThrow("확인 중");
    release();
    await first;
  });
});

it("holds editor input until native context selection settles, then releases it on failure", async () => {
  const pending = vi.fn();
  const guard = createContextTransition(
    () => [],
    async () => {},
    pending,
  );
  const failure = new Error("native selection failed");
  await expect(
    guard(async () => {
      expect(pending).toHaveBeenLastCalledWith(true);
      throw failure;
    }),
  ).rejects.toBe(failure);
  expect(pending.mock.calls).toEqual([[true], [false]]);
});
