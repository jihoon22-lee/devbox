import { useNavigate, useSearch } from "@tanstack/react-router";
import { useEffect } from "react";
import { SectionLayout } from "../../shell/SectionLayout";
import { EmptyState } from "../../ui";
import { NotesList } from "./NotesList";
import { NoteView } from "./NoteView";
import { RenameReport } from "./RenameReport";
import { useNoteSelection } from "./selection";

export type NotesSearch = {
  path?: string;
  rename?: string;
};
export const notesSearch = (search: Record<string, unknown>): NotesSearch => ({
  ...(typeof search.path === "string" && /\.md$/i.test(search.path) ? { path: search.path } : {}),
  ...(typeof search.rename === "string" && /^[0-9a-f]{1,64}$/.test(search.rename) ? { rename: search.rename } : {}),
});

/** The notes section: list on the left, the selected note on the right. */
export function NotesPage({ rename, onDismissRename }: { rename?: string; onDismissRename?: () => void }) {
  const path = useNoteSelection((st) => st.path);
  const select = useNoteSelection((st) => st.select);
  return (
    <SectionLayout list={<NotesList />}>
      {rename && <RenameReport opId={rename} onOpen={select} onDismiss={() => onDismissRename?.()} />}
      {path ? (
        <NoteView path={path} onOpen={select} />
      ) : (
        <EmptyState title="노트를 고르세요" body="왼쪽 목록에서 노트를 고르거나 Ctrl+N으로 새 노트를 만드세요." />
      )}
    </SectionLayout>
  );
}

/** Route adapter: `?path=` (search results, notifications) selects a note; `?rename=` shows a rename report. */
export function NotesRoute() {
  const search = useSearch({ strict: false }) as NotesSearch;
  const navigate = useNavigate();
  useEffect(() => {
    if (search.path) useNoteSelection.getState().select(search.path);
  }, [search.path]);
  return (
    <NotesPage
      {...(search.rename ? { rename: search.rename } : {})}
      onDismissRename={() =>
        void navigate({
          to: "/notes",
          search: (previous: Record<string, unknown>) => ({ ...previous, rename: undefined }),
        })
      }
    />
  );
}
