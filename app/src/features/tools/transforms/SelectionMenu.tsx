import { type ReactNode, useEffect, useRef, useState } from "react";
import s from "./SelectionMenu.module.css";

export function SelectionMenu({
  children,
  getSelection,
  onSend,
  label,
  className,
  dataKeyscope,
}: {
  children: ReactNode;
  getSelection: (element: HTMLElement) => string;
  onSend: (text: string) => void;
  label: string;
  className?: string;
  dataKeyscope?: string;
}) {
  const [selection, setSelection] = useState<{ text: string; x: number; y: number } | null>(null);
  const host = useRef<HTMLDivElement>(null);
  const item = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  const close = (restore: boolean) => {
    setSelection(null);
    if (restore) (previousFocus.current?.isConnected ? previousFocus.current : host.current)?.focus();
  };
  useEffect(() => {
    if (!selection) return;
    item.current?.focus();
    const dismiss = (event: PointerEvent) => {
      if (!menu.current?.contains(event.target as Node)) setSelection(null);
    };
    const blur = () => setSelection(null);
    document.addEventListener("pointerdown", dismiss);
    window.addEventListener("blur", blur);
    return () => {
      document.removeEventListener("pointerdown", dismiss);
      window.removeEventListener("blur", blur);
    };
  }, [selection]);
  const open = (event: { preventDefault(): void; stopPropagation(): void }, x: number, y: number) => {
    const text = host.current ? getSelection(host.current) : "";
    if (!text) return;
    event.preventDefault();
    event.stopPropagation();
    previousFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : host.current;
    setSelection({
      text,
      x: Math.max(0, Math.min(x, window.innerWidth - 220)),
      y: Math.max(0, Math.min(y, window.innerHeight - 48)),
    });
  };
  return (
    <div
      ref={host}
      role="group"
      aria-label={label}
      tabIndex={0}
      className={className}
      data-keyscope={dataKeyscope}
      onContextMenu={(event) => open(event, event.clientX, event.clientY)}
      onKeyDownCapture={(event) => {
        if (selection) {
          if (event.key === "Escape") {
            event.preventDefault();
            event.stopPropagation();
            close(true);
          } else if (event.key === "Tab") close(false);
          else if (["ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) {
            event.preventDefault();
            item.current?.focus();
          }
        } else if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
          const rect = event.currentTarget.getBoundingClientRect();
          open(event, rect.left, rect.top);
        }
      }}
    >
      {children}
      {selection && (
        <div
          ref={menu}
          role="menu"
          aria-label="선택 작업"
          className={s.menu}
          style={{ left: selection.x, top: selection.y }}
        >
          <button
            ref={item}
            role="menuitem"
            type="button"
            onClick={() => {
              const text = selection.text;
              close(true);
              onSend(text);
            }}
          >
            변환 도구로 보내기
          </button>
        </div>
      )}
    </div>
  );
}
