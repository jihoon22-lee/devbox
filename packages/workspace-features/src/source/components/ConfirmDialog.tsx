import { useLayoutEffect, useRef, type KeyboardEvent } from "react";

interface Props {
  title: string;
  summary: readonly string[];
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}

const FOCUSABLE_SELECTOR = [
  "button:not([disabled])",
  "[href]",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

/**
 * A small, app-owned confirmation surface for destructive or remote actions.
 *
 * It intentionally renders only caller-provided, already-safe summary lines.
 * The dialog owns the keyboard boundary and restores focus when it closes so
 * each mutation panel does not need to duplicate those accessibility rules.
 */
export default function ConfirmDialog({
  title,
  summary,
  confirmLabel,
  onConfirm,
  onCancel,
}: Props) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const restoreFocusRef = useRef<HTMLElement | null>(
    typeof document !== "undefined" && document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null,
  );
  const titleIdRef = useRef(`repo-confirm-title-${Math.random().toString(36).slice(2)}`);
  const descriptionIdRef = useRef(`repo-confirm-description-${Math.random().toString(36).slice(2)}`);

  useLayoutEffect(() => {
    const cancelButton = dialogRef.current?.querySelector<HTMLButtonElement>(
      "[data-confirm-cancel]",
    );
    cancelButton?.focus();

    return () => {
      const element = restoreFocusRef.current;
      if (element?.isConnected && !element.hasAttribute("disabled")) element.focus();
    };
  }, []);

  const onDialogKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onCancel();
      return;
    }
    if (event.key !== "Tab") return;

    const focusable = [...(dialogRef.current?.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR) ?? [])];
    if (focusable.length === 0) {
      event.preventDefault();
      dialogRef.current?.focus();
      return;
    }
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    if (event.shiftKey ? active === first || !dialogRef.current?.contains(active) : active === last) {
      event.preventDefault();
      (event.shiftKey ? last : first).focus();
    }
  };

  return (
    <div className="confirm-dialog-backdrop">
      <div
        ref={dialogRef}
        className="confirm-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleIdRef.current}
        aria-describedby={descriptionIdRef.current}
        tabIndex={-1}
        onKeyDown={onDialogKeyDown}
      >
        <h2 id={titleIdRef.current}>{title}</h2>
        <div id={descriptionIdRef.current} className="confirm-dialog-summary">
          {summary.map((line, index) => <p key={`${index}:${line}`}>{line}</p>)}
        </div>
        <div className="confirm-dialog-actions">
          <button type="button" className="btn" data-confirm-cancel onClick={onCancel}>
            취소
          </button>
          <button type="button" className="btn primary" onClick={onConfirm}>
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
