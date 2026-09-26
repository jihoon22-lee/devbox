import { beforeEach, expect, it } from "vitest";
import { browserDocumentStorage, documentSession } from "./documentStorage";
beforeEach(() => localStorage.clear());
it("creates, reloads and rejects a stale document revision", async () => {
  const store = browserDocumentStorage(localStorage);
  expect(await store.load("collections")).toBeNull();
  expect(await store.save("collections", "{}", null)).toBe(1);
  await expect(store.save("collections", "[]", null)).rejects.toHaveProperty("name", "store_revision_conflict");
  expect(await store.load("collections")).toEqual({ revision: 1, body: "{}" });
  expect(await store.save("collections", "[]", 1)).toBe(2);
});
it("does not let overlapping writes silently reuse the newer revision", async () => {
  const session = documentSession("history", browserDocumentStorage(localStorage));
  await session.load();
  const first = session.save("{}");
  const second = session.save("[]");
  await expect(first).resolves.toBe(1);
  await expect(second).rejects.toHaveProperty("name", "store_revision_conflict");
  expect(await session.load()).toEqual({ revision: 1, body: "{}" });
});
it("rejects malformed and oversized documents without replacing valid content", async () => {
  const store = browserDocumentStorage(localStorage);
  await store.save("history", "{}", null);
  await expect(store.save("history", "not-json", 1)).rejects.toHaveProperty("name", "store_document_invalid");
  await expect(store.save("history", '"' + "x".repeat(16 * 1024 * 1024) + '"', 1)).rejects.toHaveProperty(
    "name",
    "store_document_too_large",
  );
  expect(await store.load("history")).toEqual({ revision: 1, body: "{}" });
});

it("restores the previous preview body when its revision write fails", async () => {
  const values = new Map<string, string>();
  let fail = false;
  const backing = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      if (fail && key.endsWith(".revision")) throw new Error("quota");
      values.set(key, value);
    },
    removeItem: (key: string) => {
      values.delete(key);
    },
  } as Storage;
  const store = browserDocumentStorage(backing);
  await store.save("history", "{}", null);
  fail = true;
  await expect(store.save("history", "[]", 1)).rejects.toThrow();
  expect(await store.load("history")).toEqual({ revision: 1, body: "{}" });
});
it("a slow load cannot regress a completed write", async () => {
  let pending!: (value: { revision: number; body: string }) => void;
  let calls = 0;
  let retained = { revision: 1, body: "{}" };
  const target = {
    load: async () =>
      ++calls === 2
        ? new Promise<typeof retained>((resolve) => {
            pending = resolve;
          })
        : retained,
    save: async (_kind: unknown, body: string) => {
      retained = { revision: 2, body };
      return 2;
    },
  };
  const session = documentSession("history", target);
  await session.load();
  const slow = session.load();
  await session.save("[]");
  pending({ revision: 1, body: "{}" });
  expect(await slow).toEqual({ revision: 2, body: "[]" });
  expect(session.expectedRevision).toBe(2);
});
