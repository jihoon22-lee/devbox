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
      // #305가 date-range schema, source metadata, native save boundary를 함께 소유한다.
      disabled: true,
    },
    {
      type: "item",
      id: "export-json",
      label: "JSON 내보내기",
      // 현재 집계 상태를 임시 JSON으로 직렬화해 #305의 privacy 계약을 우회하지 않는다.
      disabled: true,
    },
  ];
}
