import { afterEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: native.invoke }));

import {
  currentDescription,
  fixtureDescription,
  invalidateDescription,
  publishDescription,
  resetDescriptionCache,
} from "./api";

afterEach(() => {
  resetDescriptionCache();
  native.invoke.mockReset();
});

describe("description cache", () => {
  it("shares one describe call across concurrent and later calls", async () => {
    native.invoke.mockResolvedValue(fixtureDescription("knowledge"));
    const [first, second] = await Promise.all([currentDescription("knowledge"), currentDescription("knowledge")]);
    await currentDescription("knowledge");
    expect(first).toBe(second);
    expect(native.invoke).toHaveBeenCalledTimes(1);
  });

  it("uses the published description until it is invalidated", async () => {
    const published = { ...fixtureDescription("workspace"), context: null };
    publishDescription(published);
    expect(await currentDescription("workspace")).toBe(published);
    expect(native.invoke).not.toHaveBeenCalled();
    invalidateDescription("workspace");
    native.invoke.mockResolvedValue(fixtureDescription("workspace"));
    await currentDescription("workspace");
    expect(native.invoke).toHaveBeenCalledTimes(1);
  });

  it("does not keep a failed describe", async () => {
    native.invoke.mockRejectedValueOnce(new Error("offline")).mockResolvedValue(fixtureDescription("api-studio"));
    await expect(currentDescription("api-studio")).rejects.toThrow();
    await expect(currentDescription("api-studio")).resolves.toMatchObject({ product: { id: "api-studio" } });
    expect(native.invoke).toHaveBeenCalledTimes(2);
  });

  it("keeps products apart", async () => {
    publishDescription(fixtureDescription("knowledge"));
    native.invoke.mockResolvedValue(fixtureDescription("api-studio"));
    expect((await currentDescription("api-studio")).product.id).toBe("api-studio");
  });
});

it("preserves a new context when an older request fails", async () => {
  let reject!: (error: Error) => void;
  native.invoke.mockReturnValue(
    new Promise((_, no) => {
      reject = no;
    }),
  );
  const pending = currentDescription("workspace");
  const updated = fixtureDescription("workspace");
  publishDescription(updated);
  reject(new Error("old request"));
  await expect(pending).rejects.toThrow();
  expect(await currentDescription("workspace")).toBe(updated);
});
