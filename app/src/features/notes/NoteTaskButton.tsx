import { EditorView } from "@codemirror/view";
import type { RefObject } from "react";
import { Button } from "../../ui";
import { useAgentsUi } from "../agents/ui";
import { noteTaskPrompt } from "./noteTask";
export function NoteTaskButton({ host, text }: { host: RefObject<HTMLElement | null>; text: string | undefined }) {
  return (
    <Button
      disabled={text === undefined}
      onClick={() => {
        if (text === undefined) return;
        const element = host.current?.querySelector<HTMLElement>(".cm-editor");
        const view = element ? EditorView.findFromDOM(element) : null;
        const selected =
          view?.state.selection.ranges
            .filter((range) => !range.empty)
            .map((range) => view.state.sliceDoc(range.from, range.to))
            .join("\n") ?? "";
        useAgentsUi.getState().setCreating(true, noteTaskPrompt(text, selected));
      }}
    >
      에이전트 작업 만들기
    </Button>
  );
}
