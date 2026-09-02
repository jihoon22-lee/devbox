/**
 * Windows Terminal 호환 단축키 판별. 순수 함수 — DOM 조작이나 앱 상태를 건드리지 않는다.
 *
 * 단독 Ctrl+W/Ctrl+T를 쓰지 않는 이유: bash readline에서 Ctrl+W는 "커서 앞 단어
 * 삭제", Ctrl+T는 "문자 위치 바꾸기"(또는 fzf 등으로 재바인딩된 위젯)라 이 조합을
 * 가로채면 셸 안에서 평소 쓰던 줄 편집이 조용히 망가진다. Windows Terminal도 같은
 * 이유로 Ctrl+Shift+* 조합을 쓴다.
 */
import type { FocusDirection } from "./paneGeometry";

export type ShortcutAction =
  | { type: "new-tab" }
  | { type: "new-pane" }
  | { type: "command-palette" }
  | { type: "close-pane" }
  | { type: "next-tab" }
  | { type: "prev-tab" }
  | { type: "goto-tab"; index: number }
  | { type: "focus-pane"; direction: FocusDirection };

export function matchShortcut(e: KeyboardEvent): ShortcutAction | null {
  // 팬 간 포커스 이동: Alt+Arrow (Windows Terminal 기본). Ctrl 조합보다 먼저 검사한다.
  if (e.altKey && !e.ctrlKey && !e.shiftKey) {
    if (e.key === "ArrowRight") return { type: "focus-pane", direction: "right" };
    if (e.key === "ArrowLeft") return { type: "focus-pane", direction: "left" };
    if (e.key === "ArrowDown") return { type: "focus-pane", direction: "down" };
    if (e.key === "ArrowUp") return { type: "focus-pane", direction: "up" };
  }
  if (!e.ctrlKey) return null;

  if (e.shiftKey && !e.altKey) {
    const key = e.key.toLowerCase();
    if (key === "t") return { type: "new-tab" };
    if (key === "d") return { type: "new-pane" };
    if (key === "p") return { type: "command-palette" };
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

/** 단축키 안내에 쓰는 설명. 표시 문구와 실제 matcher가 어긋나지 않도록 회귀 테스트가
 * 여기 적힌 이벤트를 그대로 matcher에 넣어 확인한다. */
export interface ShortcutEventSpec {
  key: string;
  code?: string;
  ctrlKey?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
}

export interface ShortcutDescriptor {
  id: string;
  keys: string;
  label: string;
  scope: "app" | "terminal";
  event: ShortcutEventSpec;
}

export const APP_SHORTCUTS: readonly ShortcutDescriptor[] = [
  { id: "new-tab", keys: "Ctrl+Shift+T", label: "새 탭", scope: "app", event: { key: "T", ctrlKey: true, shiftKey: true } },
  { id: "new-pane", keys: "Ctrl+Shift+D", label: "활성 탭에 팬 추가", scope: "app", event: { key: "D", ctrlKey: true, shiftKey: true } },
  { id: "command-palette", keys: "Ctrl+Shift+P", label: "명령 팔레트", scope: "app", event: { key: "P", ctrlKey: true, shiftKey: true } },
  { id: "close-pane", keys: "Ctrl+Shift+W", label: "활성 팬 닫기", scope: "app", event: { key: "W", ctrlKey: true, shiftKey: true } },
  { id: "next-tab", keys: "Ctrl+Tab", label: "다음 탭", scope: "app", event: { key: "Tab", ctrlKey: true } },
  { id: "prev-tab", keys: "Ctrl+Shift+Tab", label: "이전 탭", scope: "app", event: { key: "Tab", ctrlKey: true, shiftKey: true } },
  { id: "goto-tab", keys: "Ctrl+Alt+1 ~ 9", label: "N번째 탭으로 이동", scope: "app", event: { key: "3", ctrlKey: true, altKey: true } },
  { id: "focus-left", keys: "Alt+←", label: "왼쪽 팬으로 이동", scope: "app", event: { key: "ArrowLeft", altKey: true } },
  { id: "focus-right", keys: "Alt+→", label: "오른쪽 팬으로 이동", scope: "app", event: { key: "ArrowRight", altKey: true } },
  { id: "focus-up", keys: "Alt+↑", label: "위쪽 팬으로 이동", scope: "app", event: { key: "ArrowUp", altKey: true } },
  { id: "focus-down", keys: "Alt+↓", label: "아래쪽 팬으로 이동", scope: "app", event: { key: "ArrowDown", altKey: true } },
];

export const TERMINAL_SHORTCUTS: readonly ShortcutDescriptor[] = [
  { id: "copy", keys: "Ctrl+Shift+C", label: "선택 복사", scope: "terminal", event: { key: "C", ctrlKey: true, shiftKey: true } },
  { id: "paste", keys: "Ctrl+Shift+V", label: "붙여넣기", scope: "terminal", event: { key: "V", ctrlKey: true, shiftKey: true } },
  { id: "search", keys: "Ctrl+Shift+F", label: "출력 검색", scope: "terminal", event: { key: "F", ctrlKey: true, shiftKey: true } },
  { id: "font-increase", keys: "Ctrl++", label: "글꼴 확대", scope: "terminal", event: { key: "+", code: "Equal", ctrlKey: true } },
  { id: "font-decrease", keys: "Ctrl+-", label: "글꼴 축소", scope: "terminal", event: { key: "-", code: "Minus", ctrlKey: true } },
  { id: "font-reset", keys: "Ctrl+0", label: "기본 글꼴 크기", scope: "terminal", event: { key: "0", code: "Digit0", ctrlKey: true } },
];

/** matcher가 없는 항목 — 공용 context-menu 패키지와 브라우저가 소유한다. */
export const OTHER_SHORTCUTS: readonly { keys: string; label: string }[] = [
  { keys: "Shift+F10 / Menu", label: "팬·탭 컨텍스트 메뉴 열기" },
  { keys: "Ctrl+C", label: "셸로 SIGINT 보내기 (선택 여부와 무관)" },
  { keys: "가운데 클릭", label: "팬에 붙여넣기 · 탭에서는 탭 닫기" },
];
