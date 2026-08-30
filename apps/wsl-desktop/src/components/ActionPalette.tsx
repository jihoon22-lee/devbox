import { useEffect, useMemo, useRef, useState } from "react";
import { isImeComposing, restoreFocus, trapDialogKeyDown } from "@devbox/a11y";

export interface PaletteAction {
  id: string;
  label: string;
  description?: string;
  danger?: boolean;
  run: () => void;
}

interface ActionPaletteProps {
  open: boolean;
  actions: readonly PaletteAction[];
  onClose: () => void;
}

export default function ActionPalette({ open, actions, onClose }: ActionPaletteProps) {
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const dialogRef = useRef<HTMLElement>(null);
  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase("ko-KR");
    return needle
      ? actions.filter((action) => `${action.label} ${action.description ?? ""}`.toLocaleLowerCase("ko-KR").includes(needle))
      : [...actions];
  }, [actions, query]);

  useEffect(() => {
    if (!open) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setQuery("");
    setSelectedIndex(0);
    const frame = requestAnimationFrame(() => inputRef.current?.focus());
    return () => {
      cancelAnimationFrame(frame);
      restoreFocus(opener);
    };
  }, [open]);

  useEffect(() => {
    if (selectedIndex >= filtered.length) setSelectedIndex(Math.max(0, filtered.length - 1));
  }, [filtered.length, selectedIndex]);

  if (!open) return null;

  const run = (action: PaletteAction | undefined) => {
    if (!action) return;
    onClose();
    action.run();
  };

  return (
    <div className="palette-backdrop" role="presentation" onMouseDown={(event) => {
      if (event.target === event.currentTarget) onClose();
    }}>
      <section
        ref={dialogRef}
        className="action-palette"
        role="dialog"
        aria-modal="true"
        aria-label="WSL Desktop 명령 팔레트"
        onKeyDown={(event) => {
          if (dialogRef.current) trapDialogKeyDown(event, dialogRef.current, onClose);
        }}
      >
        <input
          ref={inputRef}
          aria-label="명령 검색"
          placeholder="명령 검색…"
          value={query}
          onChange={(event) => {
            setQuery(event.currentTarget.value);
            setSelectedIndex(0);
          }}
          onKeyDown={(event) => {
            if (isImeComposing(event)) return;
            if (event.key === "ArrowDown") {
              event.preventDefault();
              setSelectedIndex((index) => Math.min(filtered.length - 1, index + 1));
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              setSelectedIndex((index) => Math.max(0, index - 1));
            } else if (event.key === "Enter") {
              event.preventDefault();
              run(filtered[selectedIndex]);
            }
          }}
        />
        <div className="palette-list" role="listbox" aria-label="실행할 명령">
          {filtered.map((action, index) => (
            <button
              key={action.id}
              type="button"
              role="option"
              aria-selected={index === selectedIndex}
              className={`${index === selectedIndex ? "selected" : ""} ${action.danger ? "danger" : ""}`}
              onMouseEnter={() => setSelectedIndex(index)}
              onClick={() => run(action)}
            >
              <span>{action.label}</span>
              {action.description && <small>{action.description}</small>}
            </button>
          ))}
          {filtered.length === 0 && <div className="palette-empty">일치하는 명령이 없습니다.</div>}
        </div>
      </section>
    </div>
  );
}
