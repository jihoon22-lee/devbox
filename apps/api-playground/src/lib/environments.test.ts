import { beforeEach, describe, expect, it } from "vitest";
import {
  addEnvironment,
  applyToRequest,
  applyVariables,
  loadStore,
  removeEnvironment,
  saveStore,
  setVariable,
  emptyStore,
} from "./environments";
import type { RequestTemplate } from "../types";

beforeEach(() => {
  localStorage.clear();
});

class EnvironmentStorage implements Storage {
  private readonly values = new Map<string, string>();
  failWrite = false;

  get length(): number { return this.values.size; }
  clear(): void { this.values.clear(); }
  getItem(key: string): string | null { return this.values.get(key) ?? null; }
  key(index: number): string | null { return [...this.values.keys()][index] ?? null; }
  removeItem(key: string): void { this.values.delete(key); }
  setItem(key: string, value: string): void {
    if (this.failWrite) throw new Error("write failed");
    this.values.set(key, value);
  }
}

describe("variable substitution", () => {
  it("기본 치환", () => {
    const vars = new Map([["base", "https://api.example.com"], ["token", "abc"]]);
    expect(applyVariables("{{base}}/v1?token={{token}}", vars)).toBe("https://api.example.com/v1?token=abc");
  });

  it("${NAME} reference와 기존 {{NAME}} reference를 함께 치환한다", () => {
    const vars = new Map([
      ["BASE_URL", "https://api.example.com"],
      ["VERSION", "v2"],
      ["TOKEN", "abc"],
    ]);
    expect(applyVariables("${BASE_URL}/${VERSION}?token={{TOKEN}}", vars)).toBe(
      "https://api.example.com/v2?token=abc",
    );
  });

  it("알 수 없는 변수는 그대로", () => {
    const vars = new Map([["a", "1"]]);
    expect(applyVariables("{{a}}-{{missing}}", vars)).toBe("1-{{missing}}");
  });

  it("공백 무시 치환", () => {
    const vars = new Map([["a", "x"]]);
    expect(applyVariables("{{ a }}", vars)).toBe("x");
  });

  it("요청 template 불변 (원본은 그대로)", () => {
    const req = { url: "{{base}}/x", body: "hello {{name}}", headers: [{ key: "A", value: "{{t}}" }] };
    const vars = new Map([["base", "https://e"], ["name", "world"], ["t", "v"]]);
    const out = applyToRequest(req, vars);
    expect(out.url).toBe("https://e/x");
    expect(out.body).toBe("hello world");
    expect(out.headers[0].value).toBe("v");
    // 원본 template은 바뀌지 않는다
    expect(req.url).toBe("{{base}}/x");
    expect(req.body).toBe("hello {{name}}");
  });

  it("URL·params·headers·body와 모든 auth field를 치환하고 원본 auth를 보존한다", () => {
    const req: RequestTemplate = {
      method: "POST",
      url: "${BASE_URL}/${VERSION}",
      headers: [{ key: "Authorization", value: "Bearer ${TOKEN}", enabled: false }],
      cookies: [{ name: "session", value: "${TOKEN}", enabled: true }],
      multipart: [{
        kind: "text",
        name: "token",
        value: "${TOKEN}",
        file_path: "",
        file_name: "",
        content_type: "text/plain",
        enabled: true,
      }, {
        kind: "file",
        name: "upload",
        value: "",
        file_path: "C:\\${TOKEN}\\artifact.zip",
        file_name: "artifact.zip",
        content_type: "application/zip",
        enabled: true,
      }],
      params: [{ key: "tenant", value: "${TENANT}" }],
      body_kind: "json",
      body: '{"token":"${TOKEN}"}',
      auth: {
        kind: "basic",
        username: "${USERNAME}",
        password: "${PASSWORD}",
        token: "${TOKEN}",
        api_key: "${API_HEADER}",
        api_value: "${API_VALUE}",
      },
      timeout_ms: 10000,
    };
    const variables = new Map([
      ["BASE_URL", "https://api.example.com"],
      ["VERSION", "v2"],
      ["TOKEN", "token-value"],
      ["TENANT", "tenant-value"],
      ["USERNAME", "user-value"],
      ["PASSWORD", "password-value"],
      ["API_HEADER", "X-API-Key"],
      ["API_VALUE", "api-value"],
    ]);

    const out = applyToRequest(req, variables);

    expect(out.url).toBe("https://api.example.com/v2");
    expect(out.headers[0].value).toBe("Bearer token-value");
    expect(out.headers[0].enabled).toBe(false);
    expect(out.multipart[0].value).toBe("token-value");
    expect(out.multipart[1].file_path).toBe("C:\\${TOKEN}\\artifact.zip");
    expect(out.cookies).toEqual([
      { name: "session", value: "token-value", enabled: true },
    ]);
    expect(out.params[0].value).toBe("tenant-value");
    expect(out.body).toBe('{"token":"token-value"}');
    expect(out.auth).toEqual({
      kind: "basic",
      username: "user-value",
      password: "password-value",
      token: "token-value",
      api_key: "X-API-Key",
      api_value: "api-value",
    });
    expect(req.url).toBe("${BASE_URL}/${VERSION}");
    expect(req.auth?.password).toBe("${PASSWORD}");
    expect(req.cookies[0].value).toBe("${TOKEN}");
  });

  it("multipart에서는 stale body를 해석하지 않고 비운다", () => {
    const req: RequestTemplate = {
      method: "POST",
      url: "https://example.test/upload",
      headers: [],
      cookies: [],
      multipart: [],
      params: [],
      body_kind: "multipart",
      body: "${BROKEN}",
      auth: null,
      timeout_ms: 10_000,
    };

    const out = applyToRequest(req, new Map([["BROKEN", "must-not-be-used"]]));
    expect(out.body).toBe("");
    expect(req.body).toBe("${BROKEN}");
  });

  it("알 수 없는 ${NAME} reference는 그대로 보존한다", () => {
    expect(applyVariables("${KNOWN}/${MISSING}", new Map([["KNOWN", "ok"]]))).toBe("ok/${MISSING}");
  });
});

describe("environment store", () => {
  it("빈 스토어 기본", () => {
    expect(loadStore()).toEqual(emptyStore());
  });

  it("손상은 빈 스토어", () => {
    localStorage.setItem("apip-environments", "{bad");
    expect(loadStore()).toEqual(emptyStore());
  });

  it("추가·변수 설정·제거", () => {
    let store = emptyStore();
    store = addEnvironment(store, "dev", () => "e-1");
    store = setVariable(store, "e-1", "base", "https://dev");
    store = setVariable(store, "e-1", "base", "https://dev2");
    store = setVariable(store, "e-1", "token", "t", true);
    expect(store.environments[0].variables).toEqual([
      { key: "base", value: "https://dev2", secret: false },
      { key: "token", value: "t", secret: true },
    ]);
    store = removeEnvironment(store, "e-1");
    expect(store.environments).toEqual([]);
  });

  it("환경 저장은 read-back된 allowlist를 반환한다", () => {
    const storage = new EnvironmentStorage();
    const store = addEnvironment(emptyStore(), "dev", () => "e-1");
    store.environments[0].variables.push({ key: "BASE_URL", value: "https://dev", secret: false });
    const saved = saveStore(store, storage);

    expect(saved).toEqual(store);
    expect(loadStore()).toEqual(emptyStore());
    expect(JSON.parse(storage.getItem("apip-environments") ?? "null")).toEqual(store);
  });

  it("환경 write/read-back 실패 시 기존 저장 값을 유지한다", () => {
    const storage = new EnvironmentStorage();
    const original = addEnvironment(emptyStore(), "original", () => "e-original");
    saveStore(original, storage);
    const originalRaw = storage.getItem("apip-environments");
    storage.failWrite = true;

    expect(() => saveStore(addEnvironment(original, "new", () => "e-new"), storage)).toThrow("write failed");
    expect(storage.getItem("apip-environments")).toBe(originalRaw);
  });
});
