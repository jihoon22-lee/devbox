import { useNavigate, useParams } from "@tanstack/react-router";
import { useEffect } from "react";
import { EmptyState } from "../../ui";
import { keyId } from "../documents/offlineJournal";
import { useDocuments } from "../documents/store";
import { NoteView } from "./NoteView";
import { noteKey, VAULT } from "./notePaths";
import s from "./notes.module.css";

/** `/popout/note/$key`: one note in its own window (`$key` is the vault-relative path). */
export function PopoutNote() {
  const { key } = useParams({ strict: false }) as { key?: string };
  const navigate = useNavigate();
  const path = key && /\.md$/i.test(key) ? key : undefined;
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
    <div className={s.popout}>
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
