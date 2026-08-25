import type { ContextMenuEntry } from "@devbox/context-menu";

const MAX_TAB_NAME_LENGTH = 80;

export function normalizeTabName(input: string): string | null {
  const name = input
    .replace(/[\r\n]+/g, " ")
    .trim()
    .slice(0, MAX_TAB_NAME_LENGTH);
  return name || null;
}

export function buildPaneContextMenu(busy: boolean): readonly ContextMenuEntry[] {
  return [
    {
      type: "item",
      id: "copy",
      label: "복사",
      // #262가 xterm selection snapshot과 clipboard write를 소유한다.
      disabled: true,
    },
    {
      type: "item",
      id: "paste",
      label: "붙여넣기",
      // #262가 clipboard permission, multiline confirm과 term.paste()를 소유한다.
      disabled: true,
    },
    {
      type: "item",
      id: "search",
      label: "검색",
      // #262가 xterm search addon과 query lifecycle을 소유한다.
      disabled: true,
    },
    { type: "item", id: "split-vertical", label: "세로 분할", disabled: busy },
    { type: "item", id: "split-horizontal", label: "가로 분할", disabled: busy },
    {
      type: "item",
      id: "copy-cwd",
      label: "cwd 복사",
      // Pane.cwd는 시작 경로일 뿐 현재 cwd가 아니다. #262의 OSC 7 값만 복사한다.
      disabled: true,
    },
    { type: "item", id: "close", label: "팬 닫기", disabled: busy, danger: true },
  ];
}

export function buildTabContextMenu(
  busy: boolean,
  hasOtherTabs: boolean,
): readonly ContextMenuEntry[] {
  return [
    { type: "item", id: "close", label: "닫기", disabled: busy, danger: true },
    {
      type: "item",
      id: "close-others",
      label: "다른 탭 닫기",
      disabled: busy || !hasOtherTabs,
      danger: true,
    },
    { type: "item", id: "rename", label: "이름 변경", disabled: busy },
    {
      type: "submenu",
      id: "layout",
      label: "레이아웃 전환",
      disabled: busy,
      items: [
        { type: "item", id: "layout-grid", label: "격자" },
        { type: "item", id: "layout-cols", label: "세로 분할" },
        { type: "item", id: "layout-rows", label: "가로 분할" },
      ],
    },
  ];
}
