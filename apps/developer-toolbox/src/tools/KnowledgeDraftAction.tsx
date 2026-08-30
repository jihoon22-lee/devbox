import { useEffect, useRef, useState } from "react";
import * as api from "../api";

export const KNOWLEDGE_DRAFT_MAX_CHARS = 256_000;
export const KNOWLEDGE_DRAFT_MAX_BYTES = 512 * 1024;

const KNOWLEDGE_DRAFT_BROWSER_ERROR =
  "Knowledge draft handoff는 데스크톱 앱에서만 사용할 수 있습니다. 클립보드로 자동 전환하지 않습니다";
const KNOWLEDGE_DRAFT_INPUT_ERROR = "Knowledge draft로 전달할 텍스트가 유효하지 않습니다";
const KNOWLEDGE_DRAFT_CREATE_ERROR =
  "Knowledge draft를 만들거나 전달하지 못했습니다. 클립보드로 자동 전환하지 않습니다";
const KNOWLEDGE_DRAFT_TARGET_UNAVAILABLE_ERROR =
  "Knowledge를 사용할 수 없습니다. 설치 또는 업데이트 후 다시 시도하세요. 클립보드로 자동 전환하지 않습니다";
const KNOWLEDGE_DRAFT_INVALID_ERROR = "Knowledge draft 응답을 사용할 수 없습니다";

const KNOWLEDGE_DRAFT_FIXED_ERRORS = new Set([
  KNOWLEDGE_DRAFT_INPUT_ERROR,
  KNOWLEDGE_DRAFT_CREATE_ERROR,
  KNOWLEDGE_DRAFT_TARGET_UNAVAILABLE_ERROR,
  KNOWLEDGE_DRAFT_BROWSER_ERROR,
  KNOWLEDGE_DRAFT_INVALID_ERROR,
]);

interface KnowledgeDraftActionProps {
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

function withinKnowledgeDraftBounds(value: string): boolean {
  return value.trim().length > 0
    && !value.includes("\0")
    && value.length <= KNOWLEDGE_DRAFT_MAX_CHARS * 2
    && Array.from(value).length <= KNOWLEDGE_DRAFT_MAX_CHARS
    && utf8ByteLength(value) <= KNOWLEDGE_DRAFT_MAX_BYTES
    && hasWellFormedUnicode(value)
    && !Array.from(value).some((character) => {
      const code = character.charCodeAt(0);
      return (code < 0x20 && ![0x09, 0x0a, 0x0d].includes(code))
        || (code >= 0x7f && code <= 0x9f);
    });
}

function safeKnowledgeDraftError(cause: unknown): string {
  const raw = cause instanceof Error ? cause.message : typeof cause === "string" ? cause : "";
  const message = raw.replace(/^Error:\s*/u, "");
  return KNOWLEDGE_DRAFT_FIXED_ERRORS.has(message) ? message : KNOWLEDGE_DRAFT_CREATE_ERROR;
}

/** Explicit local preview and publish action for a bounded ToolOutput. */
export function KnowledgeDraftAction({ value, disabled = false }: KnowledgeDraftActionProps) {
  const [open, setOpen] = useState(false);
  const [preview, setPreview] = useState(value);
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
          'button:not([disabled]), [tabindex]:not([tabindex="-1"])',
        ),
      );
      if (focusable.length === 0) {
        event.preventDefault();
        return;
      }
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
    if (disabled || busyRef.current) return;
    revisionRef.current += 1;
    setPreview(value);
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
    if (!withinKnowledgeDraftBounds(preview)) {
      setError(KNOWLEDGE_DRAFT_INPUT_ERROR);
      return;
    }
    const revision = ++revisionRef.current;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      const dispatch = await api.createKnowledgeDraftHandoff(preview);
      if (!mountedRef.current || revisionRef.current !== revision) return;
      setOpen(false);
      setStatus(dispatch.redacted
        ? "Knowledge draft 미리보기로 전달했습니다. 민감한 값은 마스킹되었습니다."
        : "Knowledge draft 미리보기로 전달했습니다. 저장은 Knowledge에서 확인하세요.");
    } catch (cause) {
      if (!mountedRef.current || revisionRef.current !== revision) return;
      setError(safeKnowledgeDraftError(cause));
    } finally {
      busyRef.current = false;
      if (mountedRef.current) setBusy(false);
    }
  };

  return (
    <div className="knowledge-draft-action">
      <button
        type="button"
        className="copy-btn knowledge-draft-button"
        aria-label="Save draft to Knowledge"
        onClick={openPreview}
        disabled={disabled || busy}
      >
        Save draft to Knowledge
      </button>
      {status ? (
        <span className="knowledge-draft-status" role="status" aria-live="polite">
          {status}
        </span>
      ) : null}
      {open ? (
        <div className="knowledge-draft-backdrop">
          <section
            ref={dialogRef}
            className="knowledge-draft-dialog"
            role="dialog"
            aria-modal="true"
            aria-busy={busy}
            aria-labelledby="knowledge-draft-dialog-title"
            aria-describedby="knowledge-draft-dialog-description"
          >
            <h2 id="knowledge-draft-dialog-title">Knowledge draft 미리보기</h2>
            <p id="knowledge-draft-dialog-description">
              현재 결과를 확인한 뒤 명시적으로 Knowledge draft 미리보기로 전달하세요. 저장은 Knowledge에서
              별도로 확인하며, 클립보드로 자동 전환하지 않습니다.
            </p>
            <dl className="knowledge-draft-meta">
              <div><dt>source</dt><dd>Developer Toolbox</dd></div>
              <div><dt>target</dt><dd>Knowledge</dd></div>
              <div><dt>format</dt><dd><code>knowledge-draft/v2</code></dd></div>
            </dl>
            <pre className="knowledge-draft-preview" aria-label="Knowledge draft output">{preview}</pre>
            <p className="knowledge-draft-bounds" aria-label="Knowledge draft size">
              {Array.from(preview).length.toLocaleString()} / {KNOWLEDGE_DRAFT_MAX_CHARS.toLocaleString()} chars · {utf8ByteLength(preview).toLocaleString()} / {KNOWLEDGE_DRAFT_MAX_BYTES.toLocaleString()} bytes
            </p>
            {error ? <div className="context-action-error" role="alert">{error}</div> : null}
            <div className="knowledge-draft-dialog-actions">
              <button
                ref={cancelButtonRef}
                type="button"
                className="btn"
                onClick={closePreview}
                disabled={busy}
              >
                취소
              </button>
              <button type="button" className="btn active" onClick={() => void submit()} disabled={busy}>
                {busy ? "전달 중..." : "Save draft"}
              </button>
            </div>
          </section>
        </div>
      ) : null}
    </div>
  );
}
