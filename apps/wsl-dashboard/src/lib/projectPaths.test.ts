import { beforeEach, describe, expect, it } from "vitest";
import { addPath, loadSavedPaths, removePath, savePaths } from "./projectPaths";

beforeEach(() => {
  localStorage.clear();
});

describe("loadSavedPaths", () => {
  it("저장된 값이 없으면 빈 배열", () => {
    expect(loadSavedPaths()).toEqual([]);
  });

  it("손상된 JSON이 들어있어도 예외 없이 빈 배열을 반환한다", () => {
    localStorage.setItem("wsld-projects", "{not valid json");
    expect(loadSavedPaths()).toEqual([]);
  });

  it("저장한 값을 왕복한다", () => {
    savePaths(["/a", "/b"]);
    expect(loadSavedPaths()).toEqual(["/a", "/b"]);
  });
});

describe("addPath", () => {
  it("새 경로는 끝에 추가된다", () => {
    expect(addPath(["/a"], "/b")).toEqual(["/a", "/b"]);
  });

  it("이미 있는 경로는 중복 추가하지 않고 같은 배열을 그대로 반환한다", () => {
    const paths = ["/a", "/b"];
    expect(addPath(paths, "/a")).toBe(paths); // 참조 동일성까지 보존 (불필요한 리렌더 방지)
  });

  it("빈 문자열/공백만 있는 경로는 무시한다", () => {
    const paths = ["/a"];
    expect(addPath(paths, "   ")).toBe(paths);
    expect(addPath(paths, "")).toBe(paths);
  });

  it("앞뒤 공백은 trim 후 비교/저장한다", () => {
    expect(addPath(["/a"], "  /b  ")).toEqual(["/a", "/b"]);
  });
});

describe("removePath", () => {
  it("해당 경로만 제거한다", () => {
    expect(removePath(["/a", "/b", "/c"], "/b")).toEqual(["/a", "/c"]);
  });

  it("없는 경로를 제거해도 예외 없이 원래 목록을 반환한다", () => {
    expect(removePath(["/a"], "/not-there")).toEqual(["/a"]);
  });
});
