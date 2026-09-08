import type { ContextMenuEntry } from "@devbox/context-menu";
import type { RepoOpenTarget } from "../api";

export function buildRepositoryContextMenu(
  targets: readonly RepoOpenTarget[] | null,
  busy: boolean,
): readonly ContextMenuEntry[] {
  const openItems: ContextMenuEntry[] = (targets ?? []).map((target) => ({
    type: "item",
    id: `open-in:${target.id}`,
    label: target.displayName,
  }));
  return [
    {
      type: "submenu",
      id: "open-in",
      label: "다른 앱으로 열기",
      disabled: busy || targets === null || openItems.length === 0,
      items: openItems,
    },
    { type: "item", id: "create-worktree", label: "worktree 생성", disabled: busy },
    { type: "item", id: "copy-path", label: "경로 복사", disabled: busy },
    { type: "item", id: "open-folder", label: "탐색기에서 열기", disabled: busy },
  ];
}
