import { EditorView } from "@codemirror/view";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { DocKey } from "../../../rpc/gen/rpc";
import { EditorHost } from "../../documents/EditorHost";
import { keyId } from "../../documents/offlineJournal";
import { useDocuments } from "../../documents/store";
import { CompareView } from "../../notes/CompareView";
import { OpenError, ReadOnlyBar } from "../../notes/ReadOnlyBar";
import { RecoveryView } from "../../notes/RecoveryView";
import { SaveStatus } from "../../notes/SaveStatus";
import { sendToTransforms } from "../../tools/transforms/handoff";
import { Preview } from "../Preview";
import { canPreview } from "../previewHelpers";
import s from "../source.module.css";
import { codeExtensions } from "./extensions";
import { LspStatus } from "./LspStatus";
import { detectIndent, detectLanguage } from "./languages";
import { type CodeLocation, offsetAt } from "./location";
import { useEditorLsp } from "./lsp";

export function CodeEditor({ docKey, location }: { docKey: DocKey; location?: CodeLocation & { request?: number } }) {
  const [preview, setPreview] = useState(false);
  const viewRef = useRef<EditorView | null>(null);
  const [hasSelection, setHasSelection] = useState(false);
  const selectionUpdates = useMemo(
    () =>
      EditorView.updateListener.of((update) => {
        if (update.selectionSet || update.docChanged)
          setHasSelection(update.state.selection.ranges.some((range) => !range.empty));
      }),
    [],
  );
  const id = keyId(docKey);
  const key = useMemo(() => ({ root: docKey.root, path: docKey.path }), [docKey.root, docKey.path]);
  const doc = useDocuments((state) => Object.values(state.docs).find((item) => keyId(item.key) === id));
  useEffect(() => {
    useDocuments.getState().open(key, { kind: "code" });
  }, [key]);
  const lsp = useEditorLsp(key);
  const indent = detectIndent(doc?.text ?? "");
  const extensions = useMemo(
    () => [codeExtensions(indent), lsp.extension, selectionUpdates],
    [indent, lsp.extension, selectionUpdates],
  );
  const lastLocation = useRef<string | undefined>(undefined);
  const onView = useCallback(
    (view: EditorView | null) => {
      viewRef.current = view;
      setHasSelection(view?.state.selection.ranges.some((range) => !range.empty) ?? false);
      if (!view || !location) return;
      const request = JSON.stringify([id, location]);
      if (lastLocation.current === request) return;
      lastLocation.current = request;
      const anchor = offsetAt(view.state, location);
      view.dispatch({ selection: { anchor }, effects: EditorView.scrollIntoView(anchor, { y: "center" }) });
      view.focus();
    },
    [id, location],
  );
  return (
    <article className={s.editor} aria-label={`파일 ${key.path}`}>
      <header className={s.editorHeader}>
        <span title={key.path}>{key.path}</span>
        <SaveStatus docKey={key} />
        <button
          type="button"
          disabled={!hasSelection}
          onClick={() => {
            const view = viewRef.current;
            if (!view) return;
            // CodeMirror orders ranges by document position; empty cursors contribute no text.
            const text = view.state.selection.ranges
              .filter((range) => !range.empty)
              .map((range) => view.state.sliceDoc(range.from, range.to))
              .join("\n");
            void sendToTransforms(text, "editor");
          }}
        >
          선택을 변환 도구로 보내기
        </button>
        {canPreview(key.path) && (
          <button type="button" aria-pressed={preview} onClick={() => setPreview((open) => !open)}>
            미리보기
          </button>
        )}
      </header>
      {lsp.language && (
        <LspStatus key={`${key.root}:${lsp.language}`} projectId={key.root.slice(8)} language={lsp.language} />
      )}
      {lsp.message && <p role="status">{lsp.message}</p>}
      <OpenError docKey={key} />
      <ReadOnlyBar docKey={key} />
      <CompareView docKey={key} />
      <RecoveryView docKey={key} />
      {doc?.state === "Orphaned" && <p role="status">파일이 삭제되었습니다. 저장하면 현재 내용을 다시 만듭니다.</p>}
      <div className={`${s.editorBody} ${preview && canPreview(key.path) ? s.withPreview : ""}`}>
        <div className={s.codePane}>
          <EditorHost
            docKey={key}
            language={detectLanguage(key.path, doc?.text ?? "")}
            ariaLabel="코드 편집기"
            extensions={extensions}
            onView={onView}
          />
        </div>
        {preview && canPreview(key.path) && doc && (
          <div className={s.sourcePreview}>
            <Preview docKey={key} text={doc.text} />
          </div>
        )}
      </div>
    </article>
  );
}
