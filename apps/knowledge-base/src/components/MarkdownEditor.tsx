import { useEffect, useRef } from "react";
import { history, historyKeymap, defaultKeymap } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { defaultHighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { search, searchKeymap } from "@codemirror/search";
import { EditorState } from "@codemirror/state";
import { EditorView, highlightActiveLine, keymap, lineNumbers } from "@codemirror/view";

// PR 24에서 packages/editor로 추출할 공용 CodeMirror 설정. 여기서는 복사해 쓴다.
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
          lineNumbers(),
          highlightActiveLine(),
          history(),
          markdown({ base: markdownLanguage }),
          syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
          keymap.of([
            ...defaultKeymap,
            ...historyKeymap,
            ...searchKeymap,
            { key: "Mod-s", run: () => { onSaveRef.current(); return true; }, preventDefault: true },
          ]),
          search({ top: true }),
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
