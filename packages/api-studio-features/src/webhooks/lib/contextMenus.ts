import type { ContextMenuEntry } from "@devbox/context-menu";

export function buildHistoryContextMenu(
  busy: boolean,
  canReplay = false,
): readonly ContextMenuEntry[] {
  return [
    { type: "item", id: "copy-masked", label: "마스킹 복사", disabled: busy },
    { type: "item", id: "copy-raw", label: "원본 복사", disabled: busy },
    { type: "item", id: "copy-headers", label: "헤더 복사", disabled: busy },
    { type: "item", id: "save-fixture", label: "마스킹된 fixture 저장", disabled: busy },
    { type: "item", id: "replay", label: "마스킹된 요청 재전송", disabled: busy || !canReplay },
    {
      type: "item",
      id: "convert-api-playground",
      label: "API Playground로 변환",
      // The backend publishes an opaque api-request/v1 handoff and launches
      // API Playground; no request data travels through the browser clipboard.
      disabled: busy,
    },
    {
      type: "item",
      id: "inspect-log-lens",
      label: "Log Lens에서 보기",
      // The backend publishes only a bounded webhook-log/v1 projection.
      disabled: busy,
    },
    { type: "separator", id: "history-danger-separator" },
    { type: "item", id: "delete", label: "삭제", disabled: busy, danger: true },
  ];
}

export function buildRuleContextMenu(
  busy: boolean,
  canCopyExampleCurl = false,
): readonly ContextMenuEntry[] {
  return [
    { type: "item", id: "edit", label: "편집", disabled: busy },
    { type: "item", id: "duplicate", label: "복제", disabled: busy },
    {
      type: "item",
      id: "copy-example-curl-powershell",
      label: "PowerShell curl.exe 복사",
      disabled: busy || !canCopyExampleCurl,
    },
    {
      type: "item",
      id: "copy-example-curl-posix",
      label: "POSIX sh curl 복사",
      disabled: busy || !canCopyExampleCurl,
    },
    { type: "item", id: "reset-sequence", label: "응답 시퀀스 초기화", disabled: busy },
    { type: "separator", id: "rule-danger-separator" },
    { type: "item", id: "delete", label: "삭제", disabled: busy, danger: true },
  ];
}
