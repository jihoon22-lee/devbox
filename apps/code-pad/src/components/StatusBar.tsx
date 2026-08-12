import type { Doc } from "../types";
import { encodingLabel, languageForPath, languageLabel } from "../editor/extensions";

interface StatusBarProps {
  doc: Doc | null;
  zoom: number;
}

export default function StatusBar({ doc, zoom }: StatusBarProps) {
  if (!doc) {
    return <footer className="status-bar">문서를 열면 편집 상태가 표시됩니다</footer>;
  }
  return (
    <footer className="status-bar">
      <span>{languageLabel(languageForPath(doc.path))}</span>
      <span>{encodingLabel(doc.encoding.encodingKind)}{doc.encoding.bom ? " BOM" : ""}</span>
      <span>{doc.lineEnding.toUpperCase()}</span>
      <span>{doc.readOnly ? "읽기 전용" : doc.dirty ? "수정됨" : "저장됨"}</span>
      {doc.lossy && <span className="status-warning">인코딩 손실 가능성</span>}
      {doc.durabilityWarning && <span className="status-warning">{doc.durabilityWarning}</span>}
      <span>{zoom}%</span>
      <span className="status-path" title={doc.path}>{doc.path}</span>
    </footer>
  );
}
