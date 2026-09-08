import type { LspLocationTarget } from "../types";
import { pathFromFileUri } from "../lspFeatures";

interface Props {
  kind: "definition" | "references";
  locations: LspLocationTarget[];
  rejected: number;
  onOpen: (location: LspLocationTarget) => void;
  onClose: () => void;
}

export default function LspNavigationPanel({ kind, locations, rejected, onOpen, onClose }: Props) {
  return (
    <section className="lsp-navigation-panel" aria-label={kind === "definition" ? "정의 결과" : "참조 결과"}>
      <div className="lsp-navigation-header">
        <strong>{kind === "definition" ? "정의" : "참조"} ({locations.length})</strong>
        {rejected > 0 && <span>작업 폴더 밖 결과 {rejected}개 제외</span>}
        <button type="button" className="toolbar-button" onClick={onClose}>닫기</button>
      </div>
      {locations.length === 0 ? (
        <p className="lsp-empty">결과가 없습니다.</p>
      ) : (
        <ul className="lsp-navigation-list">
          {locations.map((location, index) => {
            const path = pathFromFileUri(location.uri);
            const displayPosition = location.selectionRange?.start ?? location.range.start;
            return (
              <li key={`${location.uri}:${displayPosition.line}:${displayPosition.character}:${index}`}>
                <button type="button" className="lsp-location-button" onClick={() => onOpen(location)} disabled={!path}>
                  <span>{path ?? location.uri}</span>
                  <small>{displayPosition.line + 1}:{displayPosition.character + 1}</small>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
