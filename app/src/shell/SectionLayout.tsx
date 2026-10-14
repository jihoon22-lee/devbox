import type { ReactNode } from "react";
import { useResize } from "../ui/useResize";
import { useLayout } from "./layout";
import s from "./SectionLayout.module.css";

export function SectionLayout({ list, children }: { list: ReactNode; children: ReactNode }) {
  const { listWidth, listCollapsed, setListWidth } = useLayout();
  const onPointerDown = useResize((start, current) =>
    setListWidth(listWidth + (current.x - start.x) / (Number(document.documentElement.style.zoom) || 1)),
  );
  return (
    <div
      className={s.layout}
      style={{ gridTemplateColumns: listCollapsed ? "0 0 minmax(0, 1fr)" : `${listWidth}px 4px minmax(0, 1fr)` }}
    >
      <aside className={s.list} aria-label="목록" aria-hidden={listCollapsed} inert={listCollapsed}>
        {list}
      </aside>
      <div
        role="separator"
        aria-label="목록 너비"
        aria-orientation="vertical"
        aria-valuemin={180}
        aria-valuemax={480}
        aria-valuenow={listWidth}
        tabIndex={listCollapsed ? -1 : 0}
        aria-hidden={listCollapsed}
        style={{ visibility: listCollapsed ? "hidden" : undefined }}
        className={s.handle}
        onPointerDown={onPointerDown}
        onKeyDown={(event) => {
          if (event.key !== "ArrowLeft" && event.key !== "ArrowRight" && event.key !== "Home" && event.key !== "End")
            return;
          event.preventDefault();
          setListWidth(
            event.key === "Home"
              ? 180
              : event.key === "End"
                ? 480
                : listWidth + (event.key === "ArrowRight" ? 20 : -20),
          );
        }}
      />
      <section className={s.body} aria-label="본문">
        {children}
      </section>
    </div>
  );
}
