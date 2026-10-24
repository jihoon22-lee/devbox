import { useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useRef } from "react";
import { EmptyState, useToasts } from "../../ui";
import { chooseDocumentClose } from "../documents/closePrompt";
import { keyId } from "../documents/offlineJournal";
import { useDocuments } from "../documents/store";
import { noteCloseHandler } from "../notes/closeNoteWindow";
import { CodeEditor } from "./editor/CodeEditor";
import { decodeCodeKey } from "./editor/location";
import s from "./source.module.css";
export function PopoutCode() {
  const params = useParams({ strict: false }) as { key?: string };
  const key = decodeCodeKey(params.key);
  const navigate = useNavigate();
  const host = useRef<HTMLDivElement>(null);
  const closing = useDocuments((s) => s.windowClosing);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let disposed = false;
    let off: (() => void) | undefined;
    const blocked = () =>
      useToasts
        .getState()
        .push({ tone: "warn", title: "파일을 닫지 못해 창을 열어 두었습니다. 저장 상태를 확인하세요." });
    void import("@tauri-apps/api/window")
      .then(async ({ getCurrentWindow }) => {
        if (disposed) return;
        const current = getCurrentWindow();
        const close = noteCloseHandler(
          () => current.destroy(),
          blocked,
          () => useDocuments.getState().prepareWindowClose(chooseDocumentClose),
        );
        const stop = await current.onCloseRequested(async (event) => {
          if (host.current) host.current.inert = true;
          await close(event);
          if (host.current) host.current.inert = useDocuments.getState().windowClosing;
        });
        if (disposed) stop();
        else off = stop;
      })
      .catch(blocked);
    return () => {
      disposed = true;
      off?.();
    };
  }, []);
  const identity = key ? keyId(key) : undefined;
  useEffect(() => {
    let watched = identity;
    return useDocuments.subscribe((now, before) => {
      const prior = Object.values(before.docs).find((doc) => keyId(doc.key) === watched);
      const moved = prior && now.docs[prior.id];
      if (moved && keyId(moved.key) !== watched) {
        watched = keyId(moved.key);
        void navigate({ to: "/popout/code/$key", params: { key: watched }, replace: true });
      }
    });
  }, [identity, navigate]);
  return (
    <div ref={host} className={s.popout} inert={closing}>
      {key ? <CodeEditor docKey={key} /> : <EmptyState title="파일 경로가 올바르지 않습니다" />}
    </div>
  );
}
