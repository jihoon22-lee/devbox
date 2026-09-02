import { beforeEach, describe, expect, it } from "vitest";
import {
  loadCopyOnSelect,
  loadPinned,
  loadPinnedCwd,
  loadRecentPaths,
  loadTerminalFontSize,
  pushRecentPath,
  saveCopyOnSelect,
  savePinned,
  savePinnedCwd,
  saveTerminalFontSize,
} from "./storage";

beforeEach(() => {
  localStorage.clear();
});

describe("pinned 플래그/경로 왕복", () => {
  it("기본값(저장된 적 없음)은 false / 빈 문자열", () => {
    expect(loadPinned()).toBe(false);
    expect(loadPinnedCwd()).toBe("");
  });

  it("저장한 값을 그대로 읽어온다", () => {
    savePinned(true);
    expect(loadPinned()).toBe(true);
    savePinned(false);
    expect(loadPinned()).toBe(false);

    savePinnedCwd("/home/me/project");
    expect(loadPinnedCwd()).toBe("/home/me/project");
  });
});

describe("loadRecentPaths", () => {
  it("저장된 값이 없으면 빈 배열", () => {
    expect(loadRecentPaths()).toEqual([]);
  });

  it("localStorage에 손상된 JSON이 들어있어도 예외 없이 빈 배열을 반환한다", () => {
    localStorage.setItem("wsl-desktop:recent-paths", "{not valid json");
    expect(loadRecentPaths()).toEqual([]);
  });

  it("배열이 아닌 JSON이 저장돼 있으면 빈 배열을 반환한다", () => {
    localStorage.setItem("wsl-desktop:recent-paths", JSON.stringify({ not: "an array" }));
    expect(loadRecentPaths()).toEqual([]);
  });

  it("문자열이 아닌 항목은 걸러낸다", () => {
    localStorage.setItem("wsl-desktop:recent-paths", JSON.stringify(["/a", 42, null, "/b"]));
    expect(loadRecentPaths()).toEqual(["/a", "/b"]);
  });
});

describe("pushRecentPath", () => {
  it("추가한 경로가 맨 앞에 온다(MRU)", () => {
    pushRecentPath("/a");
    pushRecentPath("/b");
    expect(loadRecentPaths()).toEqual(["/b", "/a"]);
  });

  it("이미 있는 경로를 다시 push하면 중복 없이 맨 앞으로 옮긴다", () => {
    pushRecentPath("/a");
    pushRecentPath("/b");
    pushRecentPath("/c");
    pushRecentPath("/a");
    expect(loadRecentPaths()).toEqual(["/a", "/c", "/b"]);
  });

  it("최대 12개까지만 유지한다", () => {
    for (let index = 1; index <= 13; index += 1) pushRecentPath(`/${index}`);
    const result = loadRecentPaths();
    expect(result).toHaveLength(12);
    expect(result[0]).toBe("/13");
    expect(result[11]).toBe("/2");
    expect(result).not.toContain("/1");
  });

  it("공백만 있는 경로는 무시하고 기존 목록을 그대로 반환한다", () => {
    pushRecentPath("/a");
    const result = pushRecentPath("   ");
    expect(result).toEqual(["/a"]);
    expect(loadRecentPaths()).toEqual(["/a"]);
  });

  it("앞뒤 공백은 trim된 상태로 저장된다", () => {
    pushRecentPath("  /trimmed  ");
    expect(loadRecentPaths()).toEqual(["/trimmed"]);
  });
});

describe("터미널 UX 설정 왕복", () => {
  it("selection 자동 복사는 기본 on이고 명시적으로 끌 수 있다", () => {
    expect(loadCopyOnSelect()).toBe(true);
    saveCopyOnSelect(false);
    expect(loadCopyOnSelect()).toBe(false);
    saveCopyOnSelect(true);
    expect(loadCopyOnSelect()).toBe(true);
  });

  it("글꼴 크기는 기본값과 안전 범위로 정규화해 저장한다", () => {
    expect(loadTerminalFontSize()).toBe(13);
    saveTerminalFontSize(18);
    expect(loadTerminalFontSize()).toBe(18);
    localStorage.setItem("wsl-desktop:font-size", "999");
    expect(loadTerminalFontSize()).toBe(24);
    localStorage.setItem("wsl-desktop:font-size", "invalid");
    expect(loadTerminalFontSize()).toBe(13);
  });
});
