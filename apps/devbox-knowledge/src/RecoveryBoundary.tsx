import { Component, useState, useSyncExternalStore, type ReactNode } from "react";
import { useNoteSession } from "@devbox/knowledge-features/notes-lifecycle";

function RecoveryDraft() {
  const document = useNoteSession();
  const [message, setMessage] = useState("");
  const note = useSyncExternalStore(document?.subscribe ?? (() => () => {}), document?.snapshot ?? (() => null));
  if (!document || !note?.path) return null;
  return <section aria-label="노트 복구">
    <p>{note.path} — 현재 편집 내용은 메모리에 유지됩니다.</p>
    <textarea aria-label="보존된 노트 내용" readOnly value={note.content}/>
    {note.conflict && <p role="alert">외부 변경이 있습니다. 노트 화면에서 충돌을 검토해 주세요.</p>}
    <button type="button" disabled={note.saving || !!note.conflict} onClick={() => {
      void document.saveBeforeQuit().then(saved => setMessage(saved ? "노트를 저장했습니다." : "저장하지 못했습니다. 편집 내용은 유지됩니다."));
    }}>보존된 노트 저장</button>
    {message && <p role="status">{message}</p>}
  </section>;
}
export default class RecoveryBoundary extends Component<{ children: ReactNode; name: string; recoverNote?: boolean }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  render() {
    if (!this.state.failed) return this.props.children;
    return <section aria-label={`${this.props.name} 복구`}>
      <p role="alert">{this.props.name} 화면을 표시하지 못했습니다.</p>
      {this.props.recoverNote && <RecoveryDraft/>}
      <button type="button" onClick={() => this.setState({ failed: false })}>화면 다시 시도</button>
    </section>;
  }
}
