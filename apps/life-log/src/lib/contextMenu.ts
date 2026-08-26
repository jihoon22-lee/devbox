import type { ContextMenuEntry } from "@devbox/context-menu";

const DATE_KEY_PATTERN = /^(\d{4})-(\d{2})-(\d{2})$/;

export function parseDateKey(value: string): Date | null {
  const match = DATE_KEY_PATTERN.exec(value);
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  if (year < 1) return null;
  const date = new Date(0);
  date.setHours(0, 0, 0, 0);
  date.setFullYear(year, month - 1, day);
  if (
    date.getFullYear() !== year ||
    date.getMonth() !== month - 1 ||
    date.getDate() !== day
  ) {
    return null;
  }
  return date;
}

export function buildDateContextMenu(busy: boolean): readonly ContextMenuEntry[] {
  return [
    { type: "item", id: "copy-date", label: "날짜 복사", disabled: busy },
    {
      type: "item",
      id: "export-markdown",
      label: "Markdown 내보내기",
      disabled: busy,
    },
    {
      type: "item",
      id: "export-json",
      label: "JSON 내보내기",
      disabled: busy,
    },
    {
      type: "item",
      id: "export-csv",
      label: "CSV 내보내기",
      disabled: busy,
    },
  ];
}
