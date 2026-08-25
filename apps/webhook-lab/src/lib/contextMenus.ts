import type { ContextMenuEntry } from "@devbox/context-menu";

export function buildHistoryContextMenu(busy: boolean): readonly ContextMenuEntry[] {
  return [
    { type: "item", id: "copy-masked", label: "마스킹 복사", disabled: busy },
    { type: "item", id: "copy-raw", label: "원본 복사", disabled: busy },
    { type: "item", id: "copy-headers", label: "헤더 복사", disabled: busy },
    {
      type: "item",
      id: "convert-api-playground",
      label: "API Playground로 변환",
      // P2 #315의 api-request/v1 handoff가 준비되기 전에는 데이터를 임시 채널로 넘기지 않는다.
      disabled: true,
    },
    { type: "separator", id: "history-danger-separator" },
    { type: "item", id: "delete", label: "삭제", disabled: busy, danger: true },
  ];
}

export function buildRuleContextMenu(busy: boolean): readonly ContextMenuEntry[] {
  return [
    { type: "item", id: "edit", label: "편집", disabled: busy },
    { type: "item", id: "duplicate", label: "복제", disabled: busy },
    {
      type: "item",
      id: "copy-example-curl",
      label: "예시 curl 복사",
      // P1 #283이 current bind와 안전한 quoting 계약을 구현한 뒤 활성화한다.
      disabled: true,
    },
    { type: "separator", id: "rule-danger-separator" },
    { type: "item", id: "delete", label: "삭제", disabled: busy, danger: true },
  ];
}
