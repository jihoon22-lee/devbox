import { useEffect, useMemo, useRef, useState } from "react";
import { filterQuickOpenFiles } from "../lib/quickOpen";
import type { WorkspaceFile } from "../types";

export const QUICK_OPEN_VISIBLE_LIMIT = 200;

interface QuickOpenProps {
  files: WorkspaceFile[];
  truncated: boolean;
  loading: boolean;
  workspaceFolder: string | null;
  onOpen: (path: string) => void;
  onClose: () => void;
}

export default function QuickOpen({
  files,
  truncated,
  loading,
  workspaceFolder,
  onOpen,
  onClose,
}: QuickOpenProps) {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const matches = useMemo(() => filterQuickOpenFiles(files, query), [files, query]);
  const visibleMatches = matches.slice(0, QUICK_OPEN_VISIBLE_LIMIT);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    setSelected((current) => Math.min(current, Math.max(visibleMatches.length - 1, 0)));
  }, [visibleMatches.length]);

  const chooseSelected = () => {
    const match = visibleMatches[selected];
    if (match) onOpen(match.file.path);
  };

  return (
    <div className="quick-open-backdrop" role="presentation" onMouseDown={onClose}>
      <section
        className="quick-open-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="빠른 파일 열기"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <input
          ref={inputRef}
          className="quick-open-input"
          value={query}
          placeholder="파일 이름 또는 경로 검색"
          aria-label="파일 검색"
          onChange={(event) => {
            setQuery(event.currentTarget.value);
            setSelected(0);
          }}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              onClose();
            } else if (event.key === "ArrowDown") {
              event.preventDefault();
              setSelected((current) => Math.min(current + 1, Math.max(visibleMatches.length - 1, 0)));
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              setSelected((current) => Math.max(current - 1, 0));
            } else if (event.key === "Enter") {
              event.preventDefault();
              chooseSelected();
            }
          }}
        />
        {truncated && (
          <p className="quick-open-banner" role="status">
            폴더가 커서 일부만 색인했습니다
          </p>
        )}
        {!workspaceFolder && !loading && (
          <p className="quick-open-empty">먼저 작업 폴더를 지정하세요.</p>
        )}
        {loading && <p className="quick-open-empty">작업 폴더를 읽는 중...</p>}
        {!loading && workspaceFolder && matches.length === 0 && (
          <p className="quick-open-empty">일치하는 파일이 없습니다.</p>
        )}
        {!loading && visibleMatches.length < matches.length && (
          <p className="quick-open-banner" role="status">
            결과가 많아 상위 {QUICK_OPEN_VISIBLE_LIMIT}개만 표시합니다
          </p>
        )}
        {!loading && matches.length > 0 && (
          <ul className="quick-open-list">
            {visibleMatches.map((match, index) => (
              <li key={match.file.path}>
                <button
                  type="button"
                  className={`quick-open-item ${index === selected ? "selected" : ""}`}
                  onMouseEnter={() => setSelected(index)}
                  onClick={() => onOpen(match.file.path)}
                >
                  <span>{match.file.relativePath}</span>
                  <small>{formatBytes(match.file.size)}</small>
                </button>
              </li>
            ))}
          </ul>
        )}
        <p className="quick-open-help">↑↓ 선택 · Enter 열기 · Esc 닫기</p>
      </section>
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
