/**
 * Windows Terminal 호환 단축키 판별. 순수 함수 — DOM 조작이나 앱 상태를 건드리지 않는다.
 *
 * 단독 Ctrl+W/Ctrl+T를 쓰지 않는 이유: bash readline에서 Ctrl+W는 "커서 앞 단어
 * 삭제", Ctrl+T는 "문자 위치 바꾸기"(또는 fzf 등으로 재바인딩된 위젯)라 이 조합을
 * 가로채면 셸 안에서 평소 쓰던 줄 편집이 조용히 망가진다. Windows Terminal도 같은
 * 이유로 Ctrl+Shift+* 조합을 쓴다.
 */
export type ShortcutAction =
  | { type: "new-tab" }
  | { type: "new-pane" }
  | { type: "close-pane" }
  | { type: "next-tab" }
  | { type: "prev-tab" }
  | { type: "goto-tab"; index: number };

export function matchShortcut(e: KeyboardEvent): ShortcutAction | null {
  if (!e.ctrlKey) return null;

  if (e.shiftKey && !e.altKey) {
    const key = e.key.toLowerCase();
    if (key === "t") return { type: "new-tab" };
    if (key === "d") return { type: "new-pane" };
    if (key === "w") return { type: "close-pane" };
    if (e.key === "Tab") return { type: "prev-tab" };
    return null;
  }

  if (!e.shiftKey && !e.altKey) {
    if (e.key === "Tab") return { type: "next-tab" };
  }

  if (e.altKey && !e.shiftKey) {
    const n = Number(e.key);
    if (Number.isInteger(n) && n >= 1 && n <= 9) {
      return { type: "goto-tab", index: n - 1 };
    }
  }

  return null;
}
