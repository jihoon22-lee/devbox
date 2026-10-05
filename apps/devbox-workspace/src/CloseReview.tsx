import { useEffect, useRef, useState } from "react";
import { isImeComposing, restoreFocus, trapDialogKeyDown } from "@devbox/a11y";
export default function CloseReview({
  reasons,
  filesDirty,
  onSave,
  onDiscard,
  onCancel,
  onReturn,
}: {
  reasons: string[];
  filesDirty: boolean;
  onSave(): Promise<void>;
  onDiscard(): Promise<void>;
  onCancel(): Promise<void>;
  onReturn(): Promise<void>;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const pending = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    const modal = dialog.current;
    if (!modal) return;
    const origin = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    modal.showModal();
    cancel.current?.focus();
    return () => {
      modal.close();
      restoreFocus(origin);
    };
  }, []);
  const run = async (action: () => Promise<void>) => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "종료 준비를 완료하지 못했습니다.");
    } finally {
      pending.current = false;
      setBusy(false);
    }
  };
  return (
    <dialog
      ref={dialog}
      className="workspace-close-review"
      aria-modal="true"
      aria-label="Workspace 종료 검토"
      aria-busy={busy}
      onCancel={(event) => {
        event.preventDefault();
        if (!pending.current) void run(onCancel);
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape" && isImeComposing(event)) {
          event.preventDefault();
          event.stopPropagation();
          return;
        }
        if (dialog.current)
          trapDialogKeyDown(event, dialog.current, () => {
            if (!pending.current) void run(onCancel);
          });
        event.stopPropagation();
      }}
    >
      <h2>Workspace를 닫기 전에 변경을 확인하세요</h2>
      <p>{reasons.length ? reasons.join(", ") : "마지막 복구 기록을 확인한 뒤 종료할 수 있습니다."}</p>
      {error && <p role="alert">{error}</p>}
      <div className="workspace-close-review-actions">
        <button type="button" disabled={busy} onClick={() => void run(onSave)}>
          {filesDirty ? "파일 저장 후 종료" : "종료"}
        </button>
        {filesDirty && (
          <button type="button" disabled={busy} onClick={() => void run(onDiscard)}>
            파일 변경 폐기 후 종료
          </button>
        )}
        <button type="button" disabled={busy} onClick={() => void run(onReturn)}>
          편집 화면으로 돌아가기
        </button>
        <button ref={cancel} type="button" disabled={busy} onClick={() => void run(onCancel)}>
          종료 취소
        </button>
      </div>
    </dialog>
  );
}
