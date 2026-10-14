import { X } from "lucide-react";
import { type ReactNode, useRef } from "react";
import s from "./Tabs.module.css";

export interface TabItem {
  id: string;
  label: string;
  badge?: ReactNode;
  closable?: boolean;
}
export function Tabs({
  items,
  active,
  onSelect,
  onClose,
  actions,
  label = "본문 탭",
}: {
  items: TabItem[];
  active?: string;
  onSelect: (id: string) => void;
  onClose?: (id: string) => void;
  actions?: ReactNode;
  label?: string;
}) {
  const buttons = useRef(new Map<string, HTMLButtonElement>());
  const focusId = items.some((item) => item.id === active) ? active : items[0]?.id;
  return (
    <div className={s.bar}>
      <div role="tablist" aria-label={label} className={s.list}>
        {items.map((item, index) => (
          <div key={item.id} className={s.tab} data-active={item.id === active || undefined}>
            <button
              type="button"
              role="tab"
              aria-selected={item.id === active}
              tabIndex={item.id === focusId ? 0 : -1}
              ref={(node) => {
                if (node) buttons.current.set(item.id, node);
                else buttons.current.delete(item.id);
              }}
              onClick={() => onSelect(item.id)}
              onAuxClick={(event) => {
                if (event.button === 1 && item.closable && onClose) {
                  event.preventDefault();
                  onClose(item.id);
                }
              }}
              onKeyDown={(event) => {
                const next =
                  event.key === "Home"
                    ? 0
                    : event.key === "End"
                      ? items.length - 1
                      : event.key === "ArrowRight"
                        ? (index + 1) % items.length
                        : event.key === "ArrowLeft"
                          ? (index - 1 + items.length) % items.length
                          : undefined;
                if (next === undefined) return;
                const target = items[next];
                if (!target) return;
                event.preventDefault();
                const id = target.id;
                onSelect(id);
                buttons.current.get(id)?.focus();
              }}
            >
              {item.label}
              {item.badge}
            </button>
            {item.closable && onClose && (
              <button
                type="button"
                className={s.close}
                aria-label={`${item.label} 닫기`}
                onClick={() => onClose(item.id)}
              >
                <X size={12} aria-hidden="true" />
              </button>
            )}
          </div>
        ))}
      </div>
      {actions && <div className={s.actions}>{actions}</div>}
    </div>
  );
}
