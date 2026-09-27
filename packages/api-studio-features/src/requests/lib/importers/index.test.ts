import { expect, it } from "vitest";
import { detectFormat, parseImport, toImportPreview, ImportError } from "./index";
import postman from "./fixtures/postman-v21.json";
import insomnia from "./fixtures/insomnia-v4.json";
import { serializeCollectionExport, serializeEnvironmentExport } from "../transfer";
import { emptyRequest } from "./model";
import { sanitizeRequestForPersistence } from "../persistence";
it("detects only recognized formats", () => {
  expect(detectFormat("x.har", '{"log":{"entries":[]}}')).toBe("har");
  expect(detectFormat("x.json", JSON.stringify(postman))).toBe("postman");
  expect(detectFormat("x.json", JSON.stringify(insomnia))).toBe("insomnia");
  expect(detectFormat("x.json", serializeCollectionExport({ version: 2, collections: [] }))).toBe("devbox");
  expect(detectFormat("a.bru", "meta {\n}\n")).toBe("bruno");
  expect(detectFormat("x.json", "{}")).toBeNull();
});
it("projects parse failures to one import error", () => {
  expect(() => parseImport("postman", [{ relativePath: "broken.json", text: "{not json" }])).toThrow(
    new ImportError("가져올 수 없는 형식입니다."),
  );
});
it("preserves secret review and explicit secret variables when importing Devbox exports", () => {
  const request = {
    ...sanitizeRequestForPersistence({ ...emptyRequest(), url: "https://x.test" }),
    requiresSecretReview: true,
  };
  const collection = serializeCollectionExport({
    version: 2,
    collections: [{ id: "old", name: "Review", folder: "", saved_at: 1, requiresSecretReview: true, request }],
  });
  const environment = serializeEnvironmentExport({
    version: 1,
    environments: [
      { id: "env", name: "Demo", variables: [{ key: "opaque", value: "sealed-not-exported", secret: true }] },
    ],
  });
  const bundle = parseImport("devbox", [
    { relativePath: "requests.json", text: collection },
    { relativePath: "environment.json", text: environment },
  ]);
  let n = 0;
  const preview = toImportPreview(bundle, () => `new-${++n}`);
  expect(preview.collections.collections[0].requiresSecretReview).toBe(true);
  expect(preview.collections.collections[0].request.requiresSecretReview).toBe(true);
  expect(preview.environments.environments[0].variables[0]).toEqual({
    key: "opaque",
    reference: "${opaque}",
    secret: true,
  });
  expect(JSON.stringify(preview)).not.toContain("sealed-not-exported");
});
