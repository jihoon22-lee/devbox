import { describe, expect, it } from "vitest";
import type { RequestTemplate } from "../types";
import { addEntry, emptyStore as emptyCollectionStore } from "./collections";
import { emptyStore as emptyEnvironmentStore, type EnvironmentStore } from "./environments";
import { sanitizeRequestForPersistence } from "./persistence";
import {
  COLLECTION_EXPORT_SCHEMA,
  ENVIRONMENT_EXPORT_SCHEMA,
  MAX_EXPORTED_COLLECTIONS,
  MAX_EXPORTED_ENVIRONMENTS,
  decodeTransferBytes,
  mergeImportedCollections,
  mergeImportedEnvironments,
  parseCollectionExport,
  parseEnvironmentExport,
  serializeCollectionExport,
  serializeEnvironmentExport,
} from "./transfer";

function request(overrides: Partial<RequestTemplate> = {}): RequestTemplate {
  return {
    method: "GET",
    url: "https://api.example.com/users?token=direct-token",
    headers: [{ key: "Authorization", value: "Bearer direct-secret", enabled: true }],
    cookies: [],
    multipart: [],
    params: [],
    body_kind: "none",
    body: "",
    auth: null,
    timeout_ms: 10_000,
    ...overrides,
  };
}

function persistedRequest(overrides: Partial<RequestTemplate> = {}) {
  return sanitizeRequestForPersistence(request(overrides));
}

describe("API Playground transfer documents", () => {
  it("exports a versioned collection document without direct credentials", () => {
    const store = addEntry(emptyCollectionStore(), {
      name: "Users",
      folder: "dev",
      request: request({ headers: [{ key: "Authorization", value: "Bearer direct-secret", enabled: true }] }),
    }, 1, () => "c-1");
    const raw = serializeCollectionExport(store);
    expect(raw).toContain(COLLECTION_EXPORT_SCHEMA);
    expect(raw).not.toContain("direct-secret");
    expect(raw).not.toContain("direct-token");
    expect(parseCollectionExport(raw)?.collections[0].request.headers[0].value).toBe("[REDACTED]");
  });

  it("rejects unknown collection fields and invalid schema versions", () => {
    const base = JSON.stringify({ schema: COLLECTION_EXPORT_SCHEMA, schema_version: 1, collections: [] });
    expect(parseCollectionExport(base)).not.toBeNull();
    expect(parseCollectionExport(JSON.stringify({ schema: COLLECTION_EXPORT_SCHEMA, schema_version: 2, collections: [] }))).toBeNull();
    expect(parseCollectionExport(JSON.stringify({ schema: COLLECTION_EXPORT_SCHEMA, schema_version: 1, collections: [], extra: true }))).toBeNull();
  });

  it("exports only references for secret environment variables", () => {
    const store: EnvironmentStore = {
      ...emptyEnvironmentStore(),
      environments: [{
        id: "env-1",
        name: "dev",
        variables: [
          { key: "BASE_URL", value: "https://localhost", secret: false },
          { key: "API_TOKEN", value: "sealed-secret-blob", secret: true },
          { key: "ordinary", value: "ghp_1234567890abcdef", secret: false },
        ],
      }],
    };
    const raw = serializeEnvironmentExport(store);
    expect(raw).toContain(ENVIRONMENT_EXPORT_SCHEMA);
    expect(raw).toContain("https://localhost");
    expect(raw).not.toContain("sealed-secret-blob");
    expect(raw).not.toContain("ghp_1234567890abcdef");
    const parsed = parseEnvironmentExport(raw);
    expect(parsed?.environments[0].variables).toEqual([
      { key: "BASE_URL", reference: "${BASE_URL}", secret: false, value: "https://localhost" },
      { key: "API_TOKEN", reference: "${API_TOKEN}", secret: true },
      { key: "ordinary", reference: "${ordinary}", secret: true },
    ]);
  });

  it("imports by appending and leaves imported secret values unconfigured", () => {
    const collections = parseCollectionExport(JSON.stringify({
      schema: COLLECTION_EXPORT_SCHEMA,
      schema_version: 1,
      collections: [],
    }));
    expect(collections).not.toBeNull();
    const environments = parseEnvironmentExport(JSON.stringify({
      schema: ENVIRONMENT_EXPORT_SCHEMA,
      schema_version: 1,
      environments: [{
        id: "incoming",
        name: "prod",
        variables: [{ key: "TOKEN", reference: "${TOKEN}", secret: true }],
      }],
    }));
    expect(environments).not.toBeNull();
    const merged = mergeImportedEnvironments(emptyEnvironmentStore(), environments!, () => "imported");
    expect(merged?.environments[0].variables[0]).toEqual({ key: "TOKEN", value: "", secret: true });
    expect(mergeImportedCollections(emptyCollectionStore(), collections!, () => "collection")?.collections).toEqual([]);
  });

  it("rejects a hand-written environment document that lies about secret metadata", () => {
    const raw = JSON.stringify({
      schema: ENVIRONMENT_EXPORT_SCHEMA,
      schema_version: 1,
      environments: [{
        id: "incoming",
        name: "prod",
        variables: [{ key: "API_TOKEN", reference: "${API_TOKEN}", secret: false, value: "direct-secret" }],
      }],
    });
    expect(parseEnvironmentExport(raw)).toBeNull();
  });

  it("bounds and redacts exported collection/environment metadata without control characters", () => {
    const collectionStore = addEntry(emptyCollectionStore(), {
      name: `line\n${"ghp_1234567890abcdef"}`,
      folder: `folder\u0000${"sk_1234567890abcdef"}`,
      request: request(),
    }, 1, () => `id\u0007${"ghp_1234567890abcdef"}`);
    const collectionRaw = serializeCollectionExport(collectionStore);
    expect(collectionRaw).not.toContain("ghp_1234567890abcdef");
    expect(collectionRaw).not.toContain("sk_1234567890abcdef");
    expect(collectionRaw).not.toMatch(/[\u0000-\u001f\u007f-\u009f\u2028\u2029]/u);
    const collection = parseCollectionExport(collectionRaw);
    expect(collection?.collections).toHaveLength(1);
    expect(collection?.collections[0].name).toContain("[REDACTED]");
    expect(collection?.collections[0].folder).toContain("[REDACTED]");

    const environmentRaw = serializeEnvironmentExport({
      ...emptyEnvironmentStore(),
      environments: [{
        id: `env\u0000${"ghp_1234567890abcdef"}`,
        name: `dev\n${"sk-1234567890abcdef"}`,
        variables: [],
      }],
    });
    expect(environmentRaw).not.toContain("ghp_1234567890abcdef");
    expect(environmentRaw).not.toContain("sk_1234567890abcdef");
    expect(environmentRaw).not.toMatch(/[\u0000-\u001f\u007f-\u009f\u2028\u2029]/u);
    const environment = parseEnvironmentExport(environmentRaw);
    expect(environment?.environments[0].id).toContain("[REDACTED]");
    expect(environment?.environments[0].name).toContain("[REDACTED]");
  });

  it("sanitizes every nested browser export field and revalidates the generated document", () => {
    const contaminated = {
      ...emptyCollectionStore(),
      collections: [{
        id: "collection-1",
        name: "request",
        folder: "dev",
        saved_at: 1,
        requiresSecretReview: false,
        request: persistedRequest({
          method: "GET\u0000POST",
          url: "https://example.test/path/ghp_1234567890abcdef",
          headers: [{ key: "X-Trace\u0000", value: "safe\u0001", enabled: true }],
          cookies: [{ name: "session\u0002", value: "cookie-value", enabled: true }],
          multipart: [{
            kind: "file",
            name: "upload\u0003",
            value: "generated-file-body",
            file_path: "C:\\private\\artifact.zip",
            file_name: "C:\\private\\artifact.zip",
            content_type: "application/zip\u0004",
            enabled: true,
          }],
          params: [{ key: "trace\u0005", value: "param\u0006" }],
          body_kind: "graphql",
          body: "generated GraphQL body must not persist",
          auth: {
            kind: "bearer\u0007",
            username: "",
            password: "",
            token: "prefix-${TOKEN}",
            api_key: "X-API-Key\u0008",
            api_value: "prefix-${API_VALUE}",
          },
          graphql: {
            query: "query Viewer { viewer { id } }",
            variables: "{}",
            operation_name: "Viewer\u0009",
          },
        }),
      }],
    };

    const raw = serializeCollectionExport(contaminated);
    expect(raw).not.toMatch(/[\u0000-\u001f\u007f-\u009f\u2028\u2029]/u);
    expect(raw).not.toContain("ghp_1234567890abcdef");
    expect(raw).not.toContain("C:\\private");
    expect(raw).not.toContain("generated GraphQL body");
    const parsed = parseCollectionExport(raw);
    expect(parsed?.collections[0].request.body).toBe("");
    expect(parsed?.collections[0].request.multipart[0].file_path).toBe("");
    expect(parsed?.collections[0].request.multipart[0].file_name).toBe("artifact.zip");
    expect(parsed?.collections[0].request.graphql?.operation_name).toBe("Viewer");
  });

  it("does not silently drop unsafe environment export variables", () => {
    expect(() => serializeEnvironmentExport({
      ...emptyEnvironmentStore(),
      environments: [{
        id: "env-1",
        name: "dev",
        variables: [{ key: "bad\u0000key", value: "value", secret: false }],
      }],
    })).toThrow("metadata");
  });

  it("rejects control-bearing or credential-shaped imported metadata", () => {
    const collection = {
      schema: COLLECTION_EXPORT_SCHEMA,
      schema_version: 1,
      collections: [{
        id: "c-1",
        name: "bad\u0000name",
        folder: "folder",
        saved_at: 1,
        request: persistedRequest(),
        requiresSecretReview: false,
      }],
    };
    expect(parseCollectionExport(JSON.stringify(collection))).toBeNull();
    expect(parseCollectionExport(JSON.stringify({
      ...collection,
      collections: [{ ...collection.collections[0], name: "ghp_1234567890abcdef" }],
    }))).toBeNull();

    const environment = {
      schema: ENVIRONMENT_EXPORT_SCHEMA,
      schema_version: 1,
      environments: [{ id: "env-1", name: "prod\u2028east", variables: [] }],
    };
    expect(parseEnvironmentExport(JSON.stringify(environment))).toBeNull();
    expect(parseEnvironmentExport(JSON.stringify({
      ...environment,
      environments: [{ ...environment.environments[0], name: "sk-1234567890abcdef" }],
    }))).toBeNull();
  });

  it("does not partially merge when the combined count exceeds the bound", () => {
    const currentCollections = {
      ...emptyCollectionStore(),
      collections: Array.from({ length: MAX_EXPORTED_COLLECTIONS }, (_, index) => ({
        id: `existing-${index}`,
        name: `entry-${index}`,
        folder: "",
        saved_at: index,
        request: persistedRequest(),
        requiresSecretReview: false,
      })),
    };
    const importedCollections = parseCollectionExport(JSON.stringify({
      schema: COLLECTION_EXPORT_SCHEMA,
      schema_version: 1,
      collections: [{
        id: "incoming",
        name: "incoming",
        folder: "",
        saved_at: 1,
        request: persistedRequest(),
        requiresSecretReview: false,
      }],
    }));
    expect(importedCollections).not.toBeNull();
    expect(mergeImportedCollections(currentCollections, importedCollections!, () => "new-id")).toBeNull();
    expect(currentCollections.collections).toHaveLength(MAX_EXPORTED_COLLECTIONS);

    const currentEnvironments = {
      ...emptyEnvironmentStore(),
      environments: Array.from({ length: MAX_EXPORTED_ENVIRONMENTS }, (_, index) => ({
        id: `existing-${index}`,
        name: `env-${index}`,
        variables: [],
      })),
    };
    const importedEnvironments = parseEnvironmentExport(JSON.stringify({
      schema: ENVIRONMENT_EXPORT_SCHEMA,
      schema_version: 1,
      environments: [{ id: "incoming", name: "incoming", variables: [] }],
    }));
    expect(importedEnvironments).not.toBeNull();
    expect(mergeImportedEnvironments(currentEnvironments, importedEnvironments!, () => "new-id")).toBeNull();
    expect(currentEnvironments.environments).toHaveLength(MAX_EXPORTED_ENVIRONMENTS);
  });

  it("does not partially merge when ID generation cannot produce unique IDs", () => {
    const importedCollections = parseCollectionExport(JSON.stringify({
      schema: COLLECTION_EXPORT_SCHEMA,
      schema_version: 1,
      collections: [
        { id: "incoming-1", name: "one", folder: "", saved_at: 1, request: persistedRequest(), requiresSecretReview: false },
        { id: "incoming-2", name: "two", folder: "", saved_at: 2, request: persistedRequest(), requiresSecretReview: false },
      ],
    }));
    expect(importedCollections).not.toBeNull();
    expect(mergeImportedCollections(emptyCollectionStore(), importedCollections!, () => "same-id")).toBeNull();

    const importedEnvironments = parseEnvironmentExport(JSON.stringify({
      schema: ENVIRONMENT_EXPORT_SCHEMA,
      schema_version: 1,
      environments: [
        { id: "incoming-1", name: "one", variables: [] },
        { id: "incoming-2", name: "two", variables: [] },
      ],
    }));
    expect(importedEnvironments).not.toBeNull();
    expect(mergeImportedEnvironments(emptyEnvironmentStore(), importedEnvironments!, () => "same-id")).toBeNull();
  });

  it("decodes browser transfer bytes strictly as UTF-8", () => {
    expect(decodeTransferBytes(new TextEncoder().encode('{"ok":true}'))).toBe('{"ok":true}');
    expect(() => decodeTransferBytes(new Uint8Array([0xc3, 0x28]))).toThrow("UTF-8");
  });
});
