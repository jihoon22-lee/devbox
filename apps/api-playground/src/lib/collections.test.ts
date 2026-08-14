import { beforeEach, describe, expect, it } from "vitest";
import type { ApiRequest } from "../types";
import { addEntry, foldersOf, loadStore, removeEntry, saveStore, emptyStore } from "./collections";

function req(url = "https://api.example.com/x"): ApiRequest {
  return {
    method: "GET",
    url,
    headers: [],
    params: [],
    body_kind: "none",
    body: "",
    auth: null,
    timeout_ms: 30000,
  };
}

beforeEach(() => {
  localStorage.clear();
});

describe("collections store", () => {
  it("빈 스토어가 기본이다", () => {
    expect(loadStore()).toEqual(emptyStore());
  });

  it("손상된 저장소는 빈 스토어로", () => {
    localStorage.setItem("apip-collections", "{bad json");
    expect(loadStore()).toEqual(emptyStore());
  });

  it("추가·조회 왕복", () => {
    let store = emptyStore();
    store = addEntry(store, { name: "내 요청", folder: "api", request: req() }, 1000, () => "c-1");
    saveStore(store);
    const loaded = loadStore();
    expect(loaded.collections.length).toBe(1);
    expect(loaded.collections[0].name).toBe("내 요청");
    expect(loaded.collections[0].folder).toBe("api");
  });

  it("이름이 비면 URL 사용", () => {
    let store = emptyStore();
    store = addEntry(store, { name: "  ", folder: "", request: req() }, 1, () => "c-1");
    expect(store.collections[0].name).toBe("https://api.example.com/x");
  });

  it("제거", () => {
    let store = emptyStore();
    store = addEntry(store, { name: "a", folder: "", request: req() }, 1, () => "c-1");
    store = addEntry(store, { name: "b", folder: "", request: req() }, 2, () => "c-2");
    store = removeEntry(store, "c-1");
    expect(store.collections.map((c) => c.id)).toEqual(["c-2"]);
  });

  it("폴더 목록", () => {
    let store = emptyStore();
    store = addEntry(store, { name: "a", folder: "dev", request: req() }, 1, () => "c-1");
    store = addEntry(store, { name: "b", folder: "dev", request: req() }, 2, () => "c-2");
    store = addEntry(store, { name: "c", folder: "", request: req() }, 3, () => "c-3");
    expect(foldersOf(store)).toEqual(["dev"]);
  });
});
