import { useRef } from "react";
import { restoreFocus, trapDialogKeyDown } from "@devbox/a11y";
import { APP_SHORTCUTS, OTHER_SHORTCUTS, TERMINAL_SHORTCUTS } from "../lib/shortcuts";

interface ShortcutReferenceProps {
  open: boolean;
  onClose: () => void;
}

/** 표시 목록은 matcher와 같은 표에서 나온다 — `shortcutReference.test.ts`가 어긋남을 막는다. */
export default function ShortcutReference({ open, onClose }: ShortcutReferenceProps) {
  const dialogRef = useRef<HTMLElement>(null);
  const openerRef = useRef<HTMLElement | null>(null);
  if (open && !openerRef.current) {
    openerRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  }

  if (!open) return null;

  const close = () => {
    const opener = openerRef.current;
    openerRef.current = null;
    onClose();
    restoreFocus(opener);
  };

  return (
    <div
      className="dialog-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) close();
      }}
    >
      <section
        ref={dialogRef}
        className="shortcut-reference"
        role="dialog"
        aria-modal="true"
        aria-label="키보드 단축키"
        onKeyDown={(event) => {
          if (dialogRef.current) trapDialogKeyDown(event, dialogRef.current, close);
        }}
      >
        <h2 className="dialog-title">키보드 단축키</h2>

        <h3 className="shortcut-group-title">앱</h3>
        <dl className="shortcut-list">
          {APP_SHORTCUTS.map((shortcut) => (
            <div key={shortcut.id}>
              <dt><kbd>{shortcut.keys}</kbd></dt>
              <dd>{shortcut.label}</dd>
            </div>
          ))}
        </dl>

        <h3 className="shortcut-group-title">터미널</h3>
        <dl className="shortcut-list">
          {TERMINAL_SHORTCUTS.map((shortcut) => (
            <div key={shortcut.id}>
              <dt><kbd>{shortcut.keys}</kbd></dt>
              <dd>{shortcut.label}</dd>
            </div>
          ))}
        </dl>

        <h3 className="shortcut-group-title">그 외</h3>
        <dl className="shortcut-list">
          {OTHER_SHORTCUTS.map((shortcut) => (
            <div key={shortcut.keys}>
              <dt><kbd>{shortcut.keys}</kbd></dt>
              <dd>{shortcut.label}</dd>
            </div>
          ))}
        </dl>

        <div className="dialog-actions">
          <button type="button" className="btn primary" onClick={close}>닫기</button>
        </div>
      </section>
    </div>
  );
}
