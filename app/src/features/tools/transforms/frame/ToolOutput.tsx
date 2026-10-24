import { type ReactNode, type RefObject, useRef, useState } from "react";
import { downloadTextResult } from "./ToolActions";
import { useToolAction } from "./useToolAction";

interface Props {
  value: string;
  children?: ReactNode;
  className: string;
  ariaLabel?: string;
  menuLabel?: string;
  downloadName?: string;
  actionErrorMessage?: string;
  fixedActionError?: string;
  busy?: boolean;
  onBusyChange?: (busy: boolean) => void;
  asDiv?: boolean;
}
export function ToolOutput({
  value,
  children,
  className,
  ariaLabel = "출력",
  menuLabel = "출력 작업",
  downloadName = "devbox-result.txt",
  actionErrorMessage,
  fixedActionError,
  busy = false,
  onBusyChange,
  asDiv = false,
}: Props) {
  const ref = useRef<HTMLElement>(null);
  const [open, setOpen] = useState(false);
  const action = useToolAction(value, actionErrorMessage ?? fixedActionError, onBusyChange);
  const disabled = !value || busy || action.busy;
  const select = () => {
    setOpen(false);
    if (!ref.current) return;
    ref.current.focus();
    const range = document.createRange();
    range.selectNodeContents(ref.current);
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
  };
  const run = (kind: "copy" | "save") => {
    setOpen(false);
    if (disabled) return;
    void action.run(() =>
      kind === "copy" ? navigator.clipboard.writeText(value) : downloadTextResult(value, downloadName),
    );
  };
  const content = children ?? (value || " ");
  return (
    <div className="tool-output-control">
      {asDiv ? (
        <div ref={ref as RefObject<HTMLDivElement | null>} className={className} tabIndex={0} aria-label={ariaLabel}>
          {content}
        </div>
      ) : (
        <pre
          ref={ref as RefObject<HTMLPreElement | null>}
          role="region"
          className={className}
          tabIndex={0}
          aria-label={ariaLabel}
        >
          {content}
        </pre>
      )}
      <button
        type="button"
        className="tool-actions-toggle"
        onClick={() => setOpen((o) => !o)}
        aria-label={`${menuLabel} 열기`}
      >
        출력 작업
      </button>
      {open && (
        <div role="group" aria-label={menuLabel} data-tool-actions="true">
          <button type="button" disabled={disabled} aria-disabled={disabled} onClick={() => run("copy")}>
            복사
          </button>
          <button type="button" disabled={disabled} aria-disabled={disabled} onClick={select}>
            모두 선택
          </button>
          <button type="button" disabled={disabled} aria-disabled={disabled} onClick={() => run("save")}>
            결과 파일 저장
          </button>
        </div>
      )}
      {action.error && <div role="alert">{action.error}</div>}
    </div>
  );
}
