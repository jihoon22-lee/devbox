import { useEffect, useRef, useState } from "react";
import { componentInvoke } from "@devbox/knowledge-features/transport";
import { hasUnsavedNote, saveNoteBeforeQuit, settleNoteBeforeQuit } from "@devbox/knowledge-features/notes-lifecycle";
import { nativeMode } from "@devbox/product-shell/api";
import { focusFirst, isImeComposing, restoreFocus, trapDialogKeyDown } from "@devbox/a11y";
const invoke = componentInvoke("knowledge.commands");

export default function QuitGuard() {
  const [request, setRequest] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const dialog = useRef<HTMLElement>(null);
  const handling = useRef(false);
  useEffect(() => {
    if (!nativeMode) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const pending = async () => {
      if (handling.current) return;
      handling.current = true;
      try {
        const id = await invoke<string | null>("pending_quit");
        if (disposed || !id) return;
        if (!hasUnsavedNote()) await invoke("decide_quit", { id, quit: true });
        else {
          setRequest(id);
          setError("");
        }
      } catch {
        if (!disposed) setError("종료 요청을 확인하지 못했습니다. 다시 종료를 선택해 주세요.");
      } finally {
        handling.current = false;
      }
    };
    void import("@tauri-apps/api/event")
      .then(async ({ listen }) => {
        const stop = await listen("knowledge://quit-request", () => {
          void pending();
        });
        if (disposed) stop();
        else {
          unlisten = stop;
          void pending();
        }
      })
      .catch(() => {
        if (!disposed) setError("종료 알림에 연결하지 못했습니다. 편집 내용을 먼저 저장해 주세요.");
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
  useEffect(() => {
    if (!request) return;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    if (dialog.current) focusFirst(dialog.current);
    return () => {
      restoreFocus(previous);
    };
  }, [request]);
  const decide = async (action: "save" | "discard" | "cancel") => {
    if (!request || busy) return;
    setBusy(true);
    setError("");
    try {
      if (action === "discard") await settleNoteBeforeQuit();
      if (action === "save" && !(await saveNoteBeforeQuit())) {
        setError("저장을 완료하지 못했습니다. 종료를 취소한 뒤 저장 오류나 파일 충돌을 확인해 주세요.");
        return;
      }
      await invoke("decide_quit", { id: request, quit: action !== "cancel" });
      setRequest(null);
    } catch {
      setError("종료를 완료하지 못했습니다. 현재 편집 내용은 유지됩니다.");
    } finally {
      setBusy(false);
    }
  };
  if (!request) return error ? <p role="alert">{error}</p> : null;
  return (
    <div className="modal-backdrop">
      <section
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="knowledge-quit-title"
        className="rename-dialog"
        onKeyDown={(event) => {
          if (isImeComposing(event)) return;
          if (event.key === "Escape" && !busy) {
            event.preventDefault();
            void decide("cancel");
          } else if (dialog.current) trapDialogKeyDown(event, dialog.current);
        }}
      >
        <h2 id="knowledge-quit-title">저장하지 않은 노트가 있습니다</h2>
        <p>종료하면 Activity 수집도 중지됩니다.</p>
        {error && <p role="alert">{error}</p>}
        <button disabled={busy} onClick={() => void decide("save")}>
          저장하고 종료
        </button>
        <button disabled={busy} onClick={() => void decide("discard")}>
          버리고 종료
        </button>
        <button disabled={busy} onClick={() => void decide("cancel")}>
          종료 취소
        </button>
      </section>
    </div>
  );
}
