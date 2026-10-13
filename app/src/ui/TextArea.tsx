import { type ComponentPropsWithRef, useLayoutEffect, useRef } from "react";
import s from "./TextArea.module.css";

export function TextArea({
  maxRows = 12,
  className,
  ref,
  onInput,
  ...rest
}: ComponentPropsWithRef<"textarea"> & { maxRows?: number }) {
  const inner = useRef<HTMLTextAreaElement | null>(null);
  const resize = () => {
    const element = inner.current;
    if (!element) return;
    const style = getComputedStyle(element);
    const line = Number.parseFloat(style.lineHeight) || 20;
    const padding = (Number.parseFloat(style.paddingTop) || 6) + (Number.parseFloat(style.paddingBottom) || 6);
    const border = (Number.parseFloat(style.borderTopWidth) || 0) + (Number.parseFloat(style.borderBottomWidth) || 0);
    const rows = Number.isFinite(maxRows) ? Math.max(1, maxRows) : 12;
    element.style.height = "auto";
    element.style.height = `${Math.min(element.scrollHeight + border, line * rows + padding + border)}px`;
  };
  useLayoutEffect(resize);
  return (
    <textarea
      {...rest}
      ref={(element) => {
        inner.current = element;
        if (typeof ref === "function") return ref(element);
        if (ref) ref.current = element;
      }}
      className={[s.area, className].filter(Boolean).join(" ")}
      onInput={(event) => {
        resize();
        onInput?.(event);
      }}
    />
  );
}
