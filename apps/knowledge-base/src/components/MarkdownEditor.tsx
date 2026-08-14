import { useEffect, useRef } from "react";
import { baseEditorExtensions, markdownEditorExtensions } from "@devbox/editor";
import { defaultKeymap } from "@codemirror/commands";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";

// 공용 CodeMirror 설정은 packages/editor(@devbox/editor)에서 가져온다 (PR 24).
// LSP는 넣지 않는다 (knowledge-base는 노트 앱이다).
interface Props {
  value: string;
  onChange: (text: string) => void;
  onSave: () => void;
}

export default function MarkdownEditor({ value, onChange, onSave }: Props) {
  const mountRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const onSaveRef = useRef(onSave);
  onSaveRef.current = onSave;

  useEffect(() => {
    if (!mountRef.current) return;
    const view = new EditorView({
      state: EditorState.create({
        doc: value,
        extensions: [
          baseEditorExtensions(),
          markdownEditorExtensions(),
          keymap.of([
            ...defaultKeymap,
            { key: "Mod-s", run: () => { onSaveRef.current(); return true; }, preventDefault: true },
          ]),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) onChangeRef.current(update.state.doc.toString());
          }),
        ],
      }),
      parent: mountRef.current,
    });
    viewRef.current = view;
    return () => {
      view.destroy();
      viewRef.current = null;
    };
    // 마운트 수명은 컴포넌트 수명과 같다 (파일 전환은 value 동기화로 처리)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 외부 값 동기화 (파일 전환/저장 취소)
  useEffect(() => {
    const view = viewRef.current;
    if (!view || view.state.doc.toString() === value) return;
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: value } });
  }, [value]);

  return <div ref={mountRef} className="editor codemirror-editor" />;
}
