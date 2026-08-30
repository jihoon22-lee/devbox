import { useEffect, useRef, useState } from "react";
import "./App.css";
import {
  acceptToolboxText,
  discardToolboxText,
  onOpenRequest,
  previewToolboxText,
  renewToolboxText,
  takePendingOpen,
  TOOLBOX_TEXT_HANDOFF_KIND,
} from "./api";
import { GROUPS, TOOLS } from "./tools";
import {
  SmartWorkflowPanel,
  type SmartWorkflowIncomingText,
} from "./workflows/SmartWorkflowPanel";
import type { OpenRequest, ToolboxTextHandoffPreview } from "./types";

export const TOOLBOX_TEXT_SOURCE_LABELS: Readonly<Record<string, string>> = {
  "api-playground": "API Playground",
  "devbox-launcher": "Devbox Launcher",
  "log-lens": "Log Lens",
};

const HANDOFF_ID_PATTERN = /^[0-9a-f]{32}$/u;
const TOOLBOX_TEXT_INVALID_ERROR = "텍스트 handoff를 사용할 수 없습니다";
const TOOLBOX_TEXT_BUSY_ERROR = "다른 텍스트 handoff를 먼저 처리하세요";
const TOOLBOX_TEXT_EXPIRED_ERROR = "텍스트 handoff 미리보기가 만료되었습니다. 다시 전달하세요";
const TOOLBOX_TEXT_STORAGE_ERROR = "텍스트 handoff 저장소를 사용할 수 없습니다";
const TOOLBOX_TEXT_REJECTED_ERROR = "텍스트 handoff를 처리하지 못했습니다";
const TOOLBOX_TEXT_BUSY_UI_ERROR = "기존 텍스트 handoff 미리보기를 먼저 적용하거나 취소하세요";

function safeToolboxTextError(cause: unknown): string {
  const message = cause instanceof Error
    ? cause.message.replace(/^Error:\s*/u, "")
    : typeof cause === "string" ? cause : "";
  return new Set([
    TOOLBOX_TEXT_INVALID_ERROR,
    TOOLBOX_TEXT_BUSY_ERROR,
    TOOLBOX_TEXT_EXPIRED_ERROR,
    TOOLBOX_TEXT_STORAGE_ERROR,
    TOOLBOX_TEXT_REJECTED_ERROR,
    "Developer Toolbox handoff는 데스크톱 앱에서만 사용할 수 있습니다. 클립보드로 자동 전환하지 않습니다",
    "텍스트 handoff 응답을 사용할 수 없습니다",
    TOOLBOX_TEXT_BUSY_UI_ERROR,
  ]).has(message)
    ? message
    : TOOLBOX_TEXT_REJECTED_ERROR;
}

function isTerminalToolboxTextError(message: string): boolean {
  return message === TOOLBOX_TEXT_INVALID_ERROR
    || message === TOOLBOX_TEXT_EXPIRED_ERROR
    || message === "텍스트 handoff 응답을 사용할 수 없습니다";
}

function formatHandoffExpiry(expiresAtMs: number): string {
  const date = new Date(expiresAtMs);
  return Number.isFinite(date.getTime()) ? date.toLocaleString() : "시간 미상";
}

export default function App() {
  const [activeId, setActiveId] = useState(TOOLS[0].id);
  const [handoffPreview, setHandoffPreview] = useState<ToolboxTextHandoffPreview | null>(null);
  const [handoffBusy, setHandoffBusy] = useState(false);
  const [handoffError, setHandoffError] = useState<string | null>(null);
  const [incomingText, setIncomingText] = useState<SmartWorkflowIncomingText | null>(null);
  const handoffPreviewRef = useRef<ToolboxTextHandoffPreview | null>(null);
  const handoffBusyRef = useRef(false);
  const handoffGenerationRef = useRef(0);
  const handoffRequestIdRef = useRef<string | null>(null);
  const incomingTextRevisionRef = useRef(0);
  const mountedRef = useRef(true);
  const handoffDialogRef = useRef<HTMLElement | null>(null);
  const handoffCancelButtonRef = useRef<HTMLButtonElement | null>(null);
  const handoffPreviousFocusRef = useRef<HTMLElement | null>(null);
  const cancelHandoffRef = useRef<() => void>(() => undefined);

  const active = TOOLS.find((t) => t.id === activeId) ?? TOOLS[0];
  const ActiveComponent = active.component;

  const clearHandoffPreview = () => {
    handoffPreviewRef.current = null;
    handoffRequestIdRef.current = null;
    setHandoffPreview(null);
  };

  const handleOpenRequest = (request: OpenRequest) => {
    if (request.target.kind !== "handoff") return;
    if (request.target.handoffKind !== TOOLBOX_TEXT_HANDOFF_KIND) {
      setHandoffError(TOOLBOX_TEXT_INVALID_ERROR);
      return;
    }
    if (!HANDOFF_ID_PATTERN.test(request.target.id)) {
      setHandoffError(TOOLBOX_TEXT_INVALID_ERROR);
      return;
    }
    if (handoffBusyRef.current || handoffPreviewRef.current) {
      setHandoffError(TOOLBOX_TEXT_BUSY_UI_ERROR);
      return;
    }

    const id = request.target.id;
    const generation = ++handoffGenerationRef.current;
    handoffRequestIdRef.current = id;
    handoffBusyRef.current = true;
    setHandoffBusy(true);
    setHandoffError(null);
    void previewToolboxText(id)
      .then((preview) => {
        const validSource = typeof preview.producerId === "string"
          && Object.prototype.hasOwnProperty.call(TOOLBOX_TEXT_SOURCE_LABELS, preview.producerId);
        if (!validSource) {
          void Promise.resolve().then(() => discardToolboxText(preview.handoffId)).catch(() => undefined);
          if (mountedRef.current && handoffGenerationRef.current === generation) {
            handoffRequestIdRef.current = null;
            setHandoffError(TOOLBOX_TEXT_INVALID_ERROR);
          }
          return;
        }
        if (!mountedRef.current
          || handoffGenerationRef.current !== generation
          || handoffRequestIdRef.current !== id) {
          void Promise.resolve().then(() => discardToolboxText(preview.handoffId)).catch(() => undefined);
          return;
        }
        handoffPreviewRef.current = preview;
        setHandoffPreview(preview);
      })
      .catch((cause) => {
        // A malformed renderer response can arrive after native claim. A
        // best-effort restore with the exact request id keeps that claim from
        // being stranded until lease expiry.
        void Promise.resolve().then(() => discardToolboxText(id)).catch(() => undefined);
        if (!mountedRef.current || handoffGenerationRef.current !== generation) return;
        handoffRequestIdRef.current = null;
        setHandoffError(safeToolboxTextError(cause));
      })
      .finally(() => {
        if (handoffGenerationRef.current !== generation) return;
        handoffBusyRef.current = false;
        if (mountedRef.current) setHandoffBusy(false);
        if (!handoffPreviewRef.current) handoffRequestIdRef.current = null;
      });
  };

  const handleOpenRequestRef = useRef(handleOpenRequest);
  handleOpenRequestRef.current = handleOpenRequest;

  // The native shell stores cold-start argv and emits hot relaunches into the
  // same one-shot slot. Register first, then pull; hot event payloads are only
  // wakeups and never enter the preview path directly.
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    const consumePendingOpen = () => {
      void takePendingOpen()
        .then((request) => {
          if (!disposed && request) handleOpenRequestRef.current(request);
        })
        .catch((cause) => {
          if (!disposed) setHandoffError(safeToolboxTextError(cause));
        });
    };

    void onOpenRequest(() => consumePendingOpen())
      .then((stop) => {
        if (disposed) stop();
        else {
          unlisten = stop;
          consumePendingOpen();
        }
      })
      .catch((cause) => {
        if (!disposed) setHandoffError(safeToolboxTextError(cause));
        consumePendingOpen();
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // Keep the claim alive while the explicit preview is open. Renewal extends
  // only the short claim lease; the handoff's own expiry shown in the modal
  // never changes.
  useEffect(() => {
    if (!handoffPreview) return undefined;
    const id = handoffPreview.handoffId;
    let disposed = false;
    const interval = window.setInterval(() => {
      if (disposed || handoffBusyRef.current || handoffPreviewRef.current?.handoffId !== id) return;
      void Promise.resolve().then(() => renewToolboxText(id)).catch((cause) => {
        if (disposed || !mountedRef.current || handoffPreviewRef.current?.handoffId !== id) return;
        const message = safeToolboxTextError(cause);
        if (isTerminalToolboxTextError(message)) clearHandoffPreview();
        setHandoffError(message);
      });
    }, 30_000);
    return () => {
      disposed = true;
      window.clearInterval(interval);
    };
  }, [handoffPreview]);

  const onApplyHandoff = async () => {
    const preview = handoffPreviewRef.current;
    if (!preview || handoffBusyRef.current) return;
    const id = preview.handoffId;
    const generation = ++handoffGenerationRef.current;
    handoffBusyRef.current = true;
    setHandoffBusy(true);
    setHandoffError(null);
    try {
      const text = await acceptToolboxText(id);
      if (!mountedRef.current || handoffGenerationRef.current !== generation) return;
      // Ack is deliberately before this renderer-only state update. The
      // accepted text is never persisted, copied, or used to auto-run a tool.
      clearHandoffPreview();
      setIncomingText({
        revision: ++incomingTextRevisionRef.current,
        text,
      });
    } catch (cause) {
      if (!mountedRef.current || handoffGenerationRef.current !== generation) return;
      const message = safeToolboxTextError(cause);
      if (isTerminalToolboxTextError(message)) clearHandoffPreview();
      setHandoffError(message);
    } finally {
      handoffBusyRef.current = false;
      if (mountedRef.current) setHandoffBusy(false);
    }
  };

  const onCancelHandoff = async () => {
    const preview = handoffPreviewRef.current;
    if (!preview || handoffBusyRef.current) return;
    const id = preview.handoffId;
    const generation = ++handoffGenerationRef.current;
    handoffBusyRef.current = true;
    setHandoffBusy(true);
    setHandoffError(null);
    try {
      await discardToolboxText(id);
      if (!mountedRef.current || handoffGenerationRef.current !== generation) return;
      clearHandoffPreview();
    } catch (cause) {
      if (!mountedRef.current || handoffGenerationRef.current !== generation) return;
      const message = safeToolboxTextError(cause);
      if (isTerminalToolboxTextError(message)) clearHandoffPreview();
      setHandoffError(message);
    } finally {
      handoffBusyRef.current = false;
      if (mountedRef.current) setHandoffBusy(false);
    }
  };

  cancelHandoffRef.current = onCancelHandoff;

  // A modal is a transaction: default focus is Cancel, Tab is trapped, and
  // closing it returns focus to the control that was active before preview.
  useEffect(() => {
    if (!handoffPreview) {
      const previous = handoffPreviousFocusRef.current;
      handoffPreviousFocusRef.current = null;
      if (previous?.isConnected) window.setTimeout(() => {
        if (mountedRef.current && previous.isConnected) previous.focus({ preventScroll: true });
      }, 0);
      return undefined;
    }

    // Claim completion and modal rendering can commit before the busy flag is
    // cleared. Waiting for the enabled state prevents `.focus()` from being
    // ignored by the browser and guarantees Cancel receives default focus.
    if (handoffBusy) return undefined;

    const dialog = handoffDialogRef.current;
    const activeElement = document.activeElement;
    handoffPreviousFocusRef.current = activeElement instanceof HTMLElement && !dialog?.contains(activeElement)
      ? activeElement
      : null;
    handoffCancelButtonRef.current?.focus();
    if (!dialog) return undefined;

    const focusableElements = () => Array.from(
      dialog.querySelectorAll<HTMLElement>(
        'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      ),
    );
    const onDialogKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        if (!handoffBusyRef.current) void cancelHandoffRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = focusableElements();
      if (focusable.length === 0) {
        event.preventDefault();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (!dialog.contains(document.activeElement)) {
        event.preventDefault();
        first.focus();
      } else if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    dialog.addEventListener("keydown", onDialogKeyDown);
    return () => dialog.removeEventListener("keydown", onDialogKeyDown);
  }, [handoffBusy, handoffPreview]);

  // Return a claimed envelope if this renderer disappears before Apply or
  // Cancel completes. A late preview result is handled by its generation
  // guard above and is restored with the returned id.
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      handoffGenerationRef.current += 1;
      // If claim is still in flight, the generation guard in its continuation
      // restores the exact returned envelope. Calling here as well would race
      // that continuation and issue a duplicate restore. A rendered preview,
      // on the other hand, already has a stable claim id and is restored now.
      const id = handoffPreviewRef.current?.handoffId;
      if (id && !handoffBusyRef.current) {
        void Promise.resolve().then(() => discardToolboxText(id)).catch(() => undefined);
      }
    };
  }, []);

  return (
    <div className="app">
      <aside className="sidebar">
        <h1 className="app-title">Dev Toolbox</h1>
        {GROUPS.map((group) => (
          <div key={group} className="group">
            <div className="group-name">{group}</div>
            {TOOLS.filter((t) => t.group === group).map((tool) => (
              <button
                key={tool.id}
                className={`tool-link ${tool.id === activeId ? "active" : ""}`}
                onClick={() => setActiveId(tool.id)}
              >
                {tool.name}
              </button>
            ))}
          </div>
        ))}
      </aside>
      <main className="content">
        {handoffError ? <div className="toolbox-handoff-error" role="alert">{handoffError}</div> : null}
        <SmartWorkflowPanel
          activeToolId={activeId}
          onOpenTool={setActiveId}
          incomingText={incomingText}
        />
        <h2 className="tool-title">{active.name}</h2>
        <ActiveComponent />
      </main>
      {handoffPreview ? (
        <div className="toolbox-handoff-backdrop">
          <section
            className="toolbox-handoff-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="toolbox-handoff-title"
            aria-describedby="toolbox-handoff-description"
            ref={handoffDialogRef}
          >
            <div className="toolbox-handoff-heading">
              <div>
                <h2 id="toolbox-handoff-title">Toolbox 텍스트 미리보기</h2>
                <p id="toolbox-handoff-description">
                  {TOOLBOX_TEXT_SOURCE_LABELS[handoffPreview.producerId]}에서 전달한 텍스트입니다. 적용하면 Smart input에만 넣으며,
                  자동 실행·저장·복사하지 않습니다.
                </p>
              </div>
              <span className="toolbox-handoff-kind">{TOOLBOX_TEXT_HANDOFF_KIND}</span>
            </div>
            <dl className="toolbox-handoff-meta">
              <div><dt>source</dt><dd>{TOOLBOX_TEXT_SOURCE_LABELS[handoffPreview.producerId]}</dd></div>
              <div><dt>handoff</dt><dd><code>{handoffPreview.handoffId}</code></dd></div>
              <div><dt>expires</dt><dd>{formatHandoffExpiry(handoffPreview.expiresAtMs)}</dd></div>
            </dl>
            {handoffPreview.redacted ? (
              <p className="toolbox-handoff-redacted" role="note">민감한 값은 송신 앱에서 마스킹되었습니다.</p>
            ) : null}
            <pre className="toolbox-handoff-text" aria-label="Toolbox handoff text">{handoffPreview.text}</pre>
            <div className="toolbox-handoff-actions">
              <button
                ref={handoffCancelButtonRef}
                type="button"
                className="btn"
                disabled={handoffBusy}
                onClick={() => void onCancelHandoff()}
              >
                취소
              </button>
              <button
                type="button"
                className="btn active"
                disabled={handoffBusy}
                onClick={() => void onApplyHandoff()}
              >
                {handoffBusy ? "처리 중..." : "적용"}
              </button>
            </div>
          </section>
        </div>
      ) : null}
    </div>
  );
}
