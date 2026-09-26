import { MemoryDocuments as RecordingStorage } from "../../storage/testDocuments";
import { beforeEach, describe, expect, it } from "vitest";
import type { RequestTemplate } from "../types";
import {
  addEntry,
  duplicateEntry,
  emptyStore,
  foldersOf,
  migrateCollections,
  removeEntry,
  renameEntry,
  saveStore,
} from "./collections";
import { REDACTED, type PersistenceSanitizer } from "./persistence";

function request(overrides: Partial<RequestTemplate> = {}): RequestTemplate {
  return {
    method: "GET",
    url: "https://api.example.com/x",
    headers: [],
    cookies: [],
    multipart: [],
    params: [],
    body_kind: "none",
    body: "",
    auth: null,
    timeout_ms: 30000,
    ...overrides,
  };
}

const identitySanitizer: PersistenceSanitizer = async (serialized) => serialized;

beforeEach(() => {
  localStorage.clear();
});

describe("collections v2 store", () => {
  it("빈 v2 스토어가 기본이다", () => {
    expect(emptyStore()).toEqual({ version: 2, collections: [] });
  });

  it("sanitized v2를 async saveStore로 저장하고 read-back한다", async () => {
    const storage = new RecordingStorage();
    const store = addEntry(
      emptyStore(),
      {
        name: "내 요청",
        folder: "api",
        request: request({
          headers: [{ key: "Authorization", value: "Bearer direct-secret" }],
        }),
      },
      1000,
      () => "c-1",
    );

    const saved = await saveStore(store, identitySanitizer, storage);

    expect(saved.version).toBe(2);
    expect(saved.collections).toHaveLength(1);
    expect(saved.collections[0].name).toBe("내 요청");
    expect(saved.collections[0].folder).toBe("api");
    expect(saved.collections[0].requiresSecretReview).toBe(true);
    expect(saved.collections[0].request.headers[0].value).toBe(REDACTED);
    expect(storage.body("collections")).not.toContain("direct-secret");
  });

  it("duplicate header의 순서, enabled와 secret reference를 Collection round-trip에서 보존한다", async () => {
    const storage = new RecordingStorage();
    const store = addEntry(
      emptyStore(),
      {
        name: "duplicate headers",
        folder: "api",
        request: request({
          headers: [
            { key: "X-Trace", value: "one", enabled: true },
            { key: "x-trace", value: "${TRACE_SECRET}", enabled: false },
          ],
        }),
      },
      1000,
      () => "c-headers",
    );

    const saved = await saveStore(store, identitySanitizer, storage);

    expect(saved.collections[0].request.headers).toEqual([
      { key: "X-Trace", value: "one", enabled: true },
      { key: "x-trace", value: "${TRACE_SECRET}", enabled: false },
    ]);
  });

  it("cookie 직접 값은 마스킹하고 reference·순서·enabled는 Collection에 보존한다", async () => {
    const storage = new RecordingStorage();
    const store = addEntry(
      emptyStore(),
      {
        name: "cookies",
        folder: "api",
        request: request({
          cookies: [
            { name: "session", value: "direct-cookie", enabled: true },
            { name: "token", value: "${COOKIE_TOKEN}", enabled: true },
            { name: "disabled", value: "disabled-secret", enabled: false },
          ],
        }),
      },
      1000,
      () => "c-cookies",
    );

    const saved = await saveStore(store, identitySanitizer, storage);

    expect(saved.collections[0].request.cookies).toEqual([
      { name: "session", value: REDACTED, enabled: true },
      { name: "token", value: "${COOKIE_TOKEN}", enabled: true },
      { name: "disabled", value: REDACTED, enabled: false },
    ]);
    expect(storage.body("collections")).not.toContain("direct-cookie");
    expect(storage.body("collections")).not.toContain("disabled-secret");
  });

  it("multipart file path를 제거하고 복제에서도 safe metadata만 보존한다", async () => {
    const storage = new RecordingStorage();
    const store = addEntry(
      emptyStore(),
      {
        name: "multipart",
        folder: "api",
        request: request({
          body_kind: "multipart",
          multipart: [
            {
              kind: "file",
              name: "upload",
              value: "raw-bytes",
              file_path: "C:\\private\\artifact.zip",
              file_name: "artifact.zip",
              content_type: "application/zip",
              enabled: true,
            },
          ],
        }),
      },
      1000,
      () => "c-multipart",
    );
    const saved = await saveStore(store, identitySanitizer, storage);
    const duplicate = duplicateEntry(saved, "c-multipart", 2000, () => "c-copy");

    expect(saved.collections[0].request.multipart[0]).toMatchObject({
      file_path: "",
      file_name: "artifact.zip",
      value: "",
    });
    expect(duplicate.collections[0].request.multipart[0]).toEqual(saved.collections[0].request.multipart[0]);
    expect(JSON.stringify(duplicate)).not.toContain("C:\\private");
    expect(JSON.stringify(duplicate)).not.toContain("raw-bytes");
  });

  it("multipart가 없는 v2 collection을 빈 배열로 올린다", async () => {
    const storage = new RecordingStorage();
    const legacyRequest = request();
    delete (legacyRequest as Partial<RequestTemplate>).multipart;
    storage.seed(
      "collections",
      JSON.stringify({
        version: 2,
        collections: [
          {
            id: "legacy-1",
            name: "legacy",
            folder: "",
            saved_at: 1,
            requiresSecretReview: false,
            request: { ...legacyRequest, requiresSecretReview: false },
          },
        ],
      }),
    );

    const migration = await migrateCollections(identitySanitizer, storage);

    expect(migration.failed).toBe(false);
    expect(migration.store.collections[0].request.multipart).toEqual([]);
  });

  it("saveStore sanitizer 실패 시 기존 v2를 보존하고 raw backup을 만들지 않는다", async () => {
    const storage = new RecordingStorage();
    const existing = JSON.stringify(emptyStore());
    storage.seed("collections", existing);
    storage.events.length = 0;

    await expect(
      saveStore(
        emptyStore(),
        async () => {
          throw new Error("secret review failed");
        },
        storage,
      ),
    ).rejects.toThrow("secret review failed");

    expect(storage.body("collections")).toBe(existing);
    expect(storage.events).toEqual([]);
    expect(storage.entries().some(([key]) => /backup|quarantine/i.test(key))).toBe(false);
  });

  it("sanitizer가 오래된 결과를 반환하면 commit guard 전에 저장하지 않는다", async () => {
    const storage = new RecordingStorage();
    const existing = JSON.stringify(emptyStore());
    storage.seed("collections", existing);
    storage.events.length = 0;
    let releaseSanitizer!: () => void;
    const pending = new Promise<void>((resolve) => {
      releaseSanitizer = resolve;
    });
    const saving = saveStore(
      addEntry(emptyStore(), { name: "stale", folder: "", request: request() }, 1, () => "c-stale"),
      async (serialized) => {
        await pending;
        return serialized;
      },
      storage,
      () => false,
    );

    releaseSanitizer();
    await expect(saving).rejects.toThrow("오래되어 저장하지 않았습니다");
    expect(storage.body("collections")).toBe(existing);
    expect(storage.events).toEqual([]);
  });

  it("이름이 비면 template URL을 사용한다", () => {
    const store = addEntry(emptyStore(), { name: "  ", folder: "", request: request() }, 1, () => "c-1");
    expect(store.collections[0].name).toBe("https://api.example.com/x");
  });

  it("제거와 폴더 목록은 v2 entry identity를 보존한다", () => {
    let store = emptyStore();
    store = addEntry(store, { name: "a", folder: "dev", request: request() }, 1, () => "c-1");
    store = addEntry(store, { name: "b", folder: "dev", request: request() }, 2, () => "c-2");
    store = addEntry(store, { name: "c", folder: "prod", request: request() }, 3, () => "c-3");

    expect(foldersOf(store)).toEqual(["dev", "prod"]);
    expect(removeEntry(store, "c-1").collections.map((entry) => entry.id)).toEqual(["c-3", "c-2"]);
  });

  it("복제와 이름 변경은 exact entry의 마스킹 request만 사용한다", () => {
    const secret = "direct-collection-secret";
    const source = addEntry(
      emptyStore(),
      {
        name: "원본",
        folder: "dev",
        request: request({
          headers: [{ key: "Authorization", value: secret }],
          cookies: [{ name: "session", value: secret }],
        }),
      },
      1,
      () => "c-1",
    );

    const duplicated = duplicateEntry(source, "c-1", 2, () => "c-copy");
    expect(duplicated.collections[0]).toMatchObject({ id: "c-copy", name: "원본 복사본", saved_at: 2 });
    expect(duplicated.collections[0].request).not.toBe(source.collections[0].request);
    expect(duplicated.collections[0].request.cookies).not.toBe(source.collections[0].request.cookies);
    expect(JSON.stringify(duplicated)).not.toContain(secret);
    expect(duplicated.collections[0].request.headers[0].value).toBe(REDACTED);
    expect(duplicated.collections[0].request.cookies[0].value).toBe(REDACTED);

    const renamed = renameEntry(duplicated, "c-copy", "  새\n이름  ");
    expect(renamed.collections[0].name).toBe("새 이름");
    expect(renamed.collections[1].name).toBe("원본");
  });
});
