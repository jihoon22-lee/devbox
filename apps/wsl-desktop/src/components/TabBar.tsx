import { useState } from "react";
import type { ContextMenuTriggerProps } from "@devbox/context-menu";
import { isKeyboardActivation } from "@devbox/a11y";
import type { Tab } from "../types";

interface TabBarProps {
  tabs: Tab[];
  activeTabId: string;
  onActivate: (tabId: string) => void;
  onClose: (tabId: string) => void;
  onReorder: (fromTabId: string, toTabId: string) => void;
  onDropPane: (paneId: string, tabId: string) => void;
  onNewTab: () => void;
  contextMenuTriggerProps: ContextMenuTriggerProps;
  actionsDisabled: boolean;
}

/** 탭 순서 변경(탭 pill 드래그)과 팬 이동(팬 헤더를 탭 위에 드롭) 둘 다 여기서 받는다.
 * dataTransfer의 mime 타입으로 두 드래그 종류를 구분한다. */
export default function TabBar({ tabs, activeTabId, onActivate, onClose, onReorder, onDropPane, onNewTab, contextMenuTriggerProps, actionsDisabled }: TabBarProps) {
  const [dragOverId, setDragOverId] = useState<string | null>(null);

  return (
    <div className="tab-bar">
      {tabs.map((tab) => (
        <div
          key={tab.id}
          className={`tab-pill ${tab.id === activeTabId ? "active" : ""} ${dragOverId === tab.id ? "drag-over" : ""}`}
          tabIndex={0}
          data-tab-id={tab.id}
          aria-current={tab.id === activeTabId ? "true" : undefined}
          aria-label={`${tab.title} 터미널 탭`}
          draggable
          onClick={() => onActivate(tab.id)}
          {...contextMenuTriggerProps}
          onKeyDown={(event) => {
            contextMenuTriggerProps.onKeyDown?.(event);
            if (
              event.defaultPrevented
              || event.target !== event.currentTarget
              || !isKeyboardActivation(event)
            ) return;
            event.preventDefault();
            onActivate(tab.id);
          }}
          onDragStart={(e) => {
            e.dataTransfer.setData("application/x-wsld-tab", tab.id);
            e.dataTransfer.effectAllowed = "move";
          }}
          onDragOver={(e) => {
            if (
              e.dataTransfer.types.includes("application/x-wsld-tab") ||
              e.dataTransfer.types.includes("application/x-wsld-pane")
            ) {
              e.preventDefault();
              e.dataTransfer.dropEffect = "move";
              setDragOverId(tab.id);
            }
          }}
          onDragLeave={() => setDragOverId((prev) => (prev === tab.id ? null : prev))}
          onDrop={(e) => {
            e.preventDefault();
            setDragOverId(null);
            const paneId = e.dataTransfer.getData("application/x-wsld-pane");
            if (paneId) {
              onDropPane(paneId, tab.id);
              return;
            }
            const fromTabId = e.dataTransfer.getData("application/x-wsld-tab");
            if (fromTabId && fromTabId !== tab.id) onReorder(fromTabId, tab.id);
          }}
        >
          <span className="tab-title">{tab.title}</span>
          <button
            className="tab-close"
            title="탭 닫기"
            disabled={actionsDisabled}
            onClick={(e) => {
              e.stopPropagation();
              onClose(tab.id);
            }}
          >
            ✕
          </button>
        </div>
      ))}
      <button className="tab-add" title="새 탭 (Ctrl+Shift+T)" disabled={actionsDisabled} onClick={onNewTab}>
        +
      </button>
    </div>
  );
}
