import { describe, expect, it } from "vitest";
import { APP_SHORTCUTS, TERMINAL_SHORTCUTS, matchShortcut, type ShortcutEventSpec } from "./shortcuts";
import { matchTerminalKey } from "./terminalUx";

function event(spec: ShortcutEventSpec): KeyboardEvent {
  return {
    code: "",
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    ...spec,
  } as KeyboardEvent;
}

describe("shortcut reference stays in step with the matchers", () => {
  it("안내에 적힌 앱 단축키는 모두 matchShortcut이 실제로 인식한다", () => {
    for (const shortcut of APP_SHORTCUTS) {
      expect(matchShortcut(event(shortcut.event)), shortcut.keys).not.toBeNull();
    }
  });

  it("안내에 적힌 터미널 단축키는 모두 matchTerminalKey가 실제로 인식한다", () => {
    for (const shortcut of TERMINAL_SHORTCUTS) {
      expect(matchTerminalKey(event(shortcut.event)), shortcut.keys).not.toBeNull();
    }
  });

  it("안내 목록에 중복된 키 조합이 없다", () => {
    const keys = [...APP_SHORTCUTS, ...TERMINAL_SHORTCUTS].map((shortcut) => shortcut.keys);
    expect(new Set(keys).size).toBe(keys.length);
  });

  it("각 팬 이동 방향이 서로 다른 action으로 해석된다", () => {
    const directions = APP_SHORTCUTS
      .filter((shortcut) => shortcut.id.startsWith("focus-"))
      .map((shortcut) => {
        const action = matchShortcut(event(shortcut.event));
        return action?.type === "focus-pane" ? action.direction : null;
      });
    expect(directions).toEqual(["left", "right", "up", "down"]);
  });
});
