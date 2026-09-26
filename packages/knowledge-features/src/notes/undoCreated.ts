import { undoCreatedNote } from "./api";
import type { NoteDocument } from "./noteDocument";
import type { QuickCaptureSaved } from "./types";

export async function undoCreated(
  created: QuickCaptureSaved,
  editor: NoteDocument | null,
  remove = undoCreatedNote,
): Promise<void> {
  const view = editor?.snapshot();
  if (view?.path === created.path && (view.dirty || view.saving)) {
    throw new Error("이미 수정되어 되돌리지 않았습니다.");
  }
  const complete = editor?.approveRemoval(created.path);
  const result = await remove(created.path, created.revision);
  if (!result.removed) throw new Error("이미 수정되어 되돌리지 않았습니다.");
  complete?.();
}
