import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type PointerEvent as ReactPointerEvent,
} from "react";
import type { ContextMenuTriggerProps } from "@devbox/context-menu";
import type { AskDialog } from "./AppDialog";
import type { CursorStyle, TerminalTheme } from "../lib/settings";
import { normalizeFractions, normalizePaneSizing, resizeAdjacent, toGridTemplate } from "../lib/paneSizing";
import TermPane, { type TerminalPaneHandle } from "./TermPane";
import type { Pane, PaneSizing, Tab } from "../types";
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
  onRetryPane: (key: string) => void;
  onFocusPane: (id: string) => void;
  onSizingChange: (tabId: string, sizing: { columns: number[]; rows: number[] }) => void;
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
  onRetryPane,
  onFocusPane,
  onSizingChange,
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

  const baseLayout = activeTab?.layout ?? "grid";
  const layout = zoomed ? "grid" : baseLayout;
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
  const placeholderRefs = useRef(new Map<string, HTMLElement>());
  const storedSizing = normalizePaneSizing(activeTab?.sizing, baseLayout, allActivePaneIds.length);
  const sizingSource = JSON.stringify([baseLayout, ...allActivePaneIds, storedSizing]);
  const [previewSizing, setPreviewSizing] = useState<{
    source: string;
    sizing: PaneSizing;
  } | null>(null);
  const baseSizing = previewSizing?.source === sizingSource ? previewSizing.sizing : storedSizing;
  useEffect(() => {
    setPreviewSizing((current) => current?.source === sizingSource ? current : null);
  }, [sizingSource]);

  useEffect(() => {
    if (activePaneId && activePaneIds.includes(activePaneId)) {
      placeholderRefs.current.get(activePaneId)?.focus();
    }
  }, [activePaneId, activePaneIds.join("|")]);
  const columns = zoomed
    ? [1]
    : normalizeFractions(baseSizing.columns, columnCount);
  const rows = zoomed
    ? [1]
    : normalizeFractions(baseSizing.rows, rowCount);

  const applySizing = useCallback((axis: "columns" | "rows", next: number[]) => {
    if (!activeTabId || !activeTab) return;
    const nextSizing = { ...baseSizing, [axis]: next };
    setPreviewSizing({ source: sizingSource, sizing: nextSizing });
    onSizingChange(activeTabId, nextSizing);
  }, [activeTab, activeTabId, baseSizing, onSizingChange, sizingSource]);

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
        const identity = pane.sessionId ?? pane.key;
        const order = activePaneIds.indexOf(identity);
        const active = order !== -1;
        const paneStyle: CSSProperties = active ? { order } : { display: "none" };
        if (pane.sessionId === null) {
          const connecting = pane.restoreStatus === "connecting";
          return (
            <section
              key={pane.key}
              className={`pane restore-placeholder ${connecting ? "connecting" : "failed"} ${identity === activePaneId ? "pane-focused" : ""}`}
              style={paneStyle}
              role="group"
              tabIndex={-1}
              ref={(node) => {
                if (node) placeholderRefs.current.set(identity, node);
                else placeholderRefs.current.delete(identity);
              }}
              data-pane-id={identity}
              aria-label={`${pane.distro} 터미널 복원 ${connecting ? "중" : "실패"}`}
              onMouseDownCapture={() => onFocusPane(identity)}
            >
              <div className="pane-head">
                <span className="pane-title">{pane.distro}</span>
                <span className={`pane-badge restore-${connecting ? "connecting" : "failed"}`} role="status">
                  {connecting ? "연결 중" : "복원 실패"}
                </span>
                <button
                  className="pane-close"
                  title="복원 자리 닫기"
                  disabled={actionsDisabled || connecting}
                  onClick={() => onClosePane(identity)}
                >
                  ✕
                </button>
              </div>
              <div className="restore-placeholder-body">
                <strong>{connecting ? "터미널을 복원하고 있습니다" : (pane.restoreError ?? "터미널을 복원하지 못했습니다.")}</strong>
                <dl>
                  <div><dt>배포판</dt><dd>{pane.distro}</dd></div>
                  <div><dt>경로</dt><dd title={pane.cwd}>{pane.cwd ?? "기본 경로"}</dd></div>
                  <div><dt>요청 방식</dt><dd>{pane.requestedMultiplexer ?? pane.multiplexer}</dd></div>
                </dl>
                {!connecting && (
                  <button
                    type="button"
                    className="btn compact"
                    disabled={actionsDisabled}
                    onClick={() => onRetryPane(pane.key)}
                  >
                    다시 시도
                  </button>
                )}
              </div>
            </section>
          );
        }
        const sessionId = pane.sessionId;
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
            requestedMultiplexer={pane.requestedMultiplexer}
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
            style={paneStyle}
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
