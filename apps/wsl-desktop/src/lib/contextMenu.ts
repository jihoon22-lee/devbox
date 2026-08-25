import type { ContextMenuEntry } from "@devbox/context-menu";

const MAX_TAB_NAME_LENGTH = 80;

export function normalizeTabName(input: string): string | null {
  const name = input
    .replace(/[\r\n]+/g, " ")
    .trim()
    .slice(0, MAX_TAB_NAME_LENGTH);
  return name || null;
}

export interface PaneContextCapabilities {
  busy: boolean;
  hasSelection: boolean;
  hasCwd: boolean;
}

export function buildPaneContextMenu({ busy, hasSelection, hasCwd }: PaneContextCapabilities): readonly ContextMenuEntry[] {
  return [
    {
      type: "item",
      id: "copy",
      label: "복사",
      disabled: busy || !hasSelection,
    },
    {
      type: "item",
      id: "paste",
      label: "붙여넣기",
      disabled: busy,
    },
    {
      type: "item",
      id: "search",
      label: "검색",
      disabled: busy,
    },
    { type: "item", id: "split-vertical", label: "세로 분할", disabled: busy },
    { type: "item", id: "split-horizontal", label: "가로 분할", disabled: busy },
    {
      type: "item",
      id: "copy-cwd",
      label: "cwd 복사",
      disabled: busy || !hasCwd,
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
