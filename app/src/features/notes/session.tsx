import { useEffect } from "react";
import { useRpcSession } from "../../rpc/RpcProvider";
import { keyId, type OfflineJournal } from "../documents/offlineJournal";
import { useDocuments } from "../documents/store";
import { noteKey, VAULT } from "./notePaths";
import { listenRewrites } from "./rewrite";
import { useNoteSelection } from "./selection";

/**
 * Binds this window's document store to the daemon session and answers `notes.rewrite_request`
 * for the notes it holds. Mounted once per window (main and popouts) through `NotesCommands`.
 */
export function NotesSession({ journal, windowId }: { journal?: OfflineJournal; windowId?: string }) {
  const { client } = useRpcSession();
  useEffect(() => {
    const disconnect = useDocuments.getState().connect(client, {
      ...(journal ? { journal } : {}),
      ...(windowId ? { windowId } : {}),
    });
    const stopRewrites = listenRewrites(client);
    // A rename anywhere (drag, another window) re-keys the open document (`documents.moved`);
    // the selection follows that document, after the store moved it, so nothing claims the new path twice.
    const stopMoves = useDocuments.subscribe((now, before) => {
      const { path, select } = useNoteSelection.getState();
      if (!path) return;
      const wanted = keyId(noteKey(path));
      const shown = Object.values(before.docs).find((d) => keyId(d.key) === wanted);
      const moved = shown && now.docs[shown.id];
      if (moved && keyId(moved.key) !== wanted && moved.key.root === VAULT) select(moved.key.path);
    });
    return () => {
      stopMoves();
      stopRewrites();
      disconnect();
    };
  }, [client, journal, windowId]);
  return null;
}
