export interface NoteEditorExit { unsaved: () => boolean; saveBeforeQuit: () => Promise<boolean>; settleBeforeQuit: () => Promise<void> }
let current: NoteEditorExit | undefined;
export function registerNoteEditor(editor: NoteEditorExit) {
  current = editor;
  return () => { if (current === editor) current = undefined; };
}
export function hasUnsavedNote() { return current?.unsaved() ?? false; }
export async function saveNoteBeforeQuit() { return await current?.saveBeforeQuit() ?? true; }
export async function settleNoteBeforeQuit() { await current?.settleBeforeQuit(); }

export { NoteSessionProvider, useNoteSession } from "./session";
