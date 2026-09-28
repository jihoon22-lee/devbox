import { expect, it } from "vitest";
import { applyImportDocuments } from "./importTransaction";
import { browserDocumentStorage } from "../../storage/documentStorage";
it("undo restores both documents only while their saved revisions match", async () => {
  localStorage.clear();
  const storage = browserDocumentStorage();
  const undo = await applyImportDocuments(storage, [
    { kind: "collections", before: null, emptyBody: "{}", body: '{"new":true}' },
  ]);
  await undo();
  expect((await storage.load("collections"))?.body).toBe("{}");
  const before = await storage.load("collections");
  const staleUndo = await applyImportDocuments(storage, [
    { kind: "collections", before, emptyBody: "{}", body: '{"new":true}' },
  ]);
  await storage.save("collections", '{"later":true}', (await storage.load("collections"))!.revision);
  await expect(staleUndo()).rejects.toThrow("그 사이 바뀐 내용이 있어 되돌리지 않았습니다.");
  expect((await storage.load("collections"))?.body).toBe('{"later":true}');
});
it("a second document failure conditionally compensates the first write", async () => {
  localStorage.clear();
  const base = browserDocumentStorage();
  const storage = {
    load: base.load,
    save: async (kind: Parameters<typeof base.save>[0], body: string, revision: number | null) => {
      if (kind === "environments") throw new Error("unavailable");
      return base.save(kind, body, revision);
    },
  };
  await expect(
    applyImportDocuments(storage, [
      { kind: "collections", before: null, emptyBody: "{}", body: '{"new":true}' },
      { kind: "environments", before: null, emptyBody: "{}", body: '{"new":true}' },
    ]),
  ).rejects.toThrow();
  expect((await base.load("collections"))?.body).toBe("{}");
});
