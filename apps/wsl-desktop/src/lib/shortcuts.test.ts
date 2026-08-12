import { describe, expect, it } from "vitest";
import { matchShortcut } from "./shortcuts";

/** 테스트용 최소 KeyboardEvent. 실제 이벤트 타입 전체를 만들 필요는 없다. */
function key(key: string, mods: Partial<KeyboardEvent> = {}): KeyboardEvent {
  return {
    key,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    ...mods,
  } as KeyboardEvent;
}

describe("matchShortcut", () => {
  it("Ctrl 없이는 아무것도 매치하지 않는다", () => {
    expect(matchShortcut(key("t", { shiftKey: true }))).toBeNull();
    expect(matchShortcut(key("Tab"))).toBeNull();
  });

  it("Ctrl+Shift+T는 new-tab", () => {
    expect(matchShortcut(key("t", { ctrlKey: true, shiftKey: true }))).toEqual({ type: "new-tab" });
    // 대문자 키 값(브라우저가 그대로 줄 수도 있음)도 동일하게 처리된다.
    expect(matchShortcut(key("T", { ctrlKey: true, shiftKey: true }))).toEqual({ type: "new-tab" });
  });

  it("Ctrl+Shift+D는 new-pane", () => {
    expect(matchShortcut(key("d", { ctrlKey: true, shiftKey: true }))).toEqual({ type: "new-pane" });
  });

  it("Ctrl+Shift+W는 close-pane", () => {
    expect(matchShortcut(key("w", { ctrlKey: true, shiftKey: true }))).toEqual({ type: "close-pane" });
  });

  it("Ctrl+Shift+Tab은 prev-tab", () => {
    expect(matchShortcut(key("Tab", { ctrlKey: true, shiftKey: true }))).toEqual({ type: "prev-tab" });
  });

  it("Ctrl+Tab(Shift 없이)은 next-tab", () => {
    expect(matchShortcut(key("Tab", { ctrlKey: true }))).toEqual({ type: "next-tab" });
  });

  it("Ctrl+Alt+1..9는 goto-tab(0-indexed)", () => {
    expect(matchShortcut(key("1", { ctrlKey: true, altKey: true }))).toEqual({ type: "goto-tab", index: 0 });
    expect(matchShortcut(key("9", { ctrlKey: true, altKey: true }))).toEqual({ type: "goto-tab", index: 8 });
    expect(matchShortcut(key("5", { ctrlKey: true, altKey: true }))).toEqual({ type: "goto-tab", index: 4 });
  });

  it("Ctrl+Alt+0이나 Ctrl+Alt+10처럼 범위를 벗어난 숫자는 매치하지 않는다", () => {
    expect(matchShortcut(key("0", { ctrlKey: true, altKey: true }))).toBeNull();
    // "10"은 Number("10")=10이지만 e.key로 두 자리 숫자가 오는 일은 없다 — 그래도 범위 밖이면 null.
    expect(matchShortcut(key("10", { ctrlKey: true, altKey: true }))).toBeNull();
  });

  it("Ctrl+Alt+문자(숫자가 아님)는 매치하지 않는다", () => {
    expect(matchShortcut(key("a", { ctrlKey: true, altKey: true }))).toBeNull();
  });

  it("회귀 방지: 단독 Ctrl+W는 매치하지 않는다 (셸의 readline 단어 삭제로 가야 함)", () => {
    expect(matchShortcut(key("w", { ctrlKey: true }))).toBeNull();
  });

  it("회귀 방지: 단독 Ctrl+T는 매치하지 않는다 (셸의 문자 위치 바꾸기로 가야 함)", () => {
    expect(matchShortcut(key("t", { ctrlKey: true }))).toBeNull();
  });

  it("Ctrl+Shift+Alt 조합(세 키 모두)은 어느 브랜치에도 걸리지 않아 null이다", () => {
    expect(matchShortcut(key("t", { ctrlKey: true, shiftKey: true, altKey: true }))).toBeNull();
    expect(matchShortcut(key("1", { ctrlKey: true, shiftKey: true, altKey: true }))).toBeNull();
  });

  it("Ctrl+Shift+그 외 문자는 null", () => {
    expect(matchShortcut(key("x", { ctrlKey: true, shiftKey: true }))).toBeNull();
  });
});
