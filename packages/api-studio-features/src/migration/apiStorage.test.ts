import { describe, expect, it, vi } from "vitest";
import { API_LEGACY_KEYS, normalizeLegacyApiStorage, type ExportOwner, type LegacyStorage } from "./apiStorage";
import { sanitizeRequestForPersistence } from "../requests/lib/persistence";

function empty(): LegacyStorage { return Object.fromEntries(API_LEGACY_KEYS.map((key) => [key, null])) as LegacyStorage; }
function request() {
  return { method: "POST", url: "https://fixture.test/hooks", headers: [
    { key: "X-Order", value: "one", enabled: true }, { key: "X-Order", value: "two", enabled: false },
  ], cookies: [], multipart: [], params: [], body_kind: "raw", body: "fixture", auth: null, timeout_ms: 30_000 };
}
function owner(): ExportOwner {
  return { prepareEnvironment: vi.fn(async (raw) => ({ serialized: raw, missingSecrets: 0 })), sanitize: vi.fn(async (raw) => raw) };
}
describe("legacy API copied storage", () => {
  it("preserves duplicate header order and flags, uses the owner sanitizer, and never mutates the input", async () => {
    const raw = empty();
    const safe = sanitizeRequestForPersistence(request());
    raw["apip-collections-v2"] = JSON.stringify({ version: 2, collections: [{ id: "c1", name: "fixture", folder: "", saved_at: 1, request: safe, requiresSecretReview: false }] });
    raw["apip-history-v2"] = JSON.stringify({ version: 2, history: [{ id: "h1", saved_at: 2, request: safe, status: 200 }] });
    const before = structuredClone(raw);
    const native = owner();
    const result = await normalizeLegacyApiStorage(Object.freeze(raw), native);
    expect(result.collections?.collections[0].request.headers).toEqual(request().headers);
    expect(result.history?.history[0].status).toBe(200);
    expect(native.sanitize).toHaveBeenCalled();
    expect(raw).toEqual(before);
    expect(await normalizeLegacyApiStorage(raw, owner())).toEqual(result);
  });
  it("keeps the v0.7 exclusion of unsafe v1 History without returning or sanitizing its raw contents", async () => {
    const raw = empty(); raw["apip-history"] = "synthetic-raw-secret-history";
    const native = owner();
    const result = await normalizeLegacyApiStorage(raw, native);
    expect(result.history).toBeNull();
    expect(JSON.stringify(result)).not.toContain("synthetic-raw-secret-history");
    expect(result.notices).toEqual([{ store: "history", code: "legacy-history-excluded", count: 1 }]);
    expect(native.sanitize).not.toHaveBeenCalled();
    expect(raw["apip-history"]).toBe("synthetic-raw-secret-history");
  });
  it("rejects corrupt and future current schemas instead of replacing them with an older or empty store", async () => {
    for (const corrupt of ["invalid", JSON.stringify({ version: 99, collections: [] })]) {
      const raw = empty(); raw["apip-collections-v2"] = corrupt;
      raw["apip-collections"] = JSON.stringify({ version: 1, collections: [] });
      await expect(normalizeLegacyApiStorage(raw, owner())).rejects.toThrow("legacy_api_storage_invalid");
      expect(raw["apip-collections-v2"]).toBe(corrupt);
    }
    const raw = empty(); raw["apip-history-v2"] = JSON.stringify({ version: 99, history: [] });
    await expect(normalizeLegacyApiStorage(raw, owner())).rejects.toThrow("legacy_api_storage_invalid");
  });
  it("keeps reconnect placeholders and refuses v1 request migration when a source secret cannot be checked", async () => {
    const raw = empty();
    raw["apip-environments"] = JSON.stringify({ version: 1, environments: [{ id: "e1", name: "fixture", variables: [{ key: "TOKEN", value: "opaque-ciphertext-fixture", secret: true }] }] });
    const safeEnv = JSON.stringify({ version: 1, environments: [{ id: "e1", name: "fixture", variables: [{ key: "TOKEN", value: "", secret: true }] }] });
    const native = owner(); native.prepareEnvironment = async () => ({ serialized: safeEnv, missingSecrets: 1 });
    const result = await normalizeLegacyApiStorage(raw, native);
    expect(result.environments?.environments[0].variables[0]).toEqual({ key: "TOKEN", value: "", secret: true });
    expect(result.notices[0].code).toBe("secret-reconnect-required");
    raw["apip-collections"] = JSON.stringify({ version: 1, collections: [] });
    await expect(normalizeLegacyApiStorage(raw, native)).rejects.toThrow("legacy_api_secret_reconnect_required");
  });
  it("does not publish an export if the native redaction step fails", async () => {
    const raw = empty();
    raw["apip-collections"] = JSON.stringify({ version: 1, collections: [{ id: "c1", name: "fixture", folder: "", saved_at: 1, request: request() }] });
    const native = owner(); native.sanitize = async () => { throw new Error("owner unavailable"); };
    await expect(normalizeLegacyApiStorage(raw, native)).rejects.toThrow("legacy_api_storage_invalid");
  });
});
