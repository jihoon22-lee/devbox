import type { Extension } from "@codemirror/state";
import { useEffect, useMemo, useRef } from "react";
import { useRpcSession } from "../../rpc/RpcProvider";
import { useToasts } from "../../ui";
import { EditorHost } from "../documents/EditorHost";
import { useDocuments } from "../documents/store";
import { noteKey } from "./notePaths";
import { imagePaste } from "./pasteImage";
import { type WikiCandidate, wikiCompletion } from "./wikiComplete";

/**
 * The note editor: the shared `EditorHost` (one view, one state per note) with Markdown,
 * `[[` completion from the vault list and image paste (`notes.attach`). Opening claims the
 * note; it stays open (with its undo history) while this window lives.
 */
export function NoteEditor({ path, candidates }: { path: string; candidates: WikiCandidate[] }) {
  const { client } = useRpcSession();
  const key = useMemo(() => noteKey(path), [path]);
  const current = useRef(path);
  const items = useRef(candidates);
  useEffect(() => {
    current.current = path;
    items.current = candidates;
  });
  useEffect(() => {
    useDocuments.getState().open(key, { kind: "note" });
  }, [key]);
  const extensions = useMemo<Extension>(
    () => [
      wikiCompletion(
        () => items.current,
        () => current.current,
      ),
      imagePaste(() => {
        const at = current.current;
        return {
          notePath: at,
          attach: (params) => client.call("notes.attach", params),
          onError: (title) => useToasts.getState().push({ tone: "danger", title }),
          stillCurrent: () => current.current === at,
        };
      }),
    ],
    [client],
  );
  return <EditorHost docKey={key} ariaLabel="노트 편집기" extensions={extensions} />;
}
