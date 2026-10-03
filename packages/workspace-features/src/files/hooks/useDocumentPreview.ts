import { useEffect, type RefObject, type Dispatch, type SetStateAction } from "react";
import { renderPreview } from "../api";
import { isPreviewable, snapshotMatches, safeCodePadError } from "../lib/documentPresentation";
import type { Doc, EditorState, PreviewResponse } from "../types";

interface DocumentPreviewOptions {
  previewOpen: boolean;
  activeDoc: Doc | null | undefined;
  workspaceFolder: string | null;
  stateRef: RefObject<EditorState>;
  setPreview: Dispatch<SetStateAction<PreviewResponse | null>>;
  setPreviewError: Dispatch<SetStateAction<string | null>>;
}

export function useDocumentPreview({
  previewOpen,
  activeDoc,
  workspaceFolder,
  stateRef,
  setPreview,
  setPreviewError,
}: DocumentPreviewOptions) {
  // Preview requests are tied to the document revision and discarded if a
  // newer edit arrives before the native render returns.
  // biome-ignore lint/correctness/useExhaustiveDependencies: Preview is keyed by document id/revision/text and workspace. Cursor or watcher metadata changes also replace activeDoc but must not restart native rendering.
  useEffect(() => {
    if (!previewOpen || !activeDoc || !workspaceFolder || !isPreviewable(activeDoc.path)) {
      setPreview(null);
      setPreviewError(null);
      return;
    }
    const expected = { ...activeDoc };
    let cancelled = false;
    setPreviewError(null);
    void renderPreview(activeDoc.path, activeDoc.text, workspaceFolder)
      .then((response) => {
        const latest = stateRef.current.docs.find((doc) => doc.id === expected.id);
        if (cancelled || !latest || !snapshotMatches(latest, expected)) return;
        setPreview(response);
      })
      .catch((cause) => {
        const latest = stateRef.current.docs.find((doc) => doc.id === expected.id);
        if (cancelled || !latest || !snapshotMatches(latest, expected)) return;
        setPreviewError(safeCodePadError(cause, "미리보기를 생성하지 못했습니다."));
      });
    return () => {
      cancelled = true;
    };
  }, [previewOpen, activeDoc?.id, activeDoc?.revision, activeDoc?.text, workspaceFolder]);
}
