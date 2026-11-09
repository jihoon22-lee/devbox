import { PanelLeftOpen } from "lucide-react";
import { type ReactNode, useEffect, useId, useRef } from "react";
import { IconButton } from "../ui";
import { useResize } from "../ui/useResize";
import { isListCollapsed, useLayout } from "./layout";
import s from "./SectionLayout.module.css";

export function SectionLayout({ list, children }: { list: ReactNode; children: ReactNode }) {
  const { listWidth, setListWidth, toggleList } = useLayout();
  const collapsed = useLayout(isListCollapsed);
  const listId = useId();
  const aside = useRef<HTMLElement>(null);
  const handle = useRef<HTMLDivElement>(null);
  const expand = useRef<HTMLButtonElement>(null);
  // After the next collapse or expand, focus [목록 펼치기] or the list it opened.
  const focusNext = useRef<"expand" | "list">(undefined);
  const onPointerDown = useResize((start, current) =>
    setListWidth(listWidth + (current.x - start.x) / (Number(document.documentElement.style.zoom) || 1)),
  );
  // Check focus when the store changes, before React re-renders. After that, the element that had focus is
  // inert or gone and the browser has already dropped focus to the page.
  useEffect(
    () =>
      useLayout.subscribe((next, previous) => {
        const now = isListCollapsed(next);
        if (now === isListCollapsed(previous)) return;
        const active = document.activeElement;
        const inList = !!active && (!!aside.current?.contains(active) || active === handle.current);
        focusNext.current = undefined;
        // Any collapse (by the user or by a narrow frame) that hides the focused list or its separator.
        if (now && inList) focusNext.current = "expand";
        // Any reopen (click, Ctrl+Shift+B, or a wider frame) that removes the focused [목록 펼치기].
        if (!now && active && active === expand.current) focusNext.current = "list";
      }),
    [],
  );
  useEffect(() => {
    const target = focusNext.current;
    focusNext.current = undefined;
    if (target === "expand" && collapsed) expand.current?.focus();
    if (target === "list" && !collapsed) aside.current?.focus();
  }, [collapsed]);
  return (
    <div
      className={s.layout}
      style={{ gridTemplateColumns: collapsed ? "0 28px minmax(0, 1fr)" : `${listWidth}px 4px minmax(0, 1fr)` }}
    >
      <aside
        ref={aside}
        id={listId}
        tabIndex={-1}
        className={s.list}
        aria-label="목록"
        aria-hidden={collapsed}
        inert={collapsed}
      >
        {list}
      </aside>
      <div
        ref={handle}
        role="separator"
        aria-label="목록 너비"
        aria-orientation="vertical"
        aria-valuemin={180}
        aria-valuemax={480}
        aria-valuenow={listWidth}
        tabIndex={0}
        hidden={collapsed}
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
      {collapsed && (
        <div className={s.rail}>
          <IconButton
            ref={expand}
            label="목록 펼치기"
            icon={<PanelLeftOpen size={16} />}
            shortcut={["Ctrl", "Shift", "B"]}
            aria-expanded={false}
            aria-controls={listId}
            onClick={toggleList}
          />
        </div>
      )}
      <section className={s.body} aria-label="본문">
        {children}
      </section>
    </div>
  );
}
