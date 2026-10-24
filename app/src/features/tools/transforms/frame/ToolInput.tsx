import {
  type InputHTMLAttributes,
  type RefObject,
  type TextareaHTMLAttributes,
  useEffect,
  useRef,
  useState,
} from "react";
import { readClipboardText } from "../lib/browser";

interface Shared {
  value: string;
  onValueChange: (value: string) => void;
  menuLabel?: string;
  clipboardErrorMessage?: string;
  actionErrorMessage?: string;
  fixedActionError?: string;
  maxPasteBytes?: number;
}
function useInputActions(value: string, onValueChange: (text: string) => void, limit: number, fixedError?: string) {
  const control = useRef<HTMLInputElement | HTMLTextAreaElement>(null);
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const mounted = useRef(true),
    revision = useRef(0),
    running = useRef(false);
  const current = useRef(value);
  current.current = value;
  const selected = useRef({ start: value.length, end: value.length });
  // biome-ignore lint/correctness/useExhaustiveDependencies: changing the value invalidates pending clipboard/output actions.
  useEffect(() => {
    revision.current += 1;
    setError(null);
  }, [value]);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      revision.current += 1;
    };
  }, []);
  const focus = (position: number) =>
    requestAnimationFrame(() => {
      if (control.current?.isConnected) {
        control.current.focus();
        control.current.setSelectionRange(position, position);
      }
    });
  const toggle = () => {
    const el = control.current;
    if (el) selected.current = { start: el.selectionStart ?? value.length, end: el.selectionEnd ?? value.length };
    setOpen((o) => !o);
  };
  const action = (id: "paste" | "select" | "clear") => {
    setOpen(false);
    const el = control.current;
    if (!el) return;
    if (id === "select") {
      el.focus();
      el.setSelectionRange(0, value.length);
      return;
    }
    if (id === "clear") {
      onValueChange("");
      focus(0);
      return;
    }
    if (running.current) return;
    const token = ++revision.current,
      snapshot = value,
      { start, end } = selected.current;
    running.current = true;
    setPending(true);
    setError(null);
    void Promise.resolve()
      .then(() => {
        if (mounted.current && token === revision.current) return readClipboardText();
      })
      .then((text) => {
        if (text === undefined || !mounted.current || token !== revision.current || current.current !== snapshot)
          return;
        const before = snapshot.slice(0, start),
          after = snapshot.slice(end);
        let remaining = Math.max(0, limit - new TextEncoder().encode(before + after).length),
          insert = "";
        for (const char of text) {
          const first = char.charCodeAt(0);
          if (char.length === 1 && first >= 0xd800 && first <= 0xdfff) break;
          const bytes = new TextEncoder().encode(char).length;
          if (bytes > remaining) break;
          insert += char;
          remaining -= bytes;
        }
        onValueChange(before + insert + after);
        focus(before.length + insert.length);
      })
      .catch((caught) => {
        if (mounted.current && token === revision.current)
          setError(
            fixedError ?? `클립보드를 읽지 못했습니다: ${caught instanceof Error ? caught.message : String(caught)}`,
          );
      })
      .finally(() => {
        running.current = false;
        if (mounted.current) setPending(false);
      });
  };
  return { control, open, toggle, error, pending, action };
}
function InputActions({
  state,
  label,
  disabled,
}: {
  state: ReturnType<typeof useInputActions>;
  label: string;
  disabled?: boolean;
}) {
  return (
    <>
      <button
        type="button"
        className="tool-actions-toggle"
        aria-label={`${label} 열기`}
        disabled={disabled}
        onClick={state.toggle}
      >
        입력 작업
      </button>
      {state.open && (
        <div role="group" aria-label={label} data-tool-actions="true">
          <button type="button" disabled={state.pending} onClick={() => state.action("paste")}>
            붙여넣기
          </button>
          <button type="button" onClick={() => state.action("select")}>
            모두 선택
          </button>
          <button type="button" onClick={() => state.action("clear")}>
            지우기
          </button>
        </div>
      )}
      {state.error && <div role="alert">{state.error}</div>}
    </>
  );
}
export function ToolTextArea({
  value,
  onValueChange,
  menuLabel = "입력 작업",
  clipboardErrorMessage,
  actionErrorMessage,
  fixedActionError,
  maxPasteBytes = 1024 * 1024,
  ...props
}: Shared & Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, "value" | "onChange">) {
  const state = useInputActions(
    value,
    onValueChange,
    maxPasteBytes,
    clipboardErrorMessage ?? actionErrorMessage ?? fixedActionError,
  );
  return (
    <div className="tool-text-control">
      <textarea
        {...props}
        ref={state.control as RefObject<HTMLTextAreaElement | null>}
        value={value}
        onChange={(e) => onValueChange(e.currentTarget.value)}
      />
      <InputActions state={state} label={menuLabel} disabled={props.disabled} />
    </div>
  );
}
export function ToolTextField({
  value,
  onValueChange,
  menuLabel = "입력 작업",
  clipboardErrorMessage,
  actionErrorMessage,
  fixedActionError,
  maxPasteBytes = 1024 * 1024,
  inputType,
  type,
  ...props
}: Shared & Omit<InputHTMLAttributes<HTMLInputElement>, "value" | "onChange"> & { inputType?: "text" | "password" }) {
  const state = useInputActions(
    value,
    onValueChange,
    maxPasteBytes,
    clipboardErrorMessage ?? actionErrorMessage ?? fixedActionError,
  );
  return (
    <div className="tool-text-control">
      <input
        {...props}
        ref={state.control as RefObject<HTMLInputElement | null>}
        type={type ?? inputType ?? "text"}
        value={value}
        onChange={(e) => onValueChange(e.currentTarget.value)}
      />
      <InputActions state={state} label={menuLabel} disabled={props.disabled} />
    </div>
  );
}
