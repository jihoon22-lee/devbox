import { useEffect, useRef, useState } from "react";
import { createApiRequestHandoff } from "../api";

export const API_HANDOFF_MAX_CHARS = 256_000;
export const API_HANDOFF_MAX_BYTES = 1_024_000;
const API_HANDOFF_INPUT_ERROR = "API Playground로 전달할 텍스트가 유효하지 않습니다";
const API_HANDOFF_CREATE_ERROR =
  "API Playground handoff를 만들지 못했습니다. 클립보드로 자동 전환하지 않습니다";
const API_HANDOFF_FIXED_ERRORS = new Set([
  API_HANDOFF_INPUT_ERROR,
  API_HANDOFF_CREATE_ERROR,
  "API Playground를 사용할 수 없습니다. 설치 또는 업데이트 후 다시 시도하세요. 클립보드로 자동 전환하지 않습니다",
  "API Playground를 실행하지 못했습니다. 전달 데이터는 폐기했습니다. 클립보드로 자동 전환하지 않습니다",
  "API Playground handoff는 데스크톱 앱에서만 사용할 수 있습니다. 클립보드로 자동 전환하지 않습니다",
]);

interface ApiHandoffActionProps {
  value: string;
  disabled?: boolean;
}

function utf8ByteLength(value: string): number {
  let bytes = 0;
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code <= 0x7f) bytes += 1;
    else if (code <= 0x7ff) bytes += 2;
    else if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        bytes += 4;
        index += 1;
      } else {
        bytes += 3;
      }
    } else bytes += 3;
  }
  return bytes;
}

function hasWellFormedUnicode(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (next < 0xdc00 || next > 0xdfff) return false;
      index += 1;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      return false;
    }
  }
  return true;
}

function withinHandoffBounds(value: string): boolean {
  return value.length > 0
    && !value.includes("\0")
    && value.length <= API_HANDOFF_MAX_CHARS * 2
    && utf8ByteLength(value) <= API_HANDOFF_MAX_BYTES
    && hasWellFormedUnicode(value)
    && Array.from(value).length <= API_HANDOFF_MAX_CHARS;
}

function safeHandoffError(cause: unknown): string {
  const message = cause instanceof Error ? cause.message : String(cause);
  return API_HANDOFF_FIXED_ERRORS.has(message) ? message : API_HANDOFF_CREATE_ERROR;
}

/** Preview/edit/manual handoff action for the currently visible result. */
export function ApiHandoffAction({ value, disabled = false }: ApiHandoffActionProps) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState(value);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const dialogRef = useRef<HTMLElement | null>(null);
  const cancelButtonRef = useRef<HTMLButtonElement | null>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const mountedRef = useRef(true);
  const busyRef = useRef(false);
  const revisionRef = useRef(0);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      revisionRef.current += 1;
    };
  }, []);

  useEffect(() => {
    revisionRef.current += 1;
    setOpen(false);
    setError(null);
    setStatus(null);
  }, [value]);

  useEffect(() => {
    if (!open) return undefined;
    const active = document.activeElement;
    previousFocusRef.current = active instanceof HTMLElement ? active : null;
    cancelButtonRef.current?.focus();

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busyRef.current) {
        event.preventDefault();
        revisionRef.current += 1;
        setOpen(false);
        setError(null);
        return;
      }
      if (event.key !== "Tab") return;
      const dialog = dialogRef.current;
      if (!dialog) return;
      const focusable = Array.from(
        dialog.querySelectorAll<HTMLElement>(
          'button:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
        ),
      );
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      const previous = previousFocusRef.current;
      if (previous?.isConnected) previous.focus({ preventScroll: true });
    };
  }, [open]);

  if (!value) return null;

  const openPreview = () => {
    if (busyRef.current) return;
    revisionRef.current += 1;
    setDraft(value);
    setError(null);
    setStatus(null);
    setOpen(true);
  };

  const closePreview = () => {
    if (busyRef.current) return;
    revisionRef.current += 1;
    setOpen(false);
    setError(null);
  };

  const submit = async () => {
    if (busyRef.current) return;
    if (!withinHandoffBounds(draft)) {
      setError(API_HANDOFF_INPUT_ERROR);
      return;
    }
    const revision = ++revisionRef.current;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      const dispatch = await createApiRequestHandoff(draft);
      if (!mountedRef.current || revisionRef.current !== revision) return;
      setOpen(false);
      setStatus(`API Playground 미리보기로 전달했습니다 (${dispatch.producerId} → ${dispatch.consumerId}).`);
    } catch (cause) {
      if (!mountedRef.current || revisionRef.current !== revision) return;
      setError(safeHandoffError(cause));
    } finally {
      busyRef.current = false;
      if (mountedRef.current) setBusy(false);
    }
  };

  return (
    <span className="api-handoff-action">
      <button
        type="button"
        className="copy-btn api-handoff-button"
        aria-label="API Playground로 보내기"
        onClick={openPreview}
        disabled={disabled || busy}
      >
        API Playground로 보내기
      </button>
      {status ? (
        <span className="api-handoff-status" role="status" aria-live="polite">
          {status}
        </span>
      ) : null}
      {open ? (
        <div className="api-handoff-backdrop">
          <section
            ref={dialogRef}
            className="api-handoff-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="api-handoff-dialog-title"
            aria-describedby="api-handoff-dialog-description"
          >
            <h2 id="api-handoff-dialog-title">API Playground 요청 미리보기</h2>
            <p id="api-handoff-dialog-description">
              현재 결과를 수정한 뒤 명시적으로 전달하세요. API Playground는 요청을 편집기에
              넣기만 하며 자동으로 보내지 않습니다.
            </p>
            <dl className="api-handoff-meta">
              <div><dt>method</dt><dd><code>POST</code></dd></div>
              <div><dt>url</dt><dd><code>/</code></dd></div>
              <div><dt>content-type</dt><dd><code>text/plain; charset=utf-8</code></dd></div>
            </dl>
            <label className="api-handoff-editor">
              요청 body
              <textarea
                aria-label="API Playground request body"
                value={draft}
                onChange={(event) => {
                  setDraft(event.currentTarget.value);
                  setError(null);
                }}
                rows={12}
                maxLength={API_HANDOFF_MAX_CHARS}
                disabled={busy}
                spellCheck={false}
              />
            </label>
            <p className="api-handoff-bounds">
              {Array.from(draft).length.toLocaleString()} / {API_HANDOFF_MAX_CHARS.toLocaleString()} chars · {utf8ByteLength(draft).toLocaleString()} / {API_HANDOFF_MAX_BYTES.toLocaleString()} bytes
            </p>
            {error ? <div className="context-action-error" role="alert">{error}</div> : null}
            <div className="api-handoff-dialog-actions">
              <button
                ref={cancelButtonRef}
                type="button"
                className="btn"
                onClick={closePreview}
                disabled={busy}
              >
                취소
              </button>
              <button type="button" className="btn" onClick={() => void submit()} disabled={busy}>
                {busy ? "전달 중..." : "API Playground로 전달"}
              </button>
            </div>
          </section>
        </div>
      ) : null}
    </span>
  );
}
