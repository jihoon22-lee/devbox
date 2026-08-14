import { beforeEach, describe, expect, it } from "vitest";
import {
  addEnvironment,
  applyToRequest,
  applyVariables,
  loadStore,
  removeEnvironment,
  setVariable,
  emptyStore,
} from "./environments";

beforeEach(() => {
  localStorage.clear();
});

describe("variable substitution", () => {
  it("기본 치환", () => {
    const vars = new Map([["base", "https://api.example.com"], ["token", "abc"]]);
    expect(applyVariables("{{base}}/v1?token={{token}}", vars)).toBe("https://api.example.com/v1?token=abc");
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
    store = setVariable(store, "e-1", "token", "t");
    expect(store.environments[0].variables).toEqual([
      { key: "base", value: "https://dev2" },
      { key: "token", value: "t" },
    ]);
    store = removeEnvironment(store, "e-1");
    expect(store.environments).toEqual([]);
  });
});
