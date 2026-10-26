import { getIndentUnit, indentUnit } from "@codemirror/language";
import type { LSPClient } from "@codemirror/lsp-client";
import { type EditorView, keymap, showDialog } from "@codemirror/view";
import { keyId } from "../../documents/offlineJournal";
import { useDocuments } from "../../documents/store";
import { useSource } from "../store";
import type { WorkspaceProof } from "./lspProof";
import { applyWorkspaceEdit, type EditContext } from "./lspWorkspace";

export function commandContext(context: EditContext): EditContext {
  return {
    ...context,
    expected: new Map(Object.values(useDocuments.getState().docs).map((doc) => [keyId(doc.key), doc])),
  };
}
function position(view: EditorView) {
  const at = view.state.selection.main.head,
    line = view.state.doc.lineAt(at);
  return { line: line.number - 1, character: at - line.from };
}
export function lspCommands(client: LSPClient, uri: string, context: EditContext, report: (text: string) => void) {
  const fail = () =>
    report(
      "파일 상태가 바뀌었거나 변경 범위를 안전하게 확인하지 못했습니다. 범위를 줄여 다시 요청하거나 변경을 직접 검토해 주세요.",
    );
  const format = (view: EditorView) => {
    if (!context.current()) return false;
    client.sync();
    const snapshot = commandContext(context);
    void client
      .request("textDocument/formatting", {
        textDocument: { uri },
        options: { tabSize: getIndentUnit(view.state), insertSpaces: !view.state.facet(indentUnit).includes("\t") },
      })
      .then((result) => (result ? applyWorkspaceEdit(snapshot, { changes: { [uri]: result } }) : undefined))
      .catch(fail);
    return true;
  };
  const rename = (view: EditorView) => {
    if (!context.current()) return false;
    const word = view.state.wordAt(view.state.selection.main.head);
    if (!word) return false;
    const { close, result } = showDialog(view, {
      label: "새 이름",
      input: { name: "name", value: view.state.sliceDoc(word.from, word.to) },
      focus: true,
      submitLabel: "이름 바꾸기",
      class: "cm-devbox-rename",
    });
    void result
      .then(async (form) => {
        if (!context.current()) return;
        view.dispatch({ effects: close });
        if (!form) return;
        const newName = (form.elements.namedItem("name") as HTMLInputElement).value;
        if (!newName.trim()) return;
        client.sync();
        const snapshot = commandContext(context);
        const result = await client.request("textDocument/rename", {
          textDocument: { uri },
          position: position(view),
          newName,
        });
        const response = result as { edit?: unknown; proof?: WorkspaceProof } | null;
        if (!response?.proof) throw new Error("변경 출처를 확인하지 못했습니다.");
        await applyWorkspaceEdit({ ...snapshot, proof: response.proof }, response.edit ?? { changes: {} });
      })
      .catch(fail);
    return true;
  };
  const definition = (view: EditorView) => {
    if (!context.current()) return false;
    client.sync();
    const before = useDocuments.getState().find(context.uris.key(uri));
    void client
      .request("textDocument/definition", { textDocument: { uri }, position: position(view) })
      .then(async (value) => {
        if (!context.current() || useDocuments.getState().find(context.uris.key(uri)) !== before) return;
        const first = Array.isArray(value) ? value[0] : value;
        if (!first || typeof first !== "object") return;
        const found = first as {
          uri?: string;
          targetUri?: string;
          range?: { start?: { line: number; character: number } };
          targetSelectionRange?: { start?: { line: number; character: number } };
        };
        const target = found.uri ?? found.targetUri,
          start = (found.range ?? found.targetSelectionRange)?.start;
        if (
          !target ||
          !start ||
          !Number.isSafeInteger(start.line) ||
          !Number.isSafeInteger(start.character) ||
          start.line < 0 ||
          start.character < 0
        )
          throw new Error();
        await useSource.getState().open(context.uris.key(target), {
          pinned: true,
          location: { line: start.line + 1, column: start.character + 1 },
        });
      })
      .catch(fail);
    return true;
  };
  return keymap.of([
    { key: "F12", run: definition, preventDefault: true },
    { key: "Shift-Alt-f", run: format, preventDefault: true },
    { key: "F2", run: rename, preventDefault: true },
  ]);
}
