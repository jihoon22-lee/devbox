import { useToasts } from "../../ui";
import { useDocuments } from "../documents/store";
import { openPopout } from "../terminal/popout";
import { noteKey } from "./notePaths";
import { useNoteSelection } from "./selection";

const HANDOFF_MS = 5_000;

/** Saves and closes the note here so the new window gets the lease; false if it cannot close yet. */
async function release(path: string): Promise<boolean> {
  const key = noteKey(path);
  const docs = useDocuments.getState();
  const doc = docs.find(key);
  if (!doc) return true;
  if (doc.state === "Conflict" || doc.state === "Recovering") return false;
  docs.close(key, "save");
  return new Promise((resolve) => {
    let off = () => {};
    const done = (ok: boolean) => {
      clearTimeout(timer);
      off();
      resolve(ok);
    };
    const timer = setTimeout(() => done(!useDocuments.getState().find(key)), HANDOFF_MS);
    off = useDocuments.subscribe((st) => {
      const current = Object.values(st.docs).find((d) => d.id === doc.id);
      if (!current) done(true);
      else if (current.state === "Conflict") done(false);
    });
  });
}

/** Opens the note in its own window (`/popout/note/<path>`); this window lets go of it first. */
export async function openNotePopout(path: string): Promise<void> {
  if (!(await release(path))) {
    useToasts.getState().push({ tone: "warn", title: "충돌이나 복구할 초안을 먼저 정리한 뒤 새 창으로 여세요" });
    return;
  }
  if (useNoteSelection.getState().path === path) useNoteSelection.getState().select(undefined);
  try {
    await openPopout("note", path);
  } catch {
    useToasts.getState().push({ tone: "danger", title: "새 창을 열지 못했습니다" });
  }
}
