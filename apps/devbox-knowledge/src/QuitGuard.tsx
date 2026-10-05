import { collectorMessage, useCollectorStatus } from "./collectorStatus";
import { useEffect, useRef, useState } from "react";
import { quitCall } from "@devbox/knowledge-features/commands/api";
import {
  hasUnsavedNote,
  saveNoteBeforeQuit,
  prepareNoteQuit,
  resumeNoteAfterQuit,
} from "@devbox/knowledge-features/notes-lifecycle";
import { nativeMode } from "@devbox/product-shell/api";
import { focusFirst, isImeComposing, restoreFocus, trapDialogKeyDown } from "@devbox/a11y";

export default function QuitGuard() {
  const [request, setRequest] = useState<string | null>(null);
  const collectorStatus = useCollectorStatus(request);
  const [busy, setBusy] = useState(false);
  const [permanent, setPermanent] = useState(false);
  const [error, setError] = useState("");
  const dialog = useRef<HTMLDialogElement>(null);
  const handling = useRef(false);
  useEffect(() => {
    if (!nativeMode) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const pending = async () => {
      if (handling.current) return;
      handling.current = true;
      try {
        const id = await quitCall("pending_quit", {});
        if (disposed || !id) return;
        if (!hasUnsavedNote()) {
          await prepareNoteQuit();
          await quitCall("decide_quit", { id, quit: true });
        } else {
          setPermanent(false);
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
    const element = dialog.current;
    element?.showModal();
    if (element) focusFirst(element);
    return () => {
      element?.close();
      restoreFocus(previous);
    };
  }, [request]);
  const decide = async (action: "save" | "discard" | "permanent" | "cancel") => {
    if (!request || busy) return;
    setBusy(true);
    setError("");
    try {
      if (action === "discard" || action === "permanent") await prepareNoteQuit(action === "permanent");
      if (action === "cancel") resumeNoteAfterQuit();
      if (action === "save" && !(await saveNoteBeforeQuit())) {
        setError("저장을 완료하지 못했습니다. 종료를 취소한 뒤 저장 오류나 파일 충돌을 확인해 주세요.");
        return;
      }
      if (action === "save") await prepareNoteQuit();
      await quitCall("decide_quit", { id: request, quit: action !== "cancel" });
      setRequest(null);
    } catch {
      resumeNoteAfterQuit();
      setError("종료를 완료하지 못했습니다. 현재 편집 내용은 유지됩니다.");
    } finally {
      setBusy(false);
    }
  };
  if (!request) return error ? <p role="alert">{error}</p> : null;
  return (
    <dialog
      ref={dialog}
      aria-modal="true"
      aria-labelledby="knowledge-quit-title"
      className="knowledge-quit-dialog"
      onCancel={(event) => {
        event.preventDefault();
        if (!busy) void decide("cancel");
      }}
      onKeyDown={(event) => {
        if (isImeComposing(event)) {
          if (event.key === "Escape") event.preventDefault();
          return;
        }
        if (event.key === "Escape" && !busy) {
          event.preventDefault();
          void decide("cancel");
        } else if (dialog.current) trapDialogKeyDown(event, dialog.current);
      }}
    >
      <h2 id="knowledge-quit-title">저장하지 않은 노트가 있습니다</h2>
      <p>{collectorMessage(collectorStatus)}</p>
      {error && <p role="alert">{error}</p>}
      <button disabled={busy} onClick={() => void decide("save")}>
        저장하고 종료
      </button>
      <button disabled={busy} onClick={() => void decide("discard")}>
        저장하지 않고 종료(복구본 유지)
      </button>
      {permanent ? (
        <div role="group" aria-label="복구본 영구 삭제 확인">
          <p>현재 노트의 복구본을 영구 삭제하고 종료합니다. 되돌릴 수 없습니다.</p>
          <button disabled={busy} onClick={() => void decide("permanent")}>
            영구 삭제하고 종료
          </button>
          <button disabled={busy} onClick={() => setPermanent(false)}>
            삭제 취소
          </button>
        </div>
      ) : (
        <button disabled={busy} onClick={() => setPermanent(true)}>
          복구본 영구 삭제…
        </button>
      )}
      <button disabled={busy} onClick={() => void decide("cancel")}>
        종료 취소
      </button>
    </dialog>
  );
}
