import { beforeEach, expect, it, vi } from "vitest";
import { migrateLocalDocuments, LEGACY_KEYS } from "./migrate";
import type { DocumentStorage } from "./documentStorage";
function memory() {
  const docs = new Map<string, { revision: number; body: string }>();
  const store: DocumentStorage = {
    async load(kind) {
      return docs.get(kind) ?? null;
    },
    async save(kind, body, expected) {
      if ((docs.get(kind)?.revision ?? null) !== expected) throw new Error("conflict");
      const revision = (expected ?? 0) + 1;
      docs.set(kind, { revision, body });
      return revision;
    },
  };
  return { store, docs };
}
beforeEach(() => localStorage.clear());
it("moves each legacy document with read-back verification and retires old keys", async () => {
  const { store, docs } = memory();
  localStorage.setItem(LEGACY_KEYS.collections, '{"version":2,"collections":[]}');
  localStorage.setItem("apip-collections", "[]");
  localStorage.setItem("apip-collections-v1-migrated", "2");
  const result = await migrateLocalDocuments(store, localStorage);
  expect(result.migrated).toEqual(["collections"]);
  expect(result.failed).toEqual([]);
  expect(docs.get("collections")?.body).toBe('{"version":2,"collections":[]}');
  expect(localStorage.getItem(LEGACY_KEYS.collections)).toBeNull();
  expect(localStorage.getItem("apip-collections")).toBeNull();
});
it("preserves source after a failed write or read-back and recovers on the next run", async () => {
  for (const failure of ["save", "read-back"]) {
    localStorage.setItem(LEGACY_KEYS.history, "{}");
    const { store } = memory();
    const load = store.load.bind(store);
    const save = store.save.bind(store);
    let reads = 0;
    store.load = async (kind) => {
      if (failure === "read-back" && ++reads === 2) throw new Error("read failed");
      return load(kind);
    };
    store.save = async (...args) => {
      if (failure === "save") throw new Error("disk full");
      return save(...args);
    };
    expect((await migrateLocalDocuments(store, localStorage)).failed).toEqual(["history"]);
    expect(localStorage.getItem(LEGACY_KEYS.history)).toBe("{}");
    store.load = load;
    store.save = save;
    expect((await migrateLocalDocuments(store, localStorage)).failed).toEqual([]);
    expect(localStorage.getItem(LEGACY_KEYS.history)).toBeNull();
  }
});
it("keeps existing native documents authoritative", async () => {
  const { store, docs } = memory();
  await store.save("history", "[]", null);
  localStorage.setItem(LEGACY_KEYS.history, "{}");
  expect((await migrateLocalDocuments(store, localStorage)).migrated).toEqual([]);
  expect(docs.get("history")?.body).toBe("[]");
  expect(localStorage.getItem(LEGACY_KEYS.history)).toBeNull();
});
it("contains storage access failures and checks source deletion", async () => {
  const { store } = memory();
  const denied = {
    getItem: () => {
      throw new Error("denied");
    },
    removeItem: vi.fn(),
  } as unknown as Storage;
  expect((await migrateLocalDocuments(store, denied)).failed).toHaveLength(5);
  localStorage.setItem(LEGACY_KEYS.workflows, "{}");
  const stuck = { getItem: localStorage.getItem.bind(localStorage), removeItem: () => {} } as unknown as Storage;
  expect((await migrateLocalDocuments(store, stuck)).failed).toEqual(["workflows"]);
  expect(localStorage.getItem(LEGACY_KEYS.workflows)).toBe("{}");
});

it("retires v1 data without importing its raw content or creating markers", async () => {
  const { store, docs } = memory();
  localStorage.setItem("apip-collections", '{"raw":"synthetic-secret"}');
  localStorage.setItem("apip-history", '["synthetic-secret"]');
  expect((await migrateLocalDocuments(store, localStorage)).failed).toEqual([]);
  expect(docs.size).toBe(0);
  expect(localStorage.getItem("apip-collections")).toBeNull();
  expect(localStorage.getItem("apip-history-v1-migrated")).toBeNull();
});
