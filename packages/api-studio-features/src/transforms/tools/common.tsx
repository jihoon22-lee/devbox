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
import { ApiHandoffAction } from "./ApiHandoffAction";
import { KnowledgeDraftAction } from "./KnowledgeDraftAction";

/** async 변환 결과를 입력 변경 시 자동 계산하는 훅 */
export function useAsyncTransform(
  input: string,
  run: (input: string, signal?: AbortSignal) => Promise<{ output: string; error?: string }>,
  options: { clearOutputOnStart?: boolean } = {},
) {
  const [output, setOutput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const seq = useRef(0);
  const clearOutputOnStart = options.clearOutputOnStart ?? false;

  useEffect(() => {
    const current = ++seq.current;
    const controller = typeof AbortController === "function" ? new AbortController() : null;
    if (clearOutputOnStart) {
      setOutput("");
      setError(null);
    }
    setRunning(true);
    let request: Promise<{ output: string; error?: string }>;
    try {
      request = Promise.resolve(run(input, controller?.signal));
    } catch (error) {
      request = Promise.reject(error);
    }
    request
      .then((res) => {
        if (controller?.signal.aborted || seq.current !== current) return;
        setOutput(res.output);
        setError(res.error ?? null);
      })
      .catch((e) => {
        if (controller?.signal.aborted || seq.current !== current) return;
        setError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (!controller?.signal.aborted && seq.current === current) setRunning(false);
      });
    return () => {
      controller?.abort();
      if (seq.current === current) seq.current += 1;
    };
  }, [clearOutputOnStart, input, run]);

  return { output, error, running };
}

type TextControl = HTMLInputElement | HTMLTextAreaElement;
type Selection = { start: number; end: number };

const INPUT_MENU_ITEMS: readonly ContextMenuEntry[] = [
  { type: "item", id: "paste", label: "붙여넣기" },
  { type: "item", id: "select-all", label: "모두 선택" },
  { type: "separator", id: "input-separator" },
  { type: "item", id: "clear", label: "지우기" },
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
  options: {
    clipboardErrorMessage?: string;
    maxPasteBytes?: number;
  } = {},
) {
  const controlRef = useRef<TextControl>(null);
  const selection = useRef<Selection>({ start: value.length, end: value.length });
  const [actionError, setActionError] = useState<string | null>(null);
  const mounted = useRef(true);
  const actionRevision = useRef(0);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      actionRevision.current += 1;
    };
  }, []);

  useEffect(() => {
    actionRevision.current += 1;
    setActionError(null);
  }, [value]);

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
        actionRevision.current += 1;
        target.focus({ preventScroll: true });
        target.select();
        return;
      }
      if (id === "clear") {
        actionRevision.current += 1;
        onValueChange("");
        focusAt(0);
        return;
      }
      if (id !== "paste") return;

      const captured = selection.current;
      const capturedValue = target.value;
      const revision = ++actionRevision.current;
      void Promise.resolve()
        .then(() => readClipboardText())
        .then((clipboard) => {
          const currentTarget = controlRef.current;
          if (
            !mounted.current
            || actionRevision.current !== revision
            || !currentTarget?.isConnected
            || currentTarget.value !== capturedValue
          ) return;
          const current = currentTarget.value;
          const start = Math.min(captured.start, current.length);
          const end = Math.min(Math.max(captured.end, start), current.length);
          const maxLength = currentTarget.maxLength;
          const availableCodeUnits = maxLength >= 0
            ? Math.max(0, maxLength - (current.length - (end - start)))
            : Number.POSITIVE_INFINITY;
          const currentBytes = utf8ByteLength(current);
          const selectedBytes = utf8ByteLength(current.slice(start, end));
          const maxBytes = options.maxPasteBytes ?? Number.POSITIVE_INFINITY;
          const availableBytes = Number.isFinite(maxBytes)
            ? Math.max(0, maxBytes - (currentBytes - selectedBytes))
            : Number.POSITIVE_INFINITY;
          const inserted = takeUtf8Prefix(clipboard, availableBytes, availableCodeUnits);
          const next = `${current.slice(0, start)}${inserted}${current.slice(end)}`;
          onValueChange(next);
          setActionError(null);
          focusAt(start + inserted.length);
        })
        .catch((error) => {
          if (!mounted.current || actionRevision.current !== revision) return;
          setActionError(options.clipboardErrorMessage ?? `클립보드를 읽지 못했습니다: ${message(error)}`);
        });
    },
    [focusAt, onValueChange, options.clipboardErrorMessage, options.maxPasteBytes],
  );

  return { actionError, controlRef, items, menu, onSelect };
}

/** Count UTF-8 bytes without allocating an encoded copy of a potentially large paste. */
function utf8ByteLength(value: string): number {
  let bytes = 0;
  for (let index = 0; index < value.length; index += 1) {
    const first = value.charCodeAt(index);
    if (first <= 0x7f) bytes += 1;
    else if (first <= 0x7ff) bytes += 2;
    else if (first >= 0xd800 && first <= 0xdbff) {
      const second = value.charCodeAt(index + 1);
      if (second >= 0xdc00 && second <= 0xdfff) index += 1;
      bytes += second >= 0xdc00 && second <= 0xdfff ? 4 : 3;
    } else bytes += 3;
  }
  return bytes;
}

/** Return a well-formed UTF-8/code-unit bounded prefix without allocating per character. */
function takeUtf8Prefix(value: string, maxBytes: number, maxCodeUnits: number): string {
  if (maxBytes <= 0 || maxCodeUnits <= 0) return "";
  if (!Number.isFinite(maxBytes) && !Number.isFinite(maxCodeUnits)) return value;
  let bytes = 0;
  let codeUnits = 0;
  let end = 0;
  for (let index = 0; index < value.length;) {
    const first = value.charCodeAt(index);
    let characterBytes: number;
    let characterUnits = 1;
    if (first <= 0x7f) characterBytes = 1;
    else if (first <= 0x7ff) characterBytes = 2;
    else if (first >= 0xd800 && first <= 0xdbff) {
      const second = value.charCodeAt(index + 1);
      if (second < 0xdc00 || second > 0xdfff) break;
      characterBytes = 4;
      characterUnits = 2;
    } else if (first >= 0xdc00 && first <= 0xdfff) {
      break;
    } else characterBytes = 3;
    if (bytes + characterBytes > maxBytes || codeUnits + characterUnits > maxCodeUnits) break;
    bytes += characterBytes;
    codeUnits += characterUnits;
    end += characterUnits;
    index += characterUnits;
  }
  return value.slice(0, end);
}

interface ToolTextAreaProps
  extends Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, "value" | "onChange"> {
  value: string;
  onValueChange: (value: string) => void;
  menuLabel?: string;
  /** Fixed error text for features whose clipboard boundary must not reflect platform details. */
  clipboardErrorMessage?: string;
  /** Backward-compatible alias used by earlier text-tool implementations. */
  actionErrorMessage?: string;
  /** Backward-compatible fixed-error alias used by the QR surface. */
  fixedActionError?: string;
  /** Maximum resulting UTF-8 bytes after the explicit Paste action. */
  maxPasteBytes?: number;
}

/** Controlled textarea with the exact Toolbox input menu. */
export function ToolTextArea({
  value,
  onValueChange,
  menuLabel = "입력 작업",
  clipboardErrorMessage,
  actionErrorMessage,
  fixedActionError,
  maxPasteBytes,
  ...props
}: ToolTextAreaProps) {
  const context = useEditableTextContextMenu(value, onValueChange, {
    clipboardErrorMessage: clipboardErrorMessage ?? actionErrorMessage ?? fixedActionError,
    maxPasteBytes,
  });
  return (
    <>
      <textarea
        {...props}
        onContextMenu={context.menu.triggerProps.onContextMenu}
        onKeyDown={context.menu.triggerProps.onKeyDown}
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
  extends Omit<InputHTMLAttributes<HTMLInputElement>, "value" | "onChange"> {
  value: string;
  onValueChange: (value: string) => void;
  menuLabel?: string;
  /** Fixed error text for features whose clipboard boundary must not reflect platform details. */
  clipboardErrorMessage?: string;
  /** Backward-compatible alias used by earlier text-tool implementations. */
  actionErrorMessage?: string;
  /** Backward-compatible fixed-error alias used by the QR surface. */
  fixedActionError?: string;
  /** Maximum resulting UTF-8 bytes after the explicit Paste action. */
  maxPasteBytes?: number;
  inputType?: "text" | "password";
}

/** Controlled single-line text field with the same app-owned input menu. */
export function ToolTextField({
  value,
  onValueChange,
  menuLabel = "입력 작업",
  clipboardErrorMessage,
  actionErrorMessage,
  fixedActionError,
  maxPasteBytes,
  inputType,
  type,
  ...props
}: ToolTextFieldProps) {
  const context = useEditableTextContextMenu(value, onValueChange, {
    clipboardErrorMessage: clipboardErrorMessage ?? actionErrorMessage ?? fixedActionError,
    maxPasteBytes,
  });
  return (
    <>
      <input
        {...props}
        onContextMenu={context.menu.triggerProps.onContextMenu}
        onKeyDown={context.menu.triggerProps.onKeyDown}
        ref={context.controlRef as RefObject<HTMLInputElement | null>}
        type={type ?? inputType ?? "text"}
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

export function downloadBinaryResult(value: string, filename: string, mimeType: string): void {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  const url = URL.createObjectURL(new Blob([bytes], { type: mimeType }));
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
  /** Fixed error text for features whose output action must not reflect platform details. */
  actionErrorMessage?: string;
  /** Backward-compatible fixed-error alias used by the QR surface. */
  fixedActionError?: string;
  /** Optional parent busy state used to share one copy/save flight across controls. */
  busy?: boolean;
  /** Called when a context-menu output action starts or settles. */
  onBusyChange?: (busy: boolean) => void;
  /** Value sent by the explicit API Playground handoff action. */
  handoffValue?: string;
  asDiv?: boolean;
}

/** Read-only result surface with copy/select/save actions. */
export function ToolOutput({
  value,
  children,
  className,
  ariaLabel = "출력",
  menuLabel = "출력 작업",
  downloadName = "dev-toolbox-result.txt",
  actionErrorMessage,
  fixedActionError,
  busy = false,
  onBusyChange,
  handoffValue,
  asDiv = false,
}: ToolOutputProps) {
  const outputRef = useRef<HTMLElement>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [actionBusy, setActionBusy] = useState(false);
  const actionBusyRef = useRef(false);
  const mounted = useRef(true);
  const actionRevision = useRef(0);
  const valueRef = useRef(value);
  valueRef.current = value;

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      actionRevision.current += 1;
    };
  }, []);

  useEffect(() => {
    actionRevision.current += 1;
    actionBusyRef.current = false;
    setActionBusy(false);
    onBusyChange?.(false);
    setActionError(null);
  }, [actionErrorMessage, downloadName, fixedActionError, onBusyChange, value]);

  const menu = useContextMenu({ onBeforeOpen: () => setActionError(null) });
  const items = useMemo<readonly ContextMenuEntry[]>(
    () => [
      { type: "item", id: "copy", label: "복사", disabled: value.length === 0 || actionBusy || busy },
      { type: "item", id: "select-all", label: "모두 선택", disabled: value.length === 0 || actionBusy || busy },
      { type: "item", id: "save", label: "결과 파일 저장", disabled: value.length === 0 || actionBusy || busy },
    ],
    [actionBusy, busy, value.length],
  );

  const onSelect = (id: string) => {
    if (!value || actionBusyRef.current || busy) return;
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

    const snapshot = value;
    const revision = ++actionRevision.current;
    actionBusyRef.current = true;
    setActionBusy(true);
    onBusyChange?.(true);
    const action = Promise.resolve().then(() => {
      if (id === "copy") return navigator.clipboard.writeText(snapshot);
      if (id === "save") downloadTextResult(snapshot, downloadName);
    });
    void action
      .then(() => {
        if (
          mounted.current
          && actionRevision.current === revision
          && valueRef.current === snapshot
        ) setActionError(null);
      })
      .catch((error) => {
        if (
          !mounted.current
          || actionRevision.current !== revision
          || valueRef.current !== snapshot
        ) return;
        setActionError(actionErrorMessage ?? fixedActionError ?? `출력 작업을 완료하지 못했습니다: ${message(error)}`);
      })
      .finally(() => {
        if (mounted.current && actionRevision.current === revision) {
          actionBusyRef.current = false;
          setActionBusy(false);
          onBusyChange?.(false);
        }
      });
  };

  const content = (children ?? value) || " ";
  const trigger = {
    onContextMenu: menu.triggerProps.onContextMenu,
    onKeyDown: menu.triggerProps.onKeyDown,
    tabIndex: 0,
    "aria-label": ariaLabel,
    className,
  };
  const actionValue = handoffValue ?? value;
  const handoffActions = (
    <div className="tool-output-actions">
      <ApiHandoffAction value={actionValue} disabled={busy || actionBusy} />
      <KnowledgeDraftAction value={actionValue} disabled={busy || actionBusy} />
    </div>
  );

  return (
    <>
      {asDiv ? (
        <div {...trigger} ref={outputRef as RefObject<HTMLDivElement | null>}>
          {content}
          {handoffActions}
        </div>
      ) : (
        <pre {...trigger} role="region" ref={outputRef as RefObject<HTMLPreElement | null>}>
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
      {!asDiv ? handoffActions : null}
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
  run: (input: string, signal?: AbortSignal) => Promise<{ output: string; error?: string }>;
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
          <div className="io-label">입력</div>
          <ToolTextArea
            aria-label="입력"
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
            출력 {running && <span className="dim" role="status" aria-live="polite">(실행 중...)</span>}
            {output && !error && (
              <button
                className="copy-btn"
                onClick={() => navigator.clipboard.writeText(output)}
              >
                복사
              </button>
            )}
          </div>
          <ToolOutput
            className={`io-output ${error ? "io-error" : ""}`}
            value={displayedOutput}
            handoffValue={error ? "" : output}
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
      복사
    </button>
  );
}
