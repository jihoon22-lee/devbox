import { beforeEach, expect, it, vi } from "vitest";
import { blockFailedMigrations, browserDocumentStorage, documentSession } from "./documentStorage";
import { LEGACY_KEYS } from "./migrate";
const sanitize = vi.hoisted(() => vi.fn(async (body: string) => body));
vi.mock("../requests/api", () => ({ sanitizePersistedJson: sanitize }));
import { initializeStudioDocuments } from "./initialize";
beforeEach(() => {
  localStorage.clear();
  blockFailedMigrations([]);
  sanitize.mockClear();
});
it("keeps failed migration sources and prevents later empty saves from shadowing them", async () => {
  localStorage.setItem(LEGACY_KEYS.collections, '{"version":2,"collections":[]}');
  const target = {
    load: vi.fn(async () => null),
    save: vi.fn(async () => {
      throw new Error("disk full");
    }),
  };
  const result = await initializeStudioDocuments(target, localStorage);
  expect(result.failed).toContain("collections");
  expect(localStorage.getItem(LEGACY_KEYS.collections)).not.toBeNull();
  await expect(documentSession("collections", target).load()).rejects.toHaveProperty("name", "store_unavailable");
  expect(target.save).toHaveBeenCalledTimes(1);
});
it("validates the sealed environment before any dependent document is copied", async () => {
  const memory = new Map<string, string>();
  const source = {
    getItem: (key: string) => memory.get(key) ?? null,
    setItem: (key: string, value: string) => {
      memory.set(key, value);
    },
    removeItem: (key: string) => {
      memory.delete(key);
    },
  } as Storage;
  source.setItem(
    LEGACY_KEYS.environments,
    JSON.stringify({
      version: 1,
      environments: [{ id: "e", name: "E", variables: [{ key: "token", value: "sealed-synthetic", secret: true }] }],
    }),
  );
  source.setItem(LEGACY_KEYS.collections, '{"version":2,"collections":[]}');
  sanitize.mockRejectedValueOnce(new Error("cannot unseal"));
  const target = browserDocumentStorage(localStorage);
  const result = await initializeStudioDocuments(target, source);
  expect(result.failed).toEqual(expect.arrayContaining(["collections", "environments"]));
  expect(await target.load("collections")).toBeNull();
  expect(source.getItem(LEGACY_KEYS.environments)).not.toBeNull();
});
