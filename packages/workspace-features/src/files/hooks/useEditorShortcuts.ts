import { useEffect, type RefObject } from "react";
import { isImeComposing } from "@devbox/a11y";
import { activeDocForState } from "../lib/documentPresentation";
import type { DocId, EditorState } from "../types";

interface EditorShortcutOptions {
  activeRef: RefObject<boolean>;
  hydratedRef: RefObject<boolean>;
  handleSaveRef: RefObject<() => unknown>;
  quickOpenRef: RefObject<() => void>;
  renameApplyBusyRef: RefObject<boolean>;
  stateRef: RefObject<EditorState>;
  replaceCommandsRef: RefObject<Map<DocId, () => boolean>>;
}

export function useEditorShortcuts({
  activeRef,
  hydratedRef,
  handleSaveRef,
  quickOpenRef,
  renameApplyBusyRef,
  stateRef,
  replaceCommandsRef,
}: EditorShortcutOptions) {
  useEffect(() => {
    const handleShortcut = (event: KeyboardEvent) => {
      if (!activeRef.current) return;
      if (isImeComposing(event)) return;
      if (!(event.ctrlKey || event.metaKey)) return;
      const key = event.key.toLowerCase();
      if (key === "s") {
        event.preventDefault();
        handleSaveRef.current();
      } else if (key === "o") {
        event.preventDefault();
        if (hydratedRef.current) document.getElementById("path-input")?.focus();
      } else if (key === "p") {
        if (hydratedRef.current) {
          event.preventDefault();
          quickOpenRef.current();
        }
      } else if (key === "h") {
        if (renameApplyBusyRef.current) return;
        const current = activeDocForState(stateRef.current);
        const command = current ? replaceCommandsRef.current.get(current.id) : undefined;
        if (command) {
          event.preventDefault();
          command();
        }
      }
    };
    window.addEventListener("keydown", handleShortcut);
    return () => window.removeEventListener("keydown", handleShortcut);
  }, [activeRef, hydratedRef, handleSaveRef, quickOpenRef, renameApplyBusyRef, stateRef, replaceCommandsRef]);
}
