import { useCallback, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from "react";
import type { ContextMenuTriggerProps } from "@devbox/context-menu";
import type { AskDialog } from "./AppDialog";
import type { CursorStyle, TerminalTheme } from "../lib/settings";
import { normalizeFractions, resizeAdjacent, toGridTemplate } from "../lib/paneSizing";
import TermPane, { type TerminalPaneHandle } from "./TermPane";
import type { Pane, Tab } from "../types";
import type { ShortcutAction } from "../lib/shortcuts";

interface PaneCanvasProps {
  tabs: Tab[];
  panes: Pane[];
  activeTabId: string;
  activePaneId: string | null;
  broadcastOn: boolean;
  broadcastTargetIds: string[];
  copyOnSelect: boolean;
  fontSize: number;
  fontFamily: string;
  theme: TerminalTheme;
  cursorStyle: CursorStyle;
  cursorBlink: boolean;
  scrollbackLines: number;
  registerWrite: (id: string, fn: (data: string) => void) => void;
  unregisterWrite: (id: string) => void;
  registerFocus: (id: string, fn: () => void) => void;
  unregisterFocus: (id: string) => void;
  registerTerminalHandle: (id: string, handle: TerminalPaneHandle) => void;
  unregisterTerminalHandle: (id: string) => void;
  onClosePane: (id: string) => void;
  onFocusPane: (id: string) => void;
  onShortcut: (action: ShortcutAction) => void;
  onFontSizeChange: (fontSize: number) => void;
  onMetadataChange: (id: string, metadata: { title?: string; cwd?: string }) => void;
  onTerminalError: (message: string) => void;
  onBroadcastFailure?: () => void;
  windowsBuildNumber: number | null;
  contextMenuTriggerProps: ContextMenuTriggerProps;
  actionsDisabled: boolean;
  /** 확대해서 혼자 보이는 팬. 활성 탭에 없으면 무시한다. */
  zoomedPaneId: string | null;
  ask: AskDialog;
  onConfirmLinkHost: (host: string) => Promise<boolean>;
}

/**
 * 모든 팬(TermPane)을 `panes` 배열 순서 그대로, 항상 같은 부모(.panes) 아래
 * 마운트한 채로 둔다. React portal은 쓰지 않는다 — portal은 DOM 출력 위치만 바꿀
 * 뿐 React 엘리먼트 트리상의 "부모"(그 portal을 반환한 JSX 위치)는 그대로다.
 * 팬을 활성 탭용 `.map()`에서 비활성 탭용 `.map()`으로 옮기면, 같은 key라도
 * "부모 children 배열"이 바뀌는 셈이라 React가 이전 부모에서는 그 key를 더 이상
 * 찾지 못해 언마운트하고 새 부모에서는 처음 보는 key로 취급해 새로 마운트한다
 * (portal의 containerInfo가 바뀌는 것도 별도로 같은 결과를 유발한다). 이 구현을
 * 브라우저에서 실측해 실제로 재마운트됨을 먼저 확인한 뒤 이 방식으로 고쳤다.
 *
 * 대신 비활성 탭의 팬은 CSS display:none으로 숨기고, 보이는 팬들의 화면 순서는
 * CSS order로만 준다. `panes` 배열 자체의 순서는 절대 바뀌지 않는다(App.tsx는
 * 추가는 append, 제거는 filter만 쓴다) — React가 fiber를 옮길 일이 구조적으로
 * 없어야 재마운트도 없다.
 */
export default function PaneCanvas({
  tabs,
  panes,
  activeTabId,
  activePaneId,
  broadcastOn,
  broadcastTargetIds,
  copyOnSelect,
  fontSize,
  fontFamily,
  theme,
  cursorStyle,
  cursorBlink,
  scrollbackLines,
  registerWrite,
  unregisterWrite,
  registerFocus,
  unregisterFocus,
  registerTerminalHandle,
  unregisterTerminalHandle,
  onClosePane,
  onFocusPane,
  onShortcut,
  onFontSizeChange,
  onMetadataChange,
  onTerminalError,
  onBroadcastFailure,
  windowsBuildNumber,
  contextMenuTriggerProps,
  actionsDisabled,
  zoomedPaneId,
  ask,
  onConfirmLinkHost,
}: PaneCanvasProps) {
  const activeTab = tabs.find((t) => t.id === activeTabId) ?? null;
  const allActivePaneIds = activeTab?.paneIds ?? [];
  // 확대된 팬은 활성 탭 안에 있을 때만 유효하다. 탭을 옮기면 자동으로 풀린다.
  const zoomed = zoomedPaneId !== null && allActivePaneIds.includes(zoomedPaneId) ? zoomedPaneId : null;
  const activePaneIds = zoomed ? [zoomed] : allActivePaneIds;

  const layout = zoomed ? "grid" : (activeTab?.layout ?? "grid");
  const gridCols = activePaneIds.length === 0 ? 1 : Math.ceil(Math.sqrt(activePaneIds.length));
  const gridRows = Math.ceil(activePaneIds.length / gridCols);
  // 트랙 수는 레이아웃이 정한다. 하나뿐인 축에는 구분선을 만들지 않는다.
  const columnCount = layout === "cols"
    ? Math.max(1, activePaneIds.length)
    : layout === "rows" ? 1 : Math.max(1, gridCols);
  const rowCount = layout === "rows"
    ? Math.max(1, activePaneIds.length)
    : layout === "cols" ? 1 : Math.max(1, gridRows);

  const containerRef = useRef<HTMLDivElement>(null);
  const [sizing, setSizing] = useState<Record<string, { columns: number[]; rows: number[] }>>({});
  // 팬 수나 레이아웃이 바뀌면 길이가 어긋나 균등 분할로 되돌아간다.
  const columns = normalizeFractions(sizing[activeTabId]?.columns, columnCount);
  const rows = normalizeFractions(sizing[activeTabId]?.rows, rowCount);

  const applySizing = useCallback((axis: "columns" | "rows", next: number[]) => {
    setSizing((previous) => {
      const current = previous[activeTabId] ?? { columns: [], rows: [] };
      return { ...previous, [activeTabId]: { ...current, [axis]: next } };
    });
  }, [activeTabId]);

  const startDrag = (
    axis: "columns" | "rows",
    index: number,
    event: ReactPointerEvent<HTMLDivElement>,
  ) => {
    const container = containerRef.current;
    if (!container || actionsDisabled) return;
    event.preventDefault();
    const rect = container.getBoundingClientRect();
    const extent = axis === "columns" ? rect.width : rect.height;
    if (extent <= 0) return;
    const origin = axis === "columns" ? event.clientX : event.clientY;
    const start = axis === "columns" ? columns : rows;
    const handle = event.currentTarget;
    handle.setPointerCapture(event.pointerId);

    const move = (moveEvent: PointerEvent) => {
      const position = axis === "columns" ? moveEvent.clientX : moveEvent.clientY;
      applySizing(axis, resizeAdjacent(start, index, (position - origin) / extent));
    };
    const stop = () => {
      handle.releasePointerCapture?.(event.pointerId);
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", stop);
      handle.removeEventListener("pointercancel", stop);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", stop);
    handle.addEventListener("pointercancel", stop);
  };

  const nudge = (axis: "columns" | "rows", index: number, delta: number) => {
    if (actionsDisabled) return;
    applySizing(axis, resizeAdjacent(axis === "columns" ? columns : rows, index, delta));
  };

  const dividers = (axis: "columns" | "rows") => {
    const fractions = axis === "columns" ? columns : rows;
    if (fractions.length < 2) return null;
    let offset = 0;
    return fractions.slice(0, -1).map((fraction, index) => {
      offset += fraction;
      const percent = `${(offset * 100).toFixed(4)}%`;
      const value = Math.round(offset * 100);
      return (
        <div
          key={`${axis}-${index}`}
          className={`pane-divider ${axis}`}
          role="separator"
          aria-orientation={axis === "columns" ? "vertical" : "horizontal"}
          aria-label={`${index + 1}번째 구분선 크기 조절`}
          aria-valuenow={value}
          aria-valuemin={0}
          aria-valuemax={100}
          tabIndex={0}
          style={axis === "columns" ? { left: percent } : { top: percent }}
          onPointerDown={(event) => startDrag(axis, index, event)}
          onDoubleClick={() => applySizing(axis, normalizeFractions(undefined, fractions.length))}
          onKeyDown={(event) => {
            const grow = axis === "columns" ? "ArrowRight" : "ArrowDown";
            const shrink = axis === "columns" ? "ArrowLeft" : "ArrowUp";
            if (event.key === grow) {
              event.preventDefault();
              nudge(axis, index, 0.02);
            } else if (event.key === shrink) {
              event.preventDefault();
              nudge(axis, index, -0.02);
            } else if (event.key === "Home") {
              event.preventDefault();
              applySizing(axis, normalizeFractions(undefined, fractions.length));
            }
          }}
        />
      );
    });
  };

  const gridStyle: CSSProperties = {
    gridTemplateColumns: toGridTemplate(columns),
    gridTemplateRows: toGridTemplate(rows),
  };

  return (
    <div className="panes" ref={containerRef} style={gridStyle}>
      {!zoomed && dividers("columns")}
      {!zoomed && dividers("rows")}
      {panes.map((pane) => {
        // 세션이 아직 붙지 않은 팬(sessionId === null)은 TermPane을 마운트할 것이 없다
        // — 지금 이 릴리스에서는 startSession 성공 후에만 팬이 생기므로 실질적으로는
        // 항상 non-null이지만, 타입이 허용하는 미래(레이아웃 복원)를 위해 가드해 둔다.
        // panes 배열 자체는 건드리지 않으므로(그냥 렌더를 건너뜀) 순서 불변식은 유지된다.
        if (pane.sessionId === null) return null;
        const sessionId = pane.sessionId;
        const order = activePaneIds.indexOf(sessionId);
        const active = order !== -1;
        return (
          <TermPane
            key={pane.key}
            sessionId={sessionId}
            title={pane.title?.trim() || pane.distro}
            active={active}
            isFocusedPane={sessionId === activePaneId}
            broadcastOn={broadcastOn}
            broadcastTargetIds={broadcastTargetIds}
            isBroadcastTarget={broadcastOn && broadcastTargetIds.includes(sessionId)}
            initialCommand={pane.initialCommand}
            copyOnSelect={copyOnSelect}
            fontSize={fontSize}
            fontFamily={fontFamily}
            theme={theme}
            cursorStyle={cursorStyle}
            cursorBlink={cursorBlink}
            scrollbackLines={scrollbackLines}
            multiplexer={pane.multiplexer}
            resumed={pane.resumed === true}
            registerWrite={registerWrite}
            unregisterWrite={unregisterWrite}
            registerFocus={registerFocus}
            unregisterFocus={unregisterFocus}
            registerTerminalHandle={registerTerminalHandle}
            unregisterTerminalHandle={unregisterTerminalHandle}
            onClose={() => onClosePane(sessionId)}
            onFocusPane={() => onFocusPane(sessionId)}
            onShortcut={onShortcut}
            onFontSizeChange={onFontSizeChange}
            onMetadataChange={onMetadataChange}
            onTerminalError={onTerminalError}
            onBroadcastFailure={onBroadcastFailure}
            windowsBuildNumber={windowsBuildNumber}
            contextMenuTriggerProps={contextMenuTriggerProps}
            actionsDisabled={actionsDisabled}
            ask={ask}
            onConfirmLinkHost={onConfirmLinkHost}
            style={active ? { order } : { display: "none" }}
          />
        );
      })}
      {activePaneIds.length === 0 && (
        <div className="empty">
          터미널이 없습니다. 배포판을 선택하고 "+ 터미널"을 클릭하세요.
          <div className="dim">(터미널은 Windows에서 실행해야 동작합니다)</div>
        </div>
      )}
    </div>
  );
}
