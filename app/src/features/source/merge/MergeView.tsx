import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { MergeView as CodeMirrorMerge } from "@codemirror/merge";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap, lineNumbers } from "@codemirror/view";
import { useEffect, useRef } from "react";
import type { ConflictPreview } from "../../../rpc/gen/rpc";
import s from "./merge.module.css";

function pane(label: string, editable: boolean, onChange?: (text: string) => void) {
  return [
    lineNumbers(),
    EditorView.lineWrapping,
    EditorView.contentAttributes.of({ "aria-label": label }),
    EditorState.readOnly.of(!editable),
    EditorView.editable.of(editable),
    ...(editable ? [history(), keymap.of([...defaultKeymap, ...historyKeymap])] : []),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) onChange?.(update.state.doc.toString());
    }),
  ];
}
function setText(view: EditorView, text: string) {
  if (view.state.doc.toString() !== text)
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
}
export function MergeView({
  preview,
  resolution,
  onChange,
}: {
  preview: ConflictPreview;
  resolution: string;
  onChange: (text: string) => void;
}) {
  const upper = useRef<HTMLDivElement>(null),
    lower = useRef<HTMLDivElement>(null);
  const views = useRef<{ top: CodeMirrorMerge; bottom: CodeMirrorMerge } | null>(null);
  const syncing = useRef(false);
  const change = useRef(onChange);
  change.current = onChange;
  useEffect(() => {
    if (!upper.current || !lower.current) return;
    const top = new CodeMirrorMerge({
      parent: upper.current,
      a: { extensions: pane("공통 기준", false) },
      b: { extensions: pane("내 것", false) },
      diffConfig: { scanLimit: 500, timeout: 100 },
    });
    const bottom = new CodeMirrorMerge({
      parent: lower.current,
      a: { extensions: pane("그쪽", false) },
      b: {
        extensions: pane("해결안", true, (text) => {
          if (!syncing.current) change.current(text);
        }),
      },
      diffConfig: { scanLimit: 500, timeout: 100 },
    });
    views.current = { top, bottom };
    return () => {
      views.current = null;
      top.destroy();
      bottom.destroy();
    };
  }, []);
  useEffect(() => {
    const current = views.current;
    if (!current) return;
    syncing.current = true;
    setText(current.top.a, preview.base ?? "");
    setText(current.top.b, preview.ours ?? "");
    setText(current.bottom.a, preview.theirs ?? "");
    setText(current.bottom.b, resolution);
    syncing.current = false;
  }, [preview, resolution]);
  return (
    <div className={s.editors}>
      <div className={s.labels}>
        <strong>
          공통 기준 · {preview.labels.base}
          {preview.base === null ? " · 없음" : ""}
        </strong>
        <strong>
          내 것 · {preview.labels.ours}
          {preview.ours === null ? " · 없음" : ""}
        </strong>
      </div>
      <div ref={upper} className={s.pair} />
      <div className={s.labels}>
        <strong>
          그쪽 · {preview.labels.theirs}
          {preview.theirs === null ? " · 없음" : ""}
        </strong>
        <strong>결과 · 해결안</strong>
      </div>
      <div ref={lower} className={s.pair} />
    </div>
  );
}
