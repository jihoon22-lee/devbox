import { EditorView } from "@codemirror/view";
import { useCallback, useEffect, useMemo, useRef } from "react";
import type { DocKey } from "../../../rpc/gen/rpc";
import { EditorHost } from "../../documents/EditorHost";
import { keyId } from "../../documents/offlineJournal";
import { useDocuments } from "../../documents/store";
import { CompareView } from "../../notes/CompareView";
import { OpenError, ReadOnlyBar } from "../../notes/ReadOnlyBar";
import { RecoveryView } from "../../notes/RecoveryView";
import { SaveStatus } from "../../notes/SaveStatus";
import s from "../source.module.css";
import { codeExtensions } from "./extensions";
import { detectIndent, detectLanguage } from "./languages";
import { type CodeLocation, offsetAt } from "./location";

export function CodeEditor({ docKey, location }: { docKey: DocKey; location?: CodeLocation & { request?: number } }) {
  const id = keyId(docKey);
  const key = useMemo(() => ({ root: docKey.root, path: docKey.path }), [docKey.root, docKey.path]);
  const doc = useDocuments((state) => Object.values(state.docs).find((item) => keyId(item.key) === id));
  useEffect(() => {
    useDocuments.getState().open(key, { kind: "code" });
  }, [key]);
  const indent = detectIndent(doc?.text ?? "");
  const extensions = useMemo(() => codeExtensions(indent), [indent]);
  const lastLocation = useRef<string | undefined>(undefined);
  const onView = useCallback(
    (view: EditorView | null) => {
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
      </header>
      <OpenError docKey={key} />
      <ReadOnlyBar docKey={key} />
      <CompareView docKey={key} />
      <RecoveryView docKey={key} />
      {doc?.state === "Orphaned" && <p role="status">파일이 삭제되었습니다. 저장하면 현재 내용을 다시 만듭니다.</p>}
      <div className={s.editorBody}>
        <EditorHost
          docKey={key}
          language={detectLanguage(key.path, doc?.text ?? "")}
          ariaLabel="코드 편집기"
          extensions={extensions}
          onView={onView}
        />
      </div>
    </article>
  );
}
