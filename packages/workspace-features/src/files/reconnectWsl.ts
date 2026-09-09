import { componentInvoke, WorkspaceOperationError } from "../transport";
import { openFile, watchFile } from "./api";
import type { Doc, OpenedFile } from "./types";

interface Editor {
  current(): readonly Doc[];
  active(): boolean;
  replace(doc: Doc): void;
  conflict(path: string): void;
}
interface Connection {
  reconnect(): Promise<void>;
  open: typeof openFile;
  watch: typeof watchFile;
}
const native: Connection = {
  reconnect: () => componentInvoke("workspace.files")("reconnect_wsl_files"),
  open: openFile,
  watch: watchFile,
};

/** Reconcile one file at a time: bounded native responses, no save replay, and
 * no replacement of edits which arrived while disk reads were pending. */
export async function reconnectWsl(editor: Editor, connection = native): Promise<void> {
  const documents = editor.current().filter(doc => doc.path.startsWith("/"));
  await connection.reconnect();
  if (!editor.active()) return;
  let unavailable = false;
  for (const before of documents) {
    if (!editor.active()) return;
    let opened: OpenedFile;
    try { opened = await connection.open(before.path, before.encoding); }
    catch {
      if (!editor.active()) return;
      unavailable = true;
      editor.conflict(before.path);
      continue;
    }
    if (!editor.active()) return;
    const latest = editor.current().find(doc => doc.id === before.id);
    if (!latest || latest.path !== before.path) continue;
    const sameDisk = opened.contentHash === before.contentHash && opened.size === before.size;
    if (latest.nativeRevision !== before.nativeRevision || (!sameDisk && (latest.dirty || latest.revision !== before.revision))) {
      editor.conflict(before.path);
    } else {
      editor.replace(sameDisk ? {
        ...latest,
        nativeRevision: opened.nativeRevision,
        mtimeNanos: opened.mtimeNanos,
        readOnly: opened.readOnly,
        lossy: opened.lossy,
      } : {
        ...latest, ...opened, dirty: false, revision: latest.revision + 1,
        cursor: Math.min(latest.cursor, opened.text.length),
      });
    }
    try { await connection.watch(before.path); }
    catch { unavailable = true; }
  }
  if (unavailable && editor.active()) throw new WorkspaceOperationError("일부 WSL 파일을 다시 연결하지 못했습니다. 편집 내용은 유지됩니다. 파일과 배포판 상태를 확인한 뒤 다시 시도해 주세요.");
}
