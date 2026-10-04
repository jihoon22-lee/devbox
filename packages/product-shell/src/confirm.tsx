import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { restoreFocus, trapDialogKeyDown } from "@devbox/a11y";

type Request = { message: string; resolve: (accepted: boolean) => void; origin: HTMLElement | null };
let host: ((request: Request | null) => void) | null = null;
let pending: Request | null = null;
let closeDialog: (() => void) | null = null;

/** A missing host or an already active review never authorizes another action. */
export function confirmAction(message: string): Promise<boolean> {
  if (!host || pending || !message.trim()) return Promise.resolve(false);
  return new Promise((resolve) => {
    const request = {
      message,
      resolve,
      origin: document.activeElement instanceof HTMLElement ? document.activeElement : null,
    };
    pending = request;
    try {
      host?.(request);
    } catch {
      finish(false);
    }
  });
}
function finish(accepted: boolean, expected: Request | null = pending) {
  if (pending !== expected) return;
  const request = pending;
  pending = null;
  try {
    closeDialog?.();
    host?.(null);
  } catch {
    accepted = false;
  } finally {
    request?.resolve(accepted === true);
    restoreFocus(request?.origin);
  }
}

export function ConfirmationHost() {
  const [request, setRequest] = useState<Request | null>(null);
  const dialog = useRef<HTMLDialogElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (host) return;
    host = setRequest;
    return () => {
      if (host !== setRequest) return;
      finish(false);
      host = null;
    };
  }, []);
  useEffect(() => {
    if (!request) return;
    try {
      dialog.current?.showModal();
      closeDialog = () => dialog.current?.close();
      cancel.current?.focus();
    } catch {
      finish(false, request);
    }
    return () => {
      closeDialog = null;
    };
  }, [request]);
  if (!request) return null;
  return createPortal(
    <dialog
      ref={dialog}
      aria-label="작업 확인"
      aria-describedby="devbox-confirm-message"
      onCancel={(event) => {
        event.preventDefault();
        finish(false, request);
      }}
      onKeyDown={(event) => {
        if (dialog.current) trapDialogKeyDown(event, dialog.current, () => finish(false, request));
      }}
      className="product-confirm-dialog"
    >
      <h2>작업 확인</h2>
      <p id="devbox-confirm-message">{request.message}</p>
      <div className="product-confirm-actions">
        <button ref={cancel} type="button" onClick={() => finish(false, request)}>
          취소
        </button>
        <button type="button" onClick={() => finish(true, request)}>
          확인
        </button>
      </div>
    </dialog>,
    document.body,
  );
}
