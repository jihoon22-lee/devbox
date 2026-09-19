import type { WorkspaceDefinition, WorkspacePaneDefinition } from "../types";

export const RESTORE_START_CONCURRENCY = 2;

/** Start the requested active pane alone before any background restore work. */
export function orderWorkspacePanes(
  workspace: WorkspaceDefinition,
): { active: WorkspacePaneDefinition; remaining: WorkspacePaneDefinition[] } {
  const activeKey = workspace.activePaneKey
    ?? workspace.tabs.find((tab) => tab.id === workspace.activeTabId)?.paneKeys[0]
    ?? workspace.panes[0]?.key;
  const active = workspace.panes.find((pane) => pane.key === activeKey) ?? workspace.panes[0];
  if (!active) throw new Error("workspace has no panes");
  return {
    active,
    remaining: workspace.panes.filter((pane) => pane.key !== active.key),
  };
}

/** Run bounded workers without creating one pending promise per workspace pane. */
export async function runWithConcurrencyLimit<T>(
  items: readonly T[],
  concurrency: number,
  worker: (item: T) => Promise<void>,
): Promise<void> {
  const workerCount = Math.min(items.length, Math.max(1, Math.floor(concurrency)));
  let nextIndex = 0;
  await Promise.all(Array.from({ length: workerCount }, async () => {
    while (nextIndex < items.length) {
      const item = items[nextIndex];
      nextIndex += 1;
      await worker(item);
    }
  }));
}
