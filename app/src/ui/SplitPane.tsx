import { type ReactNode, useRef } from "react";
import s from "./SplitPane.module.css";
import { useResize } from "./useResize";

export function SplitPane({
  dir,
  ratio,
  onRatio,
  min = 0.15,
  children,
  label = "분할 크기 조절",
}: {
  dir: "row" | "column";
  ratio: number;
  onRatio: (ratio: number) => void;
  min?: number;
  children: [ReactNode, ReactNode];
  label?: string;
}) {
  const box = useRef<HTMLDivElement>(null);
  const limit = Number.isFinite(min) ? Math.max(0, Math.min(0.5, min)) : 0.15;
  const clamp = (value: number) => Math.max(limit, Math.min(1 - limit, Number.isFinite(value) ? value : 0.5));
  const value = clamp(ratio);
  const resize = useResize((_start, current) => {
    const rect = box.current?.getBoundingClientRect();
    if (!rect) return;
    const length = dir === "row" ? rect.width : rect.height;
    if (!Number.isFinite(length) || length <= 0) return;
    const offset = dir === "row" ? current.x - rect.left : current.y - rect.top;
    if (Number.isFinite(offset)) onRatio(clamp(offset / length));
  });
  return (
    <div ref={box} className={s.split} data-dir={dir} style={{ "--ratio": value } as React.CSSProperties}>
      <div className={s.pane}>{children[0]}</div>
      <div
        role="separator"
        aria-label={label}
        aria-orientation={dir === "row" ? "vertical" : "horizontal"}
        aria-valuemin={Math.round(limit * 100)}
        aria-valuemax={Math.round((1 - limit) * 100)}
        aria-valuenow={Math.round(value * 100)}
        tabIndex={0}
        className={s.handle}
        onPointerDown={resize}
        onKeyDown={(event) => {
          const next =
            event.key === "Home"
              ? limit
              : event.key === "End"
                ? 1 - limit
                : event.key === (dir === "row" ? "ArrowLeft" : "ArrowUp")
                  ? value - 0.05
                  : event.key === (dir === "row" ? "ArrowRight" : "ArrowDown")
                    ? value + 0.05
                    : undefined;
          if (next === undefined) return;
          event.preventDefault();
          onRatio(clamp(next));
        }}
      />
      <div className={s.pane}>{children[1]}</div>
    </div>
  );
}
