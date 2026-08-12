import type { Doc, DocId, ViewId } from "../types";
import TabBar from "./TabBar";

interface ViewPaneProps {
  view: ViewId;
  docs: Doc[];
  docIds: DocId[];
  activeDocId: DocId | null;
  onActivateDoc: (view: ViewId, docId: DocId) => void;
  onCloseDoc: (docId: DocId) => void;
  onMoveDoc: (docId: DocId, toView: ViewId) => void;
  hidden?: boolean;
}

export default function ViewPane({
  view,
  docs,
  docIds,
  activeDocId,
  onActivateDoc,
  onCloseDoc,
  onMoveDoc,
  hidden = false,
}: ViewPaneProps) {
  return (
    <section
      className={`view-pane view-pane-${view}`}
      aria-label={`${view + 1}번 편집 뷰`}
      aria-hidden={hidden}
      style={hidden ? { display: "none" } : undefined}
    >
      <div className="view-pane-title">
        <span>뷰 {view + 1}</span>
        <span className="view-pane-count">{docIds.length}개</span>
      </div>
      <TabBar
        view={view}
        docs={docs}
        docIds={docIds}
        activeDocId={activeDocId}
        onActivate={(docId) => onActivateDoc(view, docId)}
        onClose={onCloseDoc}
        onMove={(docId) => onMoveDoc(docId, view === 0 ? 1 : 0)}
      />
    </section>
  );
}
