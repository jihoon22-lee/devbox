import { beforeEach, describe, expect, it } from "vitest";
import type { RequestTemplate } from "../types";
import {
  emptyHistoryStore,
  HISTORY_V1_LS_KEY,
  HISTORY_V1_MARKER_KEY,
  HISTORY_V2_LS_KEY,
  migrateHistoryStorage,
  parseHistoryStore,
  REDACTED,
  sanitizeRequestForPersistence,
  saveHistoryStore,
  type HistoryStore,
} from "./persistence";

class RecordingStorage implements Storage {
  private readonly values = new Map<string, string>();
  readonly events: string[] = [];
  failLegacyRemoval = false;
  failV2Write = false;
  failMarkerWrite = false;

  get length(): number {
    return this.values.size;
  }

  clear(): void {
    this.values.clear();
  }

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  key(index: number): string | null {
    return [...this.values.keys()][index] ?? null;
  }

  removeItem(key: string): void {
    this.events.push(`remove:${key}`);
    if (key === HISTORY_V1_LS_KEY && this.failLegacyRemoval) return;
    this.values.delete(key);
  }

  setItem(key: string, value: string): void {
    this.events.push(`set:${key}`);
    if (key === HISTORY_V2_LS_KEY && this.failV2Write) throw new Error("v2 write failed");
    if (key === HISTORY_V1_MARKER_KEY && this.failMarkerWrite) throw new Error("marker write failed");
    this.values.set(key, value);
  }

  entries(): Array<[string, string]> {
    return [...this.values.entries()];
  }
}

function request(overrides: Partial<RequestTemplate> = {}): RequestTemplate {
  return {
    method: "POST",
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

function legacyHistory(rawRequest: RequestTemplate): string {
  return JSON.stringify([
    {
      id: "legacy-history-1",
      saved_at: 1000,
      request: rawRequest,
      status: 200,
    },
  ]);
}

function validHistoryStore(): HistoryStore {
  return {
    ...emptyHistoryStore(),
    history: [
      {
        id: "history-1",
        saved_at: 1000,
        request: sanitizeRequestForPersistence(request()),
        status: 200,
      },
    ],
  };
}

beforeEach(() => {
  localStorage.clear();
});

describe("History v1 fail-closed migration", () => {
  it("선택적 표시 이름이 있는 v2와 기존 이름 없는 v2를 모두 읽는다", () => {
    const named = validHistoryStore();
    named.history[0].name = "내 요청";
    const legacyV2 = validHistoryStore();

    expect(parseHistoryStore(JSON.stringify(named))?.history[0].name).toBe("내 요청");
    expect(parseHistoryStore(JSON.stringify(legacyV2))?.history[0].name).toBeUndefined();
    expect(parseHistoryStore(JSON.stringify({
      ...named,
      history: [{ ...named.history[0], name: 123 }],
    }))).toEqual({ version: 2, history: [] });
  });

  it("기존 v2 header는 enabled true로 올리고 duplicate/disabled/reference를 순서대로 보존한다", () => {
    const store = validHistoryStore();
    store.history[0].request.headers = [
      { key: "X-Trace", value: "one" },
      { key: "x-trace", value: "${TRACE_SECRET}", enabled: false },
    ];

    const parsed = parseHistoryStore(JSON.stringify(store));

    expect(parsed?.history[0].request.headers).toEqual([
      { key: "X-Trace", value: "one", enabled: true },
      { key: "x-trace", value: "${TRACE_SECRET}", enabled: false },
    ]);
  });

  it("기존 v2의 cookies 누락은 빈 배열로 올리고 cookie 순서·enabled를 보존한다", () => {
    const legacy = validHistoryStore();
    delete (legacy.history[0].request as Partial<RequestTemplate>).cookies;
    expect(parseHistoryStore(JSON.stringify(legacy))?.history[0].request.cookies).toEqual([]);

    const current = validHistoryStore();
    current.history[0].request.cookies = [
      { name: "session", value: "${SESSION}", enabled: true },
      { name: "disabled", value: REDACTED, enabled: false },
    ];
    expect(parseHistoryStore(JSON.stringify(current))?.history[0].request.cookies).toEqual([
      { name: "session", value: "${SESSION}", enabled: true },
      { name: "disabled", value: REDACTED, enabled: false },
    ]);
  });

  it("기존 v2의 multipart 누락은 빈 배열로 올린다", () => {
    const legacy = validHistoryStore();
    delete (legacy.history[0].request as Partial<RequestTemplate>).multipart;
    expect(parseHistoryStore(JSON.stringify(legacy))?.history[0].request.multipart).toEqual([]);
  });

  it("v2를 선기록한 뒤 v1을 삭제하고 marker를 기록하며 raw backup을 만들지 않는다", () => {
    const storage = new RecordingStorage();
    storage.setItem(HISTORY_V1_LS_KEY, legacyHistory(request()));
    storage.events.length = 0;

    const result = migrateHistoryStorage(storage);

    expect(result.failed).toBe(false);
    expect(result.migrated).toBe(true);
    expect(result.removedLegacyEntries).toBe(1);
    expect(result.store).toEqual(emptyHistoryStore());
    expect(storage.events).toEqual([
      `set:${HISTORY_V2_LS_KEY}`,
      `remove:${HISTORY_V1_LS_KEY}`,
      `set:${HISTORY_V1_MARKER_KEY}`,
    ]);
    expect(storage.getItem(HISTORY_V1_LS_KEY)).toBeNull();
    expect(storage.getItem(HISTORY_V1_MARKER_KEY)).toBe("2");
    expect(parseHistoryStore(storage.getItem(HISTORY_V2_LS_KEY))).toEqual(emptyHistoryStore());
    expect(storage.entries().some(([key]) => /backup|quarantine/i.test(key))).toBe(false);
  });

  it("v1 삭제 실패 시 marker를 만들지 않고 raw를 격리해 다음 실행에서 retry한다", () => {
    const storage = new RecordingStorage();
    const raw = legacyHistory(request({ url: "https://api.example.com/?token=legacy-secret" }));
    storage.setItem(HISTORY_V1_LS_KEY, raw);
    storage.failLegacyRemoval = true;

    const failed = migrateHistoryStorage(storage);

    expect(failed.failed).toBe(true);
    expect(failed.store).toEqual(emptyHistoryStore());
    expect(storage.getItem(HISTORY_V1_LS_KEY)).toBe(raw);
    expect(storage.getItem(HISTORY_V1_MARKER_KEY)).toBeNull();
    expect(storage.getItem(HISTORY_V2_LS_KEY)).not.toBeNull();
    expect(storage.entries().some(([key]) => /backup|quarantine/i.test(key))).toBe(false);

    storage.failLegacyRemoval = false;
    const retried = migrateHistoryStorage(storage);

    expect(retried.failed).toBe(false);
    expect(retried.migrated).toBe(true);
    expect(storage.getItem(HISTORY_V1_LS_KEY)).toBeNull();
    expect(storage.getItem(HISTORY_V1_MARKER_KEY)).toBe("2");
  });

  it("v1이 없을 때도 v2와 marker를 초기화하고 raw backup을 만들지 않는다", () => {
    const storage = new RecordingStorage();

    const result = migrateHistoryStorage(storage);

    expect(result.failed).toBe(false);
    expect(result.migrated).toBe(false);
    expect(result.store).toEqual(emptyHistoryStore());
    expect(storage.getItem(HISTORY_V1_LS_KEY)).toBeNull();
    expect(storage.getItem(HISTORY_V1_MARKER_KEY)).toBe("2");
    expect(storage.entries().some(([key]) => /backup|quarantine/i.test(key))).toBe(false);
  });

  it("v2 선기록 실패 시 raw를 삭제하거나 marker를 기록하지 않는다", () => {
    const storage = new RecordingStorage();
    const raw = legacyHistory(request({ auth: { kind: "bearer", username: "", password: "", token: "raw-secret", api_key: "", api_value: "" } }));
    storage.setItem(HISTORY_V1_LS_KEY, raw);
    storage.failV2Write = true;

    const failed = migrateHistoryStorage(storage);

    expect(failed.failed).toBe(true);
    expect(storage.getItem(HISTORY_V1_LS_KEY)).toBe(raw);
    expect(storage.getItem(HISTORY_V2_LS_KEY)).toBeNull();
    expect(storage.getItem(HISTORY_V1_MARKER_KEY)).toBeNull();
  });

  it("marker 기록 실패는 완료로 간주하지 않고 다음 실행에서 marker를 재시도한다", () => {
    const storage = new RecordingStorage();
    storage.setItem(HISTORY_V1_LS_KEY, legacyHistory(request()));
    storage.failMarkerWrite = true;

    const failed = migrateHistoryStorage(storage);

    expect(failed.failed).toBe(true);
    expect(storage.getItem(HISTORY_V1_LS_KEY)).toBeNull();
    expect(storage.getItem(HISTORY_V2_LS_KEY)).not.toBeNull();
    expect(storage.getItem(HISTORY_V1_MARKER_KEY)).toBeNull();

    storage.failMarkerWrite = false;
    const retried = migrateHistoryStorage(storage);
    expect(retried.failed).toBe(false);
    expect(storage.getItem(HISTORY_V1_MARKER_KEY)).toBe("2");
  });
});

describe("History persistence guard", () => {
  it("sanitizer가 거부하면 v2를 쓰지 않고 기존 값을 보존한다", async () => {
    const storage = new RecordingStorage();
    const existing = JSON.stringify(emptyHistoryStore());
    storage.setItem(HISTORY_V2_LS_KEY, existing);
    storage.events.length = 0;

    await expect(
      saveHistoryStore(validHistoryStore(), async () => {
        throw new Error("secret review failed");
      }, storage),
    ).rejects.toThrow("secret review failed");

    expect(storage.getItem(HISTORY_V2_LS_KEY)).toBe(existing);
    expect(storage.events).toEqual([]);
    expect(storage.entries().some(([key]) => /backup|quarantine/i.test(key))).toBe(false);
  });

  it("sanitizer가 v2 형식이 아닌 결과를 반환하면 persistence를 거부한다", async () => {
    const storage = new RecordingStorage();

    await expect(saveHistoryStore(validHistoryStore(), async () => JSON.stringify({ version: 1 }), storage)).rejects.toThrow(
      "안전한 History 형식이 아닙니다",
    );

    expect(storage.getItem(HISTORY_V2_LS_KEY)).toBeNull();
  });

  it("정상 sanitizer 결과만 v2에 기록한다", async () => {
    const storage = new RecordingStorage();
    const seen: string[] = [];

    await saveHistoryStore(validHistoryStore(), async (serialized) => {
      seen.push(serialized);
      return serialized;
    }, storage);

    expect(seen).toHaveLength(1);
    expect(parseHistoryStore(storage.getItem(HISTORY_V2_LS_KEY))).not.toBeNull();
    expect(storage.entries().some(([key]) => /backup|quarantine/i.test(key))).toBe(false);
  });
});

describe("request persistence sanitizer", () => {
  it("JSON/form/url/auth의 직접 입력 secret을 마스킹하고 reference는 보존한다", () => {
    const raw = request({
      url: "https://url-user:url-password@example.com/x?token=url-token&api_key=url-api-key&name=alice",
      headers: [
        { key: "Authorization", value: "Bearer header-secret" },
        { key: "Cookie", value: "session=cookie-secret" },
        { key: "X-Trace", value: "${TRACE_ID}" },
      ],
      cookies: [
        { name: "session", value: "direct-cookie" },
        { name: "token", value: "${COOKIE_TOKEN}" },
        { name: "mixed", value: "prefix-${COOKIE_TOKEN}" },
        { name: "disabled", value: "disabled-secret", enabled: false },
      ],
      params: [
        { key: "access_token", value: "param-token" },
        { key: "q", value: "safe-query" },
      ],
      body_kind: "json",
      body: JSON.stringify({
        password: "json-password",
        token: "${BODY_TOKEN}",
        nested: { apiKey: "json-api-key" },
        safe: "safe-body",
      }),
      auth: {
        kind: "basic",
        username: "auth-user",
        password: "auth-password",
        token: "auth-token",
        api_key: "X-API-Key",
        api_value: "auth-api-value",
      },
    });

    const safe = sanitizeRequestForPersistence(raw);

    expect(safe.requiresSecretReview).toBe(true);
    expect(safe.url).not.toContain("url-user");
    expect(safe.url).not.toContain("url-password");
    expect(safe.url).not.toContain("url-token");
    expect(decodeURIComponent(safe.url)).toContain("token=[REDACTED]");
    expect(safe.headers).toEqual([
      { key: "Authorization", value: REDACTED, enabled: true },
      { key: "Cookie", value: REDACTED, enabled: true },
      { key: "X-Trace", value: "${TRACE_ID}", enabled: true },
    ]);
    expect(safe.cookies).toEqual([
      { name: "session", value: REDACTED, enabled: true },
      { name: "token", value: "${COOKIE_TOKEN}", enabled: true },
      { name: "mixed", value: REDACTED, enabled: true },
      { name: "disabled", value: REDACTED, enabled: false },
    ]);
    expect(JSON.stringify(safe)).not.toContain("direct-cookie");
    expect(JSON.stringify(safe)).not.toContain("disabled-secret");
    expect(safe.params).toEqual([
      { key: "access_token", value: REDACTED },
      { key: "q", value: "safe-query" },
    ]);
    expect(safe.body).toContain('"password":"[REDACTED]"');
    expect(safe.body).toContain('"token":"${BODY_TOKEN}"');
    expect(safe.body).toContain('"apiKey":"[REDACTED]"');
    expect(safe.body).toContain('"safe":"safe-body"');
    expect(safe.body).not.toContain("json-password");
    expect(safe.body).not.toContain("json-api-key");
    expect(safe.auth).toEqual({
      kind: "basic",
      username: REDACTED,
      password: REDACTED,
      token: REDACTED,
      api_key: "X-API-Key",
      api_value: REDACTED,
    });
  });

  it("form body의 token/password를 마스킹하고 reference와 일반 field를 보존한다", () => {
    const safe = sanitizeRequestForPersistence(
      request({
        body_kind: "form",
        body: "token=form-token\npassword=form-password\nname=alice\napi_key=${FORM_KEY}",
      }),
    );

    expect(safe.body).toBe("token=[REDACTED]\npassword=[REDACTED]\nname=alice\napi_key=${FORM_KEY}");
    expect(safe.body).not.toContain("form-token");
    expect(safe.body).not.toContain("form-password");
    expect(safe.requiresSecretReview).toBe(true);
  });

  it("known token pattern도 민감한 field 이름 없이 마스킹한다", () => {
    const githubToken = "ghp_1234567890abcdef";
    const safe = sanitizeRequestForPersistence(
      request({ body_kind: "raw", body: `trace=${githubToken}` }),
    );

    expect(safe.body).toBe("trace=[REDACTED]");
    expect(safe.body).not.toContain(githubToken);
    expect(safe.requiresSecretReview).toBe(true);
  });

  it("multipart 파일 경로·stale body를 제거하고 민감 text는 직접값만 마스킹한다", () => {
    const safe = sanitizeRequestForPersistence(request({
      body_kind: "multipart",
      body: "raw-file-backup",
      multipart: [
        {
          kind: "file",
          name: "upload",
          value: "raw-file-bytes",
          file_path: "C:\\private\\artifact.zip",
          file_name: "C:\\private\\artifact.zip",
          content_type: "application/zip",
          enabled: false,
        },
        {
          kind: "text",
          name: "token",
          value: "direct-token",
          file_path: "",
          file_name: "",
          content_type: "text/plain",
          enabled: true,
        },
        {
          kind: "text",
          name: "token",
          value: "${UPLOAD_TOKEN}",
          file_path: "",
          file_name: "",
          content_type: "",
          enabled: true,
        },
      ],
    }));

    expect(safe.body).toBe("");
    expect(safe.multipart[0]).toMatchObject({
      file_path: "",
      file_name: "artifact.zip",
      enabled: false,
    });
    expect(safe.multipart[1].value).toBe(REDACTED);
    expect(safe.multipart[2].value).toBe("${UPLOAD_TOKEN}");
    expect(JSON.stringify(safe)).not.toContain("C:\\private");
    expect(JSON.stringify(safe)).not.toContain("raw-file-backup");
    expect(JSON.stringify(safe)).not.toContain("direct-token");
    expect(JSON.stringify(safe)).not.toContain("raw-file-bytes");
    expect(safe.requiresSecretReview).toBe(true);
  });

  it("template URL과 깨진 JSON에서도 민감한 query·userinfo·field를 fail-closed 마스킹한다", () => {
    const safe = sanitizeRequestForPersistence(
      request({
        url: "https://direct-user:direct-pass@${HOST}/x?access_token=direct-token&q=safe",
        body_kind: "json",
        body: '{"password":"broken-secret", trailing}',
      }),
    );

    expect(safe.url).not.toContain("direct-user");
    expect(safe.url).not.toContain("direct-pass");
    expect(safe.url).not.toContain("direct-token");
    expect(safe.body).not.toContain("broken-secret");
    expect(safe.requiresSecretReview).toBe(true);
  });
});
