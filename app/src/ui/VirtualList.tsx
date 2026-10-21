import { useVirtualizer } from "@tanstack/react-virtual";
import { type ReactNode, useLayoutEffect, useRef } from "react";
import s from "./VirtualList.module.css";

export function VirtualList<T>({
  items,
  renderItem,
  follow = false,
  onAtBottomChange,
  label = "목록",
  rowHeight = 24,
}: {
  items: T[];
  renderItem: (item: T, index: number) => ReactNode;
  follow?: boolean;
  onAtBottomChange?: (atBottom: boolean) => void;
  label?: string;
  rowHeight?: number;
}) {
  const parent = useRef<HTMLDivElement>(null);
  const virtual = useVirtualizer({
    count: items.length,
    getScrollElement: () => parent.current,
    estimateSize: () => rowHeight,
    overscan: 8,
    initialRect: { width: 800, height: 320 },
  });
  useLayoutEffect(() => {
    if (follow && items.length) virtual.scrollToIndex(items.length - 1, { align: "end" });
  }, [follow, items, virtual]);
  return (
    <div
      ref={parent}
      role="log"
      aria-label={label}
      aria-live="off"
      tabIndex={0}
      className={s.viewport}
      onScroll={(event) => {
        const el = event.currentTarget;
        onAtBottomChange?.(el.scrollHeight - el.scrollTop - el.clientHeight <= rowHeight);
      }}
    >
      <div className={s.content} style={{ height: virtual.getTotalSize() }}>
        {virtual.getVirtualItems().map((row) => (
          <div key={row.key} className={s.row} style={{ height: row.size, transform: `translateY(${row.start}px)` }}>
            {renderItem(items[row.index]!, row.index)}
          </div>
        ))}
      </div>
    </div>
  );
}
