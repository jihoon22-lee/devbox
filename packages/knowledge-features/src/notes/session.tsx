import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { NoteDocument } from "./noteDocument";
import { registerNoteEditor } from "./lifecycle";

const Session = createContext<NoteDocument | null>(null);
export function NoteSessionProvider({ children }: { children: ReactNode }) {
  const [document] = useState(
    () =>
      new NoteDocument(
        async (path) => (await import("./api")).readFile(path),
        async (path, content, revision) => (await import("./api")).writeFile(path, content, revision),
      ),
  );
  useEffect(() => registerNoteEditor(document), [document]);
  return <Session.Provider value={document}>{children}</Session.Provider>;
}
export function useNoteSession() {
  return useContext(Session);
}
