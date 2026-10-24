import { ChevronDown, ChevronRight, Ellipsis, FileText, Folder } from "lucide-react";
import { type DragEvent, useMemo, useState } from "react";
import { useRpcSession } from "../../rpc/RpcProvider";
import { Button, EmptyState, ErrorState, Menu } from "../../ui";
import { VirtualList } from "../../ui/VirtualList";
import { useDocuments } from "../documents/store";
import { fileName, folderOf, joinPath, noteKey } from "./notePaths";
import s from "./notes.module.css";
import { openNotePopout } from "./openNoteWindow";
import { RenameDialog } from "./RenameDialog";
import { renameNote } from "./renameStatus";
import { useNoteSelection } from "./selection";
import { moveToTrash } from "./trash";
import { type TreeRow, treeRows } from "./treeRows";
import { useNoteTree } from "./useNoteTree";

const DRAG_TYPE = "application/x-devbox-note";

/** Folder tree of the vault. Dragging a note onto a folder (or the empty area for the root) moves it. */
export function NoteTree() {
  const { client } = useRpcSession();
  const tree = useNoteTree();
  const selected = useNoteSelection((st) => st.path);
  const select = useNoteSelection((st) => st.select);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [renaming, setRenaming] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<string | null>(null);
  const rows = useMemo(() => treeRows(tree.data?.items ?? [], collapsed), [tree.data, collapsed]);

  // The selection follows only after the daemon moved the note (and `documents.moved` re-keyed it),
  // so the editor never claims the new path while the old document is still open.
  const relocate = (from: string, to: string) => {
    if (to === from) return;
    void renameNote(client, from, to).then((outcome) => {
      if (useNoteSelection.getState().path !== from) return;
      const moved = outcome === "moved" || (outcome === "unknown" && !!useDocuments.getState().find(noteKey(to)));
      if (moved) select(to);
    });
  };
  const move = (from: string, folder: string) => relocate(from, joinPath(folder, fileName(from)));
  const dropOn = (folder: string) => ({
    onDragOver: (event: DragEvent) => {
      if (!event.dataTransfer.types.includes(DRAG_TYPE)) return;
      event.preventDefault();
      event.stopPropagation();
      setDropTarget(folder);
    },
    onDragLeave: () => setDropTarget((current) => (current === folder ? null : current)),
    onDrop: (event: DragEvent) => {
      const from = event.dataTransfer.getData(DRAG_TYPE);
      setDropTarget(null);
      if (!from) return;
      event.preventDefault();
      event.stopPropagation();
      move(from, folder);
    },
  });
  const toggle = (path: string) =>
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });

  const renderRow = (row: TreeRow) => (
    <div
      className={s.treeRow}
      style={{ paddingLeft: `calc(var(--space-2) + ${row.depth * 14}px)` }}
      data-selected={row.path === selected || undefined}
      data-drop={(row.dir && dropTarget === row.path) || undefined}
      draggable={!row.dir}
      onDragStart={(event) => {
        event.dataTransfer.setData(DRAG_TYPE, row.path);
        event.dataTransfer.effectAllowed = "move";
      }}
      {...dropOn(row.dir ? row.path : folderOf(row.path))}
    >
      <button
        type="button"
        className={s.treeButton}
        aria-current={row.path === selected ? "page" : undefined}
        aria-expanded={row.dir ? !collapsed.has(row.path) : undefined}
        title={row.path}
        onClick={() => (row.dir ? toggle(row.path) : select(row.path))}
      >
        {row.dir ? (
          collapsed.has(row.path) ? (
            <ChevronRight size={14} aria-hidden="true" />
          ) : (
            <ChevronDown size={14} aria-hidden="true" />
          )
        ) : null}
        {row.dir ? <Folder size={14} aria-hidden="true" /> : <FileText size={14} aria-hidden="true" />}
        <span className={s.treeLabel}>{row.label}</span>
      </button>
      {!row.dir && (
        <Menu
          trigger={
            <Button size="sm" variant="ghost" aria-label={`${row.label} 메뉴`} className={s.rowMenu}>
              <Ellipsis size={14} aria-hidden="true" />
            </Button>
          }
          items={[
            { label: "이름 바꾸기", onSelect: () => setRenaming(row.path), afterClose: true },
            {
              label: "새 창으로 열기",
              onSelect: () => void openNotePopout(row.path),
            },
            { label: "휴지통으로 옮기기", danger: true, onSelect: () => void trash(row.path) },
          ]}
        />
      )}
    </div>
  );
  const trash = async (path: string) => {
    if ((await moveToTrash(client, path)) && useNoteSelection.getState().path === path) select(undefined);
  };

  if (tree.isError)
    return (
      <ErrorState
        title="노트 목록을 읽지 못했습니다"
        action={<Button onClick={() => void tree.refetch()}>다시 시도</Button>}
      />
    );
  if (tree.data && !rows.length)
    return <EmptyState title="노트가 없습니다" body="새 노트를 만들거나 일일 기록을 열어 시작하세요." />;
  return (
    <div className={s.tree} data-drop={dropTarget === "" || undefined} {...dropOn("")}>
      {tree.data?.truncated && <p className={s.notice}>노트가 많아 앞의 20,000개만 보입니다.</p>}
      <VirtualList items={rows} renderItem={renderRow} label="노트 목록" rowHeight={28} />
      <RenameDialog path={renaming} onClose={() => setRenaming(null)} onRename={relocate} />
    </div>
  );
}
