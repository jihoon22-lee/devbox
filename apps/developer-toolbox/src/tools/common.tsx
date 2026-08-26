import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type InputHTMLAttributes,
  type ReactNode,
  type RefObject,
  type TextareaHTMLAttributes,
} from "react";
import { readClipboardText } from "../api";

/** async 변환 결과를 입력 변경 시 자동 계산하는 훅 */
export function useAsyncTransform(
  input: string,
  run: (input: string) => Promise<{ output: string; error?: string }>,
  options: { clearOutputOnStart?: boolean } = {},
) {
  const [output, setOutput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const seq = useRef(0);
  const clearOutputOnStart = options.clearOutputOnStart ?? false;

  useEffect(() => {
    const current = ++seq.current;
    if (clearOutputOnStart) {
      setOutput("");
      setError(null);
    }
    setRunning(true);
    run(input)
      .then((res) => {
        if (seq.current !== current) return;
        setOutput(res.output);
        setError(res.error ?? null);
      })
      .catch((e) => {
        if (seq.current !== current) return;
        setError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (seq.current === current) setRunning(false);
      });
    return () => {
      if (seq.current === current) seq.current += 1;
    };
  }, [clearOutputOnStart, input, run]);

  return { output, error, running };
}

type TextControl = HTMLInputElement | HTMLTextAreaElement;
type Selection = { start: number; end: number };

const INPUT_MENU_ITEMS: readonly ContextMenuEntry[] = [
  { type: "item", id: "paste", label: "Paste" },
  { type: "item", id: "select-all", label: "Select all" },
  { type: "separator", id: "input-separator" },
  { type: "item", id: "clear", label: "Clear" },
];

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function nextFrame(action: () => void): void {
  if (typeof requestAnimationFrame === "function") requestAnimationFrame(action);
  else setTimeout(action, 0);
}

function useEditableTextContextMenu(
  value: string,
  onValueChange: (value: string) => void,
) {
  const controlRef = useRef<TextControl>(null);
  const selection = useRef<Selection>({ start: value.length, end: value.length });
  const [actionError, setActionError] = useState<string | null>(null);

  const menu = useContextMenu({
    onBeforeOpen: (_reason, target) => {
      if (!(target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement)) return;
      controlRef.current = target;
      selection.current = {
        start: target.selectionStart ?? target.value.length,
        end: target.selectionEnd ?? target.value.length,
      };
      setActionError(null);
    },
  });

  const items = useMemo<readonly ContextMenuEntry[]>(
    () =>
      INPUT_MENU_ITEMS.map((item) =>
        item.type === "item" && (item.id === "select-all" || item.id === "clear")
          ? { ...item, disabled: value.length === 0 }
          : item,
      ),
    [value.length],
  );

  const focusAt = useCallback((position: number) => {
    nextFrame(() => {
      const target = controlRef.current;
      if (!target?.isConnected) return;
      target.focus({ preventScroll: true });
      target.setSelectionRange(position, position);
    });
  }, []);

  const onSelect = useCallback(
    (id: string) => {
      const target = controlRef.current;
      if (!target) return;
      if (id === "select-all") {
        target.focus({ preventScroll: true });
        target.select();
        return;
      }
      if (id === "clear") {
        onValueChange("");
        focusAt(0);
        return;
      }
      if (id !== "paste") return;

      const captured = selection.current;
      void readClipboardText()
        .then((clipboard) => {
          const current = controlRef.current?.value ?? value;
          const start = Math.min(captured.start, current.length);
          const end = Math.min(Math.max(captured.end, start), current.length);
          const next = `${current.slice(0, start)}${clipboard}${current.slice(end)}`;
          onValueChange(next);
          setActionError(null);
          focusAt(start + clipboard.length);
        })
        .catch((error) => setActionError(`Clipboard read failed: ${message(error)}`));
    },
    [focusAt, onValueChange, value],
  );

  return { actionError, controlRef, items, menu, onSelect };
}

interface ToolTextAreaProps
  extends Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, "value" | "onChange"> {
  value: string;
  onValueChange: (value: string) => void;
  menuLabel?: string;
}

/** Controlled textarea with the exact Toolbox input menu. */
export function ToolTextArea({
  value,
  onValueChange,
  menuLabel = "Input actions",
  ...props
}: ToolTextAreaProps) {
  const context = useEditableTextContextMenu(value, onValueChange);
  return (
    <>
      <textarea
        {...props}
        {...context.menu.triggerProps}
        ref={context.controlRef as RefObject<HTMLTextAreaElement | null>}
        value={value}
        onChange={(event) => onValueChange(event.currentTarget.value)}
      />
      <ContextMenu
        open={context.menu.open}
        anchor={context.menu.anchor}
        restoreFocusTo={context.menu.restoreFocusTo}
        items={context.items}
        onSelect={context.onSelect}
        onClose={context.menu.close}
        ariaLabel={menuLabel}
      />
      {context.actionError ? (
        <div className="context-action-error" role="alert">
          {context.actionError}
        </div>
      ) : null}
    </>
  );
}

interface ToolTextFieldProps
  extends Omit<InputHTMLAttributes<HTMLInputElement>, "type" | "value" | "onChange"> {
  value: string;
  onValueChange: (value: string) => void;
  menuLabel?: string;
}

/** Controlled single-line text field with the same app-owned input menu. */
export function ToolTextField({
  value,
  onValueChange,
  menuLabel = "Input actions",
  ...props
}: ToolTextFieldProps) {
  const context = useEditableTextContextMenu(value, onValueChange);
  return (
    <>
      <input
        {...props}
        {...context.menu.triggerProps}
        ref={context.controlRef as RefObject<HTMLInputElement | null>}
        type="text"
        value={value}
        onChange={(event) => onValueChange(event.currentTarget.value)}
      />
      <ContextMenu
        open={context.menu.open}
        anchor={context.menu.anchor}
        restoreFocusTo={context.menu.restoreFocusTo}
        items={context.items}
        onSelect={context.onSelect}
        onClose={context.menu.close}
        ariaLabel={menuLabel}
      />
      {context.actionError ? (
        <div className="context-action-error" role="alert">
          {context.actionError}
        </div>
      ) : null}
    </>
  );
}

export function downloadTextResult(value: string, filename: string): void {
  const url = URL.createObjectURL(new Blob([value], { type: "text/plain;charset=utf-8" }));
  try {
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = filename;
    anchor.click();
  } finally {
    URL.revokeObjectURL(url);
  }
}

interface ToolOutputProps {
  value: string;
  children?: ReactNode;
  className: string;
  ariaLabel?: string;
  menuLabel?: string;
  downloadName?: string;
  asDiv?: boolean;
}

/** Read-only result surface with copy/select/save actions. */
export function ToolOutput({
  value,
  children,
  className,
  ariaLabel = "Output",
  menuLabel = "Output actions",
  downloadName = "dev-toolbox-result.txt",
  asDiv = false,
}: ToolOutputProps) {
  const outputRef = useRef<HTMLElement>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const menu = useContextMenu({ onBeforeOpen: () => setActionError(null) });
  const items = useMemo<readonly ContextMenuEntry[]>(
    () => [
      { type: "item", id: "copy", label: "Copy", disabled: value.length === 0 },
      { type: "item", id: "select-all", label: "Select all", disabled: value.length === 0 },
      { type: "item", id: "save", label: "Save result file", disabled: value.length === 0 },
    ],
    [value.length],
  );

  const onSelect = (id: string) => {
    if (!value) return;
    if (id === "select-all") {
      const output = outputRef.current;
      if (!output) return;
      output.focus({ preventScroll: true });
      const selection = window.getSelection();
      const range = document.createRange();
      range.selectNodeContents(output);
      selection?.removeAllRanges();
      selection?.addRange(range);
      return;
    }

    const action = Promise.resolve().then(() => {
      if (id === "copy") return navigator.clipboard.writeText(value);
      if (id === "save") downloadTextResult(value, downloadName);
    });
    void action
      .then(() => setActionError(null))
      .catch((error) => setActionError(`Output action failed: ${message(error)}`));
  };

  const content = (children ?? value) || " ";
  const trigger = {
    ...menu.triggerProps,
    tabIndex: 0,
    "aria-label": ariaLabel,
    className,
  };

  return (
    <>
      {asDiv ? (
        <div {...trigger} ref={outputRef as RefObject<HTMLDivElement | null>}>
          {content}
        </div>
      ) : (
        <pre {...trigger} ref={outputRef as RefObject<HTMLPreElement | null>}>
          {content}
        </pre>
      )}
      <ContextMenu
        open={menu.open}
        anchor={menu.anchor}
        restoreFocusTo={menu.restoreFocusTo}
        items={items}
        onSelect={onSelect}
        onClose={menu.close}
        ariaLabel={menuLabel}
      />
      {actionError ? (
        <div className="context-action-error" role="alert">
          {actionError}
        </div>
      ) : null}
    </>
  );
}

/** 입력/출력 2분할 + 복사 버튼을 갖춘 범용 변환 도구 */
export function TransformerTool({
  placeholder,
  run,
  extra,
  rows = 8,
  clearOutputOnInput = false,
}: {
  placeholder: string;
  run: (input: string) => Promise<{ output: string; error?: string }>;
  extra?: React.ReactNode;
  rows?: number;
  /** Clear the previous result while a new bounded transform is running. */
  clearOutputOnInput?: boolean;
}) {
  const [input, setInput] = useState("");
  const { output, error, running } = useAsyncTransform(input, run, {
    clearOutputOnStart: clearOutputOnInput,
  });
  const displayedOutput = error || output;

  return (
    <div className="tool">
      {extra}
      <div className="io-grid">
        <div className="io-col">
          <div className="io-label">Input</div>
          <ToolTextArea
            aria-label="Input"
            aria-busy={running}
            className="io-input"
            placeholder={placeholder}
            rows={rows}
            value={input}
            onValueChange={setInput}
            spellCheck={false}
          />
        </div>
        <div className="io-col">
          <div className="io-label">
            Output {running && <span className="dim" role="status" aria-live="polite">(running...)</span>}
            {output && !error && (
              <button
                className="copy-btn"
                onClick={() => navigator.clipboard.writeText(output)}
              >
                Copy
              </button>
            )}
          </div>
          <ToolOutput
            className={`io-output ${error ? "io-error" : ""}`}
            value={displayedOutput}
          />
        </div>
      </div>
    </div>
  );
}

export function CopyBtn({ value }: { value: string }) {
  if (!value) return null;
  return (
    <button className="copy-btn" onClick={() => navigator.clipboard.writeText(value)}>
      Copy
    </button>
  );
}
