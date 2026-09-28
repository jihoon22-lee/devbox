import { describe, expect, it } from "vitest";
import { toImportPreview, emptyRequest } from "./index";
import {
  MAX_EXPORTED_COLLECTIONS,
  parseCollectionExport,
  parseEnvironmentExport,
  serializeCollectionExport,
} from "../transfer";
describe("import preview", () => {
  it("redacts auth and blanks secret environment values in the existing export shape", () => {
    let n = 0;
    const preview = toImportPreview(
      {
        requests: [
          {
            name: "Me",
            folder: "Users",
            request: {
              ...emptyRequest(),
              url: "https://x.test/me",
              auth: {
                kind: "bearer",
                username: "",
                password: "",
                token: "ghp_abcdefghijklmnopqrstuvwxyz0123",
                api_key: "",
                api_value: "",
              },
            },
          },
        ],
        environments: [
          {
            name: "dev",
            variables: [
              { key: "baseUrl", value: "https://x.test" },
              { key: "apiToken", value: "abc" },
            ],
          },
        ],
        warnings: [],
      },
      () => `id-${++n}`,
    );
    expect(preview.collections.collections[0].request.auth?.token).toBe("[REDACTED]");
    expect(preview.collections.collections[0].requiresSecretReview).toBe(true);
    expect(preview.environments.environments[0].variables).toEqual([
      { key: "baseUrl", reference: "${baseUrl}", value: "https://x.test", secret: false },
      { key: "apiToken", reference: "${apiToken}", secret: true },
    ]);
    expect(parseCollectionExport(serializeCollectionExport(preview.collections))).not.toBeNull();
    expect(parseEnvironmentExport(JSON.stringify(preview.environments))).not.toBeNull();
  });
  it("bounds preview requests and reports truncation", () => {
    let n = 0;
    const preview = toImportPreview(
      {
        requests: Array.from({ length: MAX_EXPORTED_COLLECTIONS + 1 }, () => ({
          name: "x",
          folder: "",
          request: { ...emptyRequest(), url: "https://x.test" },
        })),
        environments: [],
        warnings: [],
      },
      () => `id-${++n}`,
    );
    expect(preview.collections.collections).toHaveLength(MAX_EXPORTED_COLLECTIONS);
    expect(preview.warnings.join(" ")).toContain(String(MAX_EXPORTED_COLLECTIONS));
  });
});
