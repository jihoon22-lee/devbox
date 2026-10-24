import { ExternalLink, Link2, SquareSplitHorizontal } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { IconButton, SplitPane } from "../../ui";
import { keyId } from "../documents/offlineJournal";
import { useDocuments } from "../documents/store";
import { Backlinks } from "./Backlinks";
import { CompareView } from "./CompareView";
import { InsertSummary } from "./InsertSummary";
import { NoteEditor } from "./NoteEditor";
import { NoteTaskButton } from "./NoteTaskButton";
import { noteKey, stem } from "./notePaths";
import s from "./notes.module.css";
import { Preview } from "./Preview";
import { openNotePopout } from "./popoutNote";
import { OpenError, ReadOnlyBar } from "./ReadOnlyBar";
import { RecoveryView } from "./RecoveryView";
import { SaveStatus } from "./SaveStatus";
import { useNoteTree } from "./useNoteTree";

/**
 * One open note: header with the save state, the read-only / conflict / recovery panels, and the
 * editor with an optional preview split and the backlinks panel. The editor stays mounted while
 * the panels come and go, so every note keeps its own undo history.
 */
export function NoteView({
  path,
  popout = false,
  onOpen,
}: {
  path: string;
  popout?: boolean;
  onOpen(path: string): void;
}) {
  const host = useRef<HTMLElement>(null);
  const key = useMemo(() => noteKey(path), [path]);
  const tree = useNoteTree();
  const candidates = useMemo(() => (tree.data?.items ?? []).filter((item) => !item.dir), [tree.data]);
  const title = candidates.find((item) => item.path === path)?.title.trim() || stem(path);
  const text = useDocuments((st) => Object.values(st.docs).find((d) => keyId(d.key) === keyId(key))?.text);
  const [preview, setPreview] = useState(false);
  const [ratio, setRatio] = useState(0.5);
  const [backlinks, setBacklinks] = useState(!popout);
  return (
    <article ref={host} className={s.note} aria-label={`노트 ${title}`}>
      <header className={s.noteHeader}>
        <h2 title={path}>{title}</h2>
        <SaveStatus docKey={key} />
        <InsertSummary onOpen={onOpen} />
        <NoteTaskButton host={host} text={text} />
        <span className={s.spacer} />
        <IconButton
          label="미리보기 분할"
          pressed={preview}
          icon={<SquareSplitHorizontal size={16} />}
          onClick={() => setPreview((on) => !on)}
        />
        <IconButton
          label="백링크"
          pressed={backlinks}
          icon={<Link2 size={16} />}
          onClick={() => setBacklinks((on) => !on)}
        />
        {!popout && (
          <IconButton
            label="새 창으로 열기"
            icon={<ExternalLink size={16} />}
            onClick={() => void openNotePopout(path)}
          />
        )}
      </header>
      <OpenError docKey={key} />
      <ReadOnlyBar docKey={key} />
      <CompareView docKey={key} />
      <RecoveryView docKey={key} />
      <div className={s.workspace} data-preview={preview ? "on" : "off"}>
        <SplitPane
          dir="row"
          ratio={preview ? ratio : 1}
          min={preview ? 0.15 : 0}
          onRatio={setRatio}
          label="미리보기 너비"
        >
          <NoteEditor path={path} candidates={candidates} />
          {preview && text !== undefined ? <Preview path={path} text={text} onOpenNote={onOpen} /> : null}
        </SplitPane>
        {backlinks && <Backlinks path={path} onOpen={onOpen} />}
      </div>
    </article>
  );
}
