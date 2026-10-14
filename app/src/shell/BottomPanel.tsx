import { PanelBottomClose } from "lucide-react";
import { type ReactNode, useRef, useState } from "react";
import { create } from "zustand";
import { IconButton, Tabs } from "../ui";
import { useResize } from "../ui/useResize";
import s from "./BottomPanel.module.css";
import { useLayout } from "./layout";

export interface BottomTab {
  id: string;
  label: string;
  render: () => ReactNode;
}
export const useBottomTabs = create<{ tabs: BottomTab[]; register(tab: BottomTab): () => void }>((set, get) => ({
  tabs: [],
  register: (tab) => {
    const owned = { ...tab };
    set({ tabs: [...get().tabs.filter((t) => t.id !== tab.id), owned] });
    return () => {
      if (get().tabs.find((t) => t.id === tab.id) === owned) set({ tabs: get().tabs.filter((t) => t.id !== tab.id) });
    };
  },
}));
export const registerBottomTab = (tab: BottomTab) => useBottomTabs.getState().register(tab);
export function BottomPanel() {
  const { bottomOpen, bottomHeight, setBottomHeight, toggleBottom } = useLayout();
  const tabs = useBottomTabs((s) => s.tabs);
  const [active, setActive] = useState<string>();
  const panel = useRef<HTMLElement>(null);
  const drag = useRef({ height: bottomHeight, scale: 1 });
  const displayedHeight = () => {
    const scale = Number(document.documentElement.style.zoom) || 1;
    const measured = panel.current?.getBoundingClientRect().height;
    return { height: measured && Number.isFinite(measured) ? measured / scale : bottomHeight, scale };
  };
  const resize = useResize((start, current) =>
    setBottomHeight(drag.current.height + (start.y - current.y) / drag.current.scale),
  );
  const current = tabs.find((t) => t.id === active) ?? tabs[0];
  if (!bottomOpen || !current) return null;
  return (
    <section ref={panel} className={s.panel} aria-label="하단 패널" style={{ height: bottomHeight }}>
      <div
        role="separator"
        aria-label="하단 패널 높이"
        aria-orientation="horizontal"
        aria-valuemin={120}
        aria-valuemax={600}
        aria-valuenow={bottomHeight}
        tabIndex={0}
        className={s.handle}
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          drag.current = displayedHeight();
          resize(event);
        }}
        onKeyDown={(event) => {
          if (event.key !== "ArrowUp" && event.key !== "ArrowDown" && event.key !== "Home" && event.key !== "End")
            return;
          event.preventDefault();
          setBottomHeight(
            event.key === "Home"
              ? 120
              : event.key === "End"
                ? 600
                : displayedHeight().height + (event.key === "ArrowUp" ? 20 : -20),
          );
        }}
      />
      <Tabs
        items={tabs}
        active={current.id}
        onSelect={setActive}
        actions={<IconButton label="하단 패널 접기" onClick={toggleBottom} icon={<PanelBottomClose size={16} />} />}
      />
      <div className={s.content} role="tabpanel" aria-label={current.label}>
        {current.render()}
      </div>
    </section>
  );
}
