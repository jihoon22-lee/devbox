import { setDiagnostics } from "@codemirror/lint";
import { LSPClient, LSPPlugin, Workspace, type WorkspaceFile } from "@codemirror/lsp-client";
import { ChangeSet, Text, type TransactionSpec } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import type { DocKey } from "../../../rpc/gen/rpc";
import { activeEditors, editorStates } from "../../documents/editorStates";
import { useDocuments } from "../../documents/store";
import type { ProjectUris } from "./lspWorkspace";

interface File extends WorkspaceFile {
  key: DocKey;
  id: string;
  leaseId: string;
  views: Set<EditorView>;
}
/** Store-backed documents remain synchronized when their editor tab is inactive. */
export class SourceLspFiles extends Workspace {
  files: File[] = [];
  private live = true;
  private timer?: ReturnType<typeof setTimeout>;
  private unsubscribe: () => void;
  constructor(
    client: LSPClient,
    readonly uris: ProjectUris,
    private invalidate: () => void,
  ) {
    super(client);
    this.unsubscribe = useDocuments.subscribe(() => {
      if (this.live && !this.timer)
        this.timer = setTimeout(() => {
          this.timer = undefined;
          if (this.live) {
            try {
              this.client.sync();
            } catch {
              this.invalidate();
            }
          }
        }, 100);
    });
  }
  openFile(uri: string, languageId: string, view: EditorView) {
    if (!this.live) return;
    const known = this.getFile(uri) as File | null;
    if (known) {
      known.views.add(view);
      return;
    }
    const key = this.uris.key(uri),
      doc = useDocuments.getState().find(key);
    if (!doc?.leaseId) return;
    const views = new Set([view]);
    const file: File = {
      uri,
      languageId,
      key,
      id: doc.id,
      leaseId: doc.leaseId,
      version: 0,
      doc: Text.of(doc.text.split("\n")),
      views,
      getView: (preferred) => (preferred && views.has(preferred) ? preferred : (views.values().next().value ?? null)),
    };
    this.files.push(file);
    this.client.didOpen(file);
  }
  closeFile(uri: string, view: EditorView) {
    const file = this.getFile(uri) as File | null;
    file?.views.delete(view);
  }
  syncFiles() {
    if (!this.live) return [];
    const updates = [];
    for (const file of [...this.files]) {
      const doc = useDocuments.getState().docs[file.id];
      if (!doc || doc.key.root !== file.key.root || doc.key.path !== file.key.path) {
        this.client.didClose(file.uri);
        this.files = this.files.filter((f) => f !== file);
        continue;
      }
      if (doc.leaseId !== file.leaseId) {
        this.invalidate();
        return [];
      }
      const next = Text.of(doc.text.split("\n"));
      for (const view of file.views) LSPPlugin.get(view)?.clear();
      if (file.doc.eq(next)) continue;
      const prevDoc = file.doc;
      const changes = ChangeSet.of({ from: 0, to: prevDoc.length, insert: next }, prevDoc.length);
      file.doc = next;
      file.version++;
      updates.push({ file, prevDoc, changes });
    }
    return updates;
  }
  // Commands use the all-target preflight in applyWorkspaceEdit instead of partial per-file updates.
  updateFile(_uri: string, _update: TransactionSpec): void {
    throw new Error("파일 변경은 전체 검증이 필요합니다.");
  }
  dispose() {
    this.live = false;
    clearTimeout(this.timer);
    this.unsubscribe();
    for (const file of this.files) {
      for (const view of file.views) {
        if (LSPPlugin.get(view)?.client === this.client) {
          try {
            view.dispatch(setDiagnostics(view.state, []));
          } catch {
            /* A constructing/destroying view is no longer a sink. */
          }
        }
      }
      if (!activeEditors.has(file.id)) {
        const state = editorStates.get(file.id);
        if (state) editorStates.set(file.id, state.update(setDiagnostics(state, [])).state);
      }
    }
    this.files = [];
  }
}
