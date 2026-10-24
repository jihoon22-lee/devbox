import type { EditorState } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { replaceText } from "./editorExtensions";
import { useDocuments } from "./store";

/** Window/document-session lifetime, independent of section or split-pane mounts. */
export const editorStates = new Map<string, EditorState>();
export const activeEditors = new Map<string, EditorView>();
export const editorSaves = new Map<string, (() => void) | undefined>();
export function cacheEditor(id: string, state: EditorState) {
  if (useDocuments.getState().docs[id]) editorStates.set(id, state);
}
useDocuments.subscribe(({ docs }) => {
  for (const [id, state] of editorStates) {
    const doc = docs[id];
    if (!doc) {
      editorStates.delete(id);
      editorSaves.delete(id);
      continue;
    }
    if (activeEditors.has(id)) continue;
    const spec = replaceText(state, doc.text);
    if (spec) editorStates.set(id, state.update(spec).state);
  }
});
