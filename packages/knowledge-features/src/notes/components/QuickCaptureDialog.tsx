import { useEffect, useRef, useState } from "react";
import {
  discardQuickCapturePreview,
  previewQuickCapture,
  readClipboardText,
  saveQuickCapture,
} from "../api";
import type { QuickCaptureInput, QuickCapturePreview, QuickCaptureSaved } from "../types";
import {
  isQuickCaptureUtf8Within,
  MAX_QUICK_CAPTURE_BODY_BYTES,
  MAX_QUICK_CAPTURE_CLIPBOARD_BYTES,
  MAX_QUICK_CAPTURE_RAW_BODY_BYTES,
  MAX_QUICK_CAPTURE_TAG_BYTES,
  MAX_QUICK_CAPTURE_TAGS,
  MAX_QUICK_CAPTURE_TAG_ITEM_BYTES,
  MAX_QUICK_CAPTURE_TITLE_BYTES,
  MAX_QUICK_CAPTURE_TITLE_CHARS,
  QuickCaptureValidationError,
  normalizeQuickCapture,
  parseQuickCaptureTags,
  quickCaptureUnicodeScalars,
  quickCaptureUtf8Bytes,
} from "../lib/quickCapture";

interface Props {
  open: boolean;
  onClose: () => void;
  onSaved: (saved: QuickCaptureSaved) => void;
  restoreFocusRef?: React.RefObject<HTMLElement | null>;
}

type DialogPhase = "edit" | "preview";
type BusyOperation = "preview" | "clipboard" | "save" | null;

const SAFE_ERROR_MESSAGES = new Set([
  "빠른 캡처 본문을 입력하세요",
  "빠른 캡처 입력이 올바르지 않습니다",
  "민감한 정보가 포함되어 있어 저장하지 않았습니다",
  "제목은 UTF-8 800바이트·200자 이내로 입력하세요",
  "본문은 LF 기준 64 KiB(원문 128 KiB) 이내로 입력하세요",
  "태그는 최대 20개까지 입력하세요",
  "태그 하나는 UTF-8 192바이트·48자 이내로 입력하세요",
  "태그 전체는 UTF-8 1 KiB 이내로 입력하세요",
  "태그에 줄바꿈·쉼표·대괄호·따옴표를 사용할 수 없습니다",
  "빠른 캡처 미리보기가 오래되어 다시 확인하세요",
  "빠른 캡처 저장은 Knowledge 앱에서만 사용할 수 있습니다",
]);

function focusableElements(container: HTMLElement): HTMLElement[] {
  return Array.from(
    container.querySelectorAll<HTMLElement>(
      "button:not([disabled]), input:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])",
    ),
  );
}

function errorMessage(cause: unknown, fallback: string): string {
  const message = cause instanceof Error ? cause.message : "";
  if (SAFE_ERROR_MESSAGES.has(message)) {
    return message;
  }
  return fallback;
}

export default function QuickCaptureDialog({
  open,
  onClose,
  onSaved,
  restoreFocusRef,
}: Props) {
  const dialogRef = useRef<HTMLElement>(null);
  const titleRef = useRef<HTMLInputElement>(null);
  const generationRef = useRef(0);
  const busyRef = useRef(false);
  const busyOperationRef = useRef<BusyOperation>(null);
  const previewIdRef = useRef<string | null>(null);
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [tagsText, setTagsText] = useState("");
  const [phase, setPhase] = useState<DialogPhase>("edit");
  const [preview, setPreview] = useState<QuickCapturePreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const parsedTags = parseQuickCaptureTags(tagsText);
  const titleBytes = quickCaptureUtf8Bytes(title);
  const titleScalars = quickCaptureUnicodeScalars(title);
  const normalizedBody = body.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
  const bodyBytes = quickCaptureUtf8Bytes(normalizedBody);
  const rawBodyBytes = quickCaptureUtf8Bytes(body);
  const tagBytes = parsedTags.reduce((total, tag) => total + quickCaptureUtf8Bytes(tag), 0);

  const discardPreview = () => {
    const previewId = previewIdRef.current;
    previewIdRef.current = null;
    setPreview(null);
    if (previewId) void discardQuickCapturePreview(previewId);
  };

  useEffect(() => {
    if (!open) return;
    generationRef.current += 1;
    setTitle("");
    setBody("");
    setTagsText("");
    setPhase("edit");
    setPreview(null);
    previewIdRef.current = null;
    setBusy(false);
    busyRef.current = false;
    busyOperationRef.current = null;
    setError(null);
    const focusTimer = window.setTimeout(() => titleRef.current?.focus(), 0);
    return () => {
      window.clearTimeout(focusTimer);
      const previewId = previewIdRef.current;
      previewIdRef.current = null;
      if (previewId) void discardQuickCapturePreview(previewId);
      generationRef.current += 1;
    };
  }, [open]);

  if (!open) return null;

  const input = (): QuickCaptureInput => ({
    title,
    body,
    tags: parsedTags,
  });

  const setBusyState = (next: boolean) => {
    busyRef.current = next;
    setBusy(next);
  };

  const close = () => {
    // Preview/clipboard requests are cancellable at the UI boundary: bumping
    // the generation suppresses their eventual state update and a late native
    // preview ID is explicitly discarded. A save has already consumed its
    // one-shot approval, so keep the modal until that mutation settles.
    if (busyRef.current && busyOperationRef.current === "save") return;
    generationRef.current += 1;
    busyOperationRef.current = null;
    setBusyState(false);
    discardPreview();
    onClose();
    window.setTimeout(() => restoreFocusRef?.current?.focus(), 0);
  };

  const runPreview = async () => {
    if (busyRef.current) return;
    const token = ++generationRef.current;
    busyOperationRef.current = "preview";
    setBusyState(true);
    setError(null);
    try {
      const next = await previewQuickCapture(input());
      if (token !== generationRef.current) {
        // The native preview slot is durable until explicitly consumed.  A
        // response that arrives after Escape/unmount must not leave an
        // approval behind for a later dialog instance.
        void discardQuickCapturePreview(next.previewId);
        return;
      }
      previewIdRef.current = next.previewId;
      setPreview(next);
      setPhase("preview");
    } catch (cause) {
      if (token === generationRef.current) {
        setError(errorMessage(cause, "빠른 캡처 미리보기를 만들 수 없습니다"));
      }
    } finally {
      if (token === generationRef.current) {
        busyOperationRef.current = null;
        setBusyState(false);
      }
    }
  };

  const save = async () => {
    if (busyRef.current || !preview) return;
    const token = ++generationRef.current;
    busyOperationRef.current = "save";
    setBusyState(true);
    setError(null);
    try {
      const saved = await saveQuickCapture(preview.previewId);
      if (token !== generationRef.current) return;
      previewIdRef.current = null;
      onSaved(saved);
      generationRef.current += 1;
      onClose();
      window.setTimeout(() => restoreFocusRef?.current?.focus(), 0);
    } catch (cause) {
      if (token === generationRef.current) {
        // Native approval IDs are one-shot even when the filesystem fails.
        // Return to edit so the next attempt must obtain a fresh preview.
        previewIdRef.current = null;
        setPreview(null);
        setPhase("edit");
        setError(errorMessage(cause, "빠른 캡처를 저장하지 못했습니다"));
      }
    } finally {
      if (token === generationRef.current) {
        busyOperationRef.current = null;
        setBusyState(false);
      }
    }
  };

  const pasteClipboard = async () => {
    if (busyRef.current) return;
    const token = ++generationRef.current;
    busyOperationRef.current = "clipboard";
    setBusyState(true);
    setError(null);
    try {
      const text = await readClipboardText(MAX_QUICK_CAPTURE_CLIPBOARD_BYTES);
      if (token !== generationRef.current) return;
      if (!isQuickCaptureUtf8Within(text, MAX_QUICK_CAPTURE_CLIPBOARD_BYTES)) {
        throw new QuickCaptureValidationError("body-too-large");
      }
      const normalizedText = text.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
      // Apply the same policy before placing clipboard text in the controlled
      // draft.  A credential-like or malformed clipboard value is therefore
      // never retained by this dialog, even temporarily as a body draft.
      const checked = normalizeQuickCapture({ title: "", body: normalizedText, tags: [] });
      if (token !== generationRef.current) return;
      setBody(checked.body);
    } catch (cause) {
      if (token === generationRef.current) {
        setError(errorMessage(cause, "클립보드 내용을 읽을 수 없습니다"));
      }
    } finally {
      if (token === generationRef.current) {
        busyOperationRef.current = null;
        setBusyState(false);
      }
    }
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLElement>) => {
    if (event.nativeEvent.isComposing || event.keyCode === 229) return;
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      if (phase === "edit") void runPreview();
      else void save();
      return;
    }
    if (event.key !== "Tab" || !dialogRef.current) return;
    const elements = focusableElements(dialogRef.current);
    if (elements.length === 0) return;
    const first = elements[0];
    const last = elements[elements.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <div
      className="modal-backdrop quick-capture-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) close();
      }}
    >
      <section
        ref={dialogRef}
        className="quick-capture-dialog"
        role="dialog"
        aria-modal="true"
        aria-busy={busy}
        aria-labelledby="quick-capture-title"
        aria-describedby="quick-capture-description"
        onKeyDown={handleKeyDown}
      >
        <div className="quick-capture-head">
          <div>
            <h2 id="quick-capture-title">빠른 캡처</h2>
            <p id="quick-capture-description">인터넷 없이 Inbox에 새 Markdown 노트를 저장합니다.</p>
          </div>
          <button
            className="btn"
            type="button"
            onClick={close}
            disabled={busyOperationRef.current === "save"}
            aria-label="빠른 캡처 닫기"
          >
            닫기
          </button>
        </div>

        {phase === "edit" ? (
          <div className="quick-capture-form">
            <div className="quick-capture-target" aria-label="저장 위치">
              저장 위치 <strong>Inbox</strong>
            </div>
            <label className="quick-capture-field" htmlFor="quick-capture-title-input">
              제목 <span className="dim">(선택, 비워 두면 기본 제목)</span>
              <span className={`quick-capture-counter ${titleBytes > MAX_QUICK_CAPTURE_TITLE_BYTES || titleScalars > MAX_QUICK_CAPTURE_TITLE_CHARS ? "over" : ""}`} aria-live="polite">
                {titleBytes} / {MAX_QUICK_CAPTURE_TITLE_BYTES} bytes · {titleScalars} / {MAX_QUICK_CAPTURE_TITLE_CHARS}자
              </span>
              <input
                id="quick-capture-title-input"
                ref={titleRef}
                value={title}
                // HTML maxlength counts UTF-16 code units.  Two units per
                // non-BMP scalar keeps the UI from rejecting a valid 200
                // scalar title before the shared byte/scalar validator runs.
                maxLength={MAX_QUICK_CAPTURE_TITLE_CHARS * 2}
                aria-describedby="quick-capture-title-hint"
                onChange={(event) => setTitle(event.currentTarget.value)}
                disabled={busy}
                autoComplete="off"
              />
              <span id="quick-capture-title-hint" className="sr-only">최대 200자</span>
            </label>
            <label className="quick-capture-field" htmlFor="quick-capture-body-input">
              본문 <span className="dim">(필수)</span>
              <span className={`quick-capture-counter ${bodyBytes > MAX_QUICK_CAPTURE_BODY_BYTES || rawBodyBytes > MAX_QUICK_CAPTURE_RAW_BODY_BYTES ? "over" : ""}`} aria-live="polite">
                LF {bodyBytes} / {MAX_QUICK_CAPTURE_BODY_BYTES} bytes · 원문 {rawBodyBytes} / {MAX_QUICK_CAPTURE_RAW_BODY_BYTES} bytes
              </span>
              <textarea
                id="quick-capture-body-input"
                value={body}
                maxLength={MAX_QUICK_CAPTURE_RAW_BODY_BYTES}
                aria-describedby="quick-capture-body-hint"
                onChange={(event) => setBody(event.currentTarget.value)}
                disabled={busy}
                rows={10}
              />
              <span id="quick-capture-body-hint" className="sr-only">LF 기준 UTF-8 최대 64 KiB</span>
            </label>
            <label className="quick-capture-field" htmlFor="quick-capture-tags-input">
              태그 <span className="dim">(쉼표로 구분, 선택)</span>
              <span className={`quick-capture-counter ${tagBytes > MAX_QUICK_CAPTURE_TAG_BYTES || parsedTags.length > MAX_QUICK_CAPTURE_TAGS || parsedTags.some((tag) => quickCaptureUtf8Bytes(tag) > MAX_QUICK_CAPTURE_TAG_ITEM_BYTES) ? "over" : ""}`} aria-live="polite">
                {parsedTags.length} / {MAX_QUICK_CAPTURE_TAGS}개 · {tagBytes} / {MAX_QUICK_CAPTURE_TAG_BYTES} bytes
              </span>
              <input
                id="quick-capture-tags-input"
                value={tagsText}
                maxLength={MAX_QUICK_CAPTURE_TAG_BYTES}
                aria-describedby="quick-capture-tags-hint"
                onChange={(event) => setTagsText(event.currentTarget.value)}
                disabled={busy}
                autoComplete="off"
              />
              <span id="quick-capture-tags-hint" className="sr-only">최대 20개, 태그 하나당 48자</span>
            </label>
            <p className="quick-capture-privacy">
              민감한 정보처럼 보이는 credential은 저장하지 않습니다. 클립보드는 이 버튼을 누른 순간에만 한 번 읽습니다.
            </p>
            {error && <div id="quick-capture-error" className="quick-capture-error" role="alert" aria-live="assertive">{error}</div>}
            <div className="quick-capture-progress" role="status" aria-live="polite" aria-atomic="true">
              {busy ? "미리보기를 확인하는 중…" : ""}
            </div>
            <div className="quick-capture-actions">
              <button className="btn" type="button" onClick={() => void pasteClipboard()} disabled={busy}>
                클립보드에서 본문 가져오기
              </button>
              <span className="spacer" />
              <button
                className="btn"
                type="button"
                onClick={close}
                disabled={busyOperationRef.current === "save"}
              >취소</button>
              <button className="btn primary" type="button" onClick={() => void runPreview()} disabled={busy}>
                {busy ? "확인 중…" : "미리보기"}
              </button>
            </div>
          </div>
        ) : (
          <div className="quick-capture-preview">
            <div className="quick-capture-preview-meta">
              <span>저장 위치</span><strong>{preview?.target}</strong>
              <span>제목</span><strong>{preview?.title}</strong>
              <span>태그</span><strong>{preview?.tags.length ? preview.tags.join(", ") : "없음"}</strong>
            </div>
            <div className="quick-capture-preview-body">
              <div className="dim" id="quick-capture-preview-label">본문 미리보기</div>
              <pre aria-labelledby="quick-capture-preview-label">{preview?.body}</pre>
            </div>
            {error && <div id="quick-capture-error" className="quick-capture-error" role="alert" aria-live="assertive">{error}</div>}
            <div className="quick-capture-progress" role="status" aria-live="polite" aria-atomic="true">
              {busy ? "저장하는 중…" : "미리보기를 확인했습니다. 저장을 누르면 새 노트가 생성됩니다."}
            </div>
            <div className="quick-capture-actions">
              <button
                className="btn"
                type="button"
                onClick={() => {
                  if (busyRef.current) return;
                  discardPreview();
                  setPhase("edit");
                  setError(null);
                  window.setTimeout(() => titleRef.current?.focus(), 0);
                }}
                disabled={busy}
              >
                수정
              </button>
              <span className="spacer" />
              <button
                className="btn"
                type="button"
                onClick={close}
                disabled={busyOperationRef.current === "save"}
              >취소</button>
              <button className="btn primary" type="button" onClick={() => void save()} disabled={busy}>
                {busy ? "저장 중…" : "저장"}
              </button>
            </div>
          </div>
        )}
      </section>
    </div>
  );
}
