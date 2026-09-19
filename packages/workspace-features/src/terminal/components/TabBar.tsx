import { useEffect, useRef, useState } from "react";
import type { ContextMenuTriggerProps } from "@devbox/context-menu";
import { isKeyboardActivation } from "@devbox/a11y";
import type { Tab } from "../types";

interface TabBarProps {
  tabs: Tab[];
  activeTabId: string;
  onActivate: (tabId: string) => void;
  onClose: (tabId: string) => void;
  onRename: (tabId: string) => void;
  onReorder: (fromTabId: string, toTabId: string) => void;
  onDropPane: (paneId: string, tabId: string) => void;
  onNewTab: () => void;
  contextMenuTriggerProps: ContextMenuTriggerProps;
  actionsDisabled: boolean;
}

/** 탭 순서 변경(탭 pill 드래그)과 팬 이동(팬 헤더를 탭 위에 드롭) 둘 다 여기서 받는다.
 * dataTransfer의 mime 타입으로 두 드래그 종류를 구분한다. */
export default function TabBar({
  tabs,
  activeTabId,
  onActivate,
  onClose,
  onRename,
  onReorder,
  onDropPane,
  onNewTab,
  contextMenuTriggerProps,
  actionsDisabled,
}: TabBarProps) {
  const [dragOverId, setDragOverId] = useState<string | null>(null);
  const activeRef = useRef<HTMLDivElement>(null);

  // 탭 바는 가로 스크롤된다. Ctrl+Tab이나 Ctrl+Alt+N으로 화면 밖 탭을 활성화해도
  // 보이도록 활성 탭을 시야 안으로 끌어온다.
  useEffect(() => {
    const element = activeRef.current;
    // jsdom과 일부 WebView 조합에는 scrollIntoView가 없다. 없으면 스크롤만 건너뛴다.
    if (typeof element?.scrollIntoView === "function") {
      element.scrollIntoView({ block: "nearest", inline: "nearest" });
    }
  }, [activeTabId, tabs.length]);

  const step = (from: string, delta: 1 | -1) => {
    const index = tabs.findIndex((tab) => tab.id === from);
    if (index === -1) return;
    const next = tabs[index + delta];
    if (next) onActivate(next.id);
  };

  return (
    <div className="tab-bar">
      <div className="tab-list" role="tablist" aria-label="터미널 탭">
        {tabs.map((tab) => {
          const active = tab.id === activeTabId;
          return (
            <div
              key={tab.id}
              ref={active ? activeRef : undefined}
              className={`tab-pill ${active ? "active" : ""} ${dragOverId === tab.id ? "drag-over" : ""}`}
              role="tab"
              aria-selected={active}
              tabIndex={active ? 0 : -1}
              data-tab-id={tab.id}
              aria-label={`${tab.title} 터미널 탭`}
              draggable
              onClick={() => onActivate(tab.id)}
              onDoubleClick={() => {
                if (!actionsDisabled) onRename(tab.id);
              }}
              onAuxClick={(event) => {
                if (event.button !== 1 || actionsDisabled) return;
                event.preventDefault();
                onClose(tab.id);
              }}
              // 가운데 클릭의 기본 자동 스크롤을 막아야 onAuxClick이 탭 닫기로만 쓰인다.
              onMouseDown={(event) => {
                if (event.button === 1) event.preventDefault();
              }}
              {...contextMenuTriggerProps}
              onKeyDown={(event) => {
                contextMenuTriggerProps.onKeyDown?.(event);
                if (event.defaultPrevented || event.target !== event.currentTarget) return;
                if (event.key === "ArrowRight") {
                  event.preventDefault();
                  step(tab.id, 1);
                  return;
                }
                if (event.key === "ArrowLeft") {
                  event.preventDefault();
                  step(tab.id, -1);
                  return;
                }
                if (event.key === "Home" && tabs[0]) {
                  event.preventDefault();
                  onActivate(tabs[0].id);
                  return;
                }
                if (event.key === "End" && tabs[tabs.length - 1]) {
                  event.preventDefault();
                  onActivate(tabs[tabs.length - 1].id);
                  return;
                }
                if (event.key === "Delete" && !actionsDisabled) {
                  event.preventDefault();
                  onClose(tab.id);
                  return;
                }
                if (!isKeyboardActivation(event)) return;
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
              {/* 마우스 전용 보조 수단. tab role 안의 focusable 컨트롤은 중첩 상호작용이
                * 되므로 접근성 트리에서 감추고, 키보드·보조기술 사용자는 컨텍스트 메뉴,
                * Delete, Ctrl+Shift+W로 같은 동작에 도달한다. */}
              <span
                className="tab-close"
                aria-hidden="true"
                title="탭 닫기"
                onClick={(event) => {
                  event.stopPropagation();
                  if (!actionsDisabled) onClose(tab.id);
                }}
              >
                ✕
              </span>
            </div>
          );
        })}
      </div>
      <button className="tab-add" title="새 탭 (Ctrl+Shift+T)" disabled={actionsDisabled} onClick={onNewTab}>
        +
      </button>
    </div>
  );
}
