import { useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import TermPane from "./TermPane";
import type { Pane, Tab } from "../types";
import type { ShortcutAction } from "../lib/shortcuts";

interface PaneCanvasProps {
  tabs: Tab[];
  panes: Pane[];
  activeTabId: string;
  activePaneId: string | null;
  broadcastOn: boolean;
  registerWrite: (id: string, fn: (data: string) => void) => void;
  unregisterWrite: (id: string) => void;
  onClosePane: (id: string) => void;
  onFocusPane: (id: string) => void;
  onShortcut: (action: ShortcutAction) => void;
}

/**
 * 모든 팬(TermPane)을 항상 마운트한 채로 두고, React Portal로 어느 DOM에 그릴지만
 * 바꾼다. 활성 탭의 팬은 `.panes` grid로, 나머지는 화면 밖 holding pen으로 보낸다.
 * 탭 전환·팬의 다른 탭 이동 모두 "portal 대상만 바뀜"이라 TermPane이 언마운트되지
 * 않는다 (design doc "최대 함정" 참고).
 */
export default function PaneCanvas({
  tabs,
  panes,
  activeTabId,
  activePaneId,
  broadcastOn,
  registerWrite,
  unregisterWrite,
  onClosePane,
  onFocusPane,
  onShortcut,
}: PaneCanvasProps) {
  const [gridEl, setGridEl] = useState<HTMLDivElement | null>(null);
  const [holdEl, setHoldEl] = useState<HTMLDivElement | null>(null);

  const panesById = new Map(panes.map((p) => [p.id, p]));
  const activeTab = tabs.find((t) => t.id === activeTabId) ?? null;
  const activePaneIds = activeTab?.paneIds ?? [];
  const inactivePaneIds = tabs.filter((t) => t.id !== activeTabId).flatMap((t) => t.paneIds);

  const gridCols = activePaneIds.length === 0 ? 1 : Math.ceil(Math.sqrt(activePaneIds.length));
  const layout = activeTab?.layout ?? "grid";
  const gridStyle: CSSProperties =
    layout === "cols"
      ? { gridTemplateColumns: `repeat(${Math.max(1, activePaneIds.length)}, 1fr)` }
      : layout === "rows"
        ? { gridTemplateRows: `repeat(${Math.max(1, activePaneIds.length)}, 1fr)` }
        : { gridTemplateColumns: `repeat(${gridCols}, 1fr)` };

  const renderPane = (id: string, active: boolean) => {
    const pane = panesById.get(id);
    const target = active ? gridEl : holdEl;
    if (!pane || !target) return null;
    return createPortal(
      <TermPane
        sessionId={id}
        title={pane.distro}
        active={active}
        isFocusedPane={id === activePaneId}
        broadcastOn={broadcastOn}
        broadcastTargetIds={activePaneIds}
        registerWrite={registerWrite}
        unregisterWrite={unregisterWrite}
        onClose={() => onClosePane(id)}
        onFocusPane={() => onFocusPane(id)}
        onShortcut={onShortcut}
      />,
      target,
      id,
    );
  };

  return (
    <>
      <div className="panes" ref={setGridEl} style={gridStyle}>
        {activePaneIds.map((id) => renderPane(id, true))}
        {activePaneIds.length === 0 && (
          <div className="empty">
            No terminals. Select a distro and click "+ Terminal".
            <div className="dim">(터미널은 Windows에서 실행해야 동작합니다)</div>
          </div>
        )}
      </div>
      {/* 비활성 탭의 팬들은 여기서 마운트된 채로 숨어 있는다 (App.css: display:none) */}
      <div className="pane-holding" ref={setHoldEl}>
        {inactivePaneIds.map((id) => renderPane(id, false))}
      </div>
    </>
  );
}
