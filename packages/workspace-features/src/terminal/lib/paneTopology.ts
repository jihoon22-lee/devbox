import type { Tab } from "../types";
import { normalizePaneSizing } from "./paneSizing";

interface TerminalTopology {
  tabs: Tab[];
  activeTabId: string;
  activePaneId: string | null;
}

/** Remove a closed pane and select a surviving pane/tab when necessary. */
export function removePaneFromTopology(current: TerminalTopology, paneId: string): TerminalTopology {
  const { tabs, activeTabId, activePaneId } = current;
  const ownerIdx = tabs.findIndex((tab) => tab.paneIds.includes(paneId));
  if (ownerIdx === -1) {
    return { ...current, activePaneId: activePaneId === paneId ? null : activePaneId };
  }
  const owner = tabs[ownerIdx];
  const remaining = owner.paneIds.filter((id) => id !== paneId);
  const tabClosed = remaining.length === 0;
  const nextTabs = tabClosed
    ? tabs.filter((tab) => tab.id !== owner.id)
    : tabs.map((tab) =>
        tab.id === owner.id
          ? { ...tab, paneIds: remaining, sizing: normalizePaneSizing(undefined, tab.layout, remaining.length) }
          : tab,
      );
  let nextActiveTabId = activeTabId;
  let nextActivePaneId = activePaneId;
  if (tabClosed && activeTabId === owner.id) {
    const fallback = nextTabs[Math.min(ownerIdx, nextTabs.length - 1)] ?? null;
    nextActiveTabId = fallback?.id ?? "";
    nextActivePaneId = fallback?.paneIds[fallback.paneIds.length - 1] ?? null;
  } else if (activePaneId === paneId) {
    nextActivePaneId = remaining[remaining.length - 1] ?? null;
  }
  return { tabs: nextTabs, activeTabId: nextActiveTabId, activePaneId: nextActivePaneId };
}
