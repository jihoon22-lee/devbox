import { useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useRef } from "react";
import { EmptyState, useToasts } from "../../ui";
import { keyId } from "../documents/offlineJournal";
import { useDocuments } from "../documents/store";
import { noteCloseHandler } from "./closeNoteWindow";
import { NoteView } from "./NoteView";
import { noteKey, VAULT } from "./notePaths";
import s from "./notes.module.css";

/** `/popout/note/$key`: one note in its own window (`$key` is the vault-relative path). */
export function PopoutNote() {
  const { key } = useParams({ strict: false }) as { key?: string };
  const navigate = useNavigate();
  const host = useRef<HTMLDivElement>(null);
  const path = key && /\.md$/i.test(key) ? key : undefined;
  const closing = useDocuments((state) => state.windowClosing);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let disposed = false;
    let off: (() => void) | undefined;
    const blocked = () =>
      useToasts
        .getState()
        .push({ tone: "warn", title: "저장하지 못해 창을 열어 두었습니다. 저장 상태와 충돌을 확인하세요" });
    void import("@tauri-apps/api/window")
      .then(async ({ getCurrentWindow }) => {
        if (disposed) return;
        const current = getCurrentWindow();
        const close = noteCloseHandler(() => current.destroy(), blocked);
        const unlisten = await current.onCloseRequested(async (event) => {
          // Native events are outside React's event batch: stop input synchronously,
          // before waiting for the render that makes editors read-only.
          if (host.current) host.current.inert = true;
          await close(event);
          if (host.current) host.current.inert = useDocuments.getState().windowClosing;
        });
        if (disposed) unlisten();
        else off = unlisten;
      })
      .catch(blocked);
    return () => {
      disposed = true;
      off?.();
    };
  }, []);

  useEffect(() => {
    // Advance synchronously as well as navigating: multiple moves can arrive before the
    // router commits its next render, and every move must keep following this document.
    let watchedPath = path;
    return useDocuments.subscribe((now, before) => {
      if (!watchedPath) return;
      const wanted = keyId(noteKey(watchedPath));
      const shown = Object.values(before.docs).find((d) => keyId(d.key) === wanted);
      const moved = shown && now.docs[shown.id];
      if (moved && moved.key.root === VAULT && moved.key.path !== watchedPath) {
        watchedPath = moved.key.path;
        void navigate({ to: "/popout/note/$key", params: { key: moved.key.path }, replace: true });
      }
    });
  }, [path, navigate]);
  return (
    <div ref={host} className={s.popout} inert={closing}>
      {path ? (
        <NoteView
          path={path}
          popout
          onOpen={(target) => void navigate({ to: "/popout/note/$key", params: { key: target } })}
        />
      ) : (
        <EmptyState title="노트를 찾을 수 없습니다" />
      )}
    </div>
  );
}
