import { expect, it } from "vitest";
import { emptyRequest } from "./importers";
import { sanitizeRequestForPersistence, toRequestTemplate } from "./persistence";
import { addEntry, emptyStore, parseStore } from "./collections";
import { serializeCollectionExport, parseCollectionExport } from "./transfer";
import { serializeFileCollection, parseFileCollection } from "./fileCollection";
it("preserves TLS metadata across documents, JSON and files without changing old requests", () => {
  expect(sanitizeRequestForPersistence(emptyRequest()).tls).toBeUndefined();
  const tls = { credentialId: "a".repeat(32), verify: false };
  const store = addEntry(
    emptyStore(),
    { name: "TLS", folder: "", request: { ...emptyRequest(), url: "https://api.test", tls } },
    1,
    () => "tls",
  );
  const restored = parseStore(JSON.stringify(store))!;
  expect(restored.collections[0].request.tls).toEqual(tls);
  expect(parseCollectionExport(serializeCollectionExport(store))?.collections[0].request.tls).toEqual(tls);
  expect(parseFileCollection(serializeFileCollection("TLS", store)).requests[0].request.tls).toEqual(tls);
  const editable = toRequestTemplate(restored.collections[0].request);
  editable.tls!.verify = true;
  expect(restored.collections[0].request.tls?.verify).toBe(false);
});
it("clears invalid credential references and keeps verification enabled by default", () => {
  expect(
    sanitizeRequestForPersistence({ ...emptyRequest(), tls: { credentialId: "../private", verify: false } }).tls,
  ).toEqual({ credentialId: null, verify: false });
  expect(sanitizeRequestForPersistence({ ...emptyRequest(), tls: { credentialId: null } as never }).tls).toEqual({
    credentialId: null,
    verify: true,
  });
});
