import { useEffect, useMemo, useRef, useState } from "react";
import {
  filterQuickOpenFiles,
  flattenQuickOpenTree,
  groupQuickOpenMatches,
  splitQuickOpenPath,
  type QuickOpenDirectory,
  type QuickOpenMatch,
} from "../lib/quickOpen";
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
  const returnFocusRef = useRef<HTMLElement | null>(null);
  const matches = useMemo(() => filterQuickOpenFiles(files, query), [files, query]);
  const visibleMatches = useMemo(
    () => matches.slice(0, QUICK_OPEN_VISIBLE_LIMIT),
    [matches],
  );
  const tree = useMemo(() => groupQuickOpenMatches(visibleMatches), [visibleMatches]);
  const displayedMatches = useMemo(() => flattenQuickOpenTree(tree), [tree]);
  const resultsVisible = !loading && Boolean(workspaceFolder) && displayedMatches.length > 0;
  const selectedOptionId =
    resultsVisible && displayedMatches[selected] ? optionIdForIndex(selected) : undefined;

  useEffect(() => {
    returnFocusRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    inputRef.current?.focus();
    return () => {
      if (returnFocusRef.current?.isConnected) returnFocusRef.current.focus();
    };
  }, []);

  useEffect(() => {
    setSelected((current) => Math.min(current, Math.max(displayedMatches.length - 1, 0)));
  }, [displayedMatches.length]);

  useEffect(() => {
    if (!selectedOptionId) return;
    document.getElementById(selectedOptionId)?.scrollIntoView?.({ block: "nearest" });
  }, [selectedOptionId]);

  const moveSelection = (delta: number) => {
    if (displayedMatches.length === 0) return;
    setSelected((current) => {
      const next = current + delta;
      if (next < 0) return displayedMatches.length - 1;
      if (next >= displayedMatches.length) return 0;
      return next;
    });
  };

  const chooseSelected = () => {
    const match = displayedMatches[selected];
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
        <header className="quick-open-header">
          <div>
            <h2>빠른 파일 열기</h2>
            <p className="quick-open-workspace" title={workspaceFolder ?? undefined}>
              {workspaceFolder ?? "작업 폴더 없음"}
            </p>
          </div>
          <span className="quick-open-count" aria-live="polite">
            {loading ? "읽는 중…" : `${matches.length}개`}
          </span>
        </header>
        <input
          ref={inputRef}
          className="quick-open-input"
          value={query}
          placeholder="파일 이름 또는 경로 검색"
          role="combobox"
          aria-label="파일 검색"
          aria-autocomplete="list"
          aria-expanded={resultsVisible}
          aria-controls={resultsVisible ? "quick-open-results" : undefined}
          aria-activedescendant={selectedOptionId}
          onChange={(event) => {
            setQuery(event.currentTarget.value);
            setSelected(0);
          }}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              onClose();
            } else if (event.key === "Tab") {
              // Search results are controlled through the single search input;
              // keeping Tab here prevents focus from escaping behind the modal
              // and preserves a keyboard-only flow.
              event.preventDefault();
              inputRef.current?.focus();
            } else if (event.key === "ArrowDown") {
              event.preventDefault();
              moveSelection(1);
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              moveSelection(-1);
            } else if (event.key === "Home") {
              event.preventDefault();
              setSelected(0);
            } else if (event.key === "End") {
              event.preventDefault();
              setSelected(Math.max(displayedMatches.length - 1, 0));
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
        {resultsVisible && (
          <div id="quick-open-results" className="quick-open-results" role="listbox" aria-label="검색 결과">
            {tree.files.length > 0 && (
              <ul
                className="quick-open-list quick-open-root-list"
                role="group"
                aria-label="작업 폴더 파일"
              >
                {tree.files.map((match) =>
                  renderMatch(match, displayedMatches, selected, setSelected, onOpen),
                )}
              </ul>
            )}
            {tree.directories.map((directory) =>
              renderDirectory(directory, displayedMatches, selected, setSelected, onOpen),
            )}
          </div>
        )}
        <p className="quick-open-help">↑↓ 선택 · Home/End 이동 · Enter 열기 · Esc 닫기</p>
      </section>
    </div>
  );
}

function renderDirectory(
  directory: QuickOpenDirectory,
  matches: QuickOpenMatch[],
  selected: number,
  setSelected: (index: number) => void,
  onOpen: (path: string) => void,
) {
  return (
    <section
      key={directory.path}
      className="quick-open-directory"
      role="group"
      aria-label={`디렉터리 ${directory.path}`}
    >
      <h3 className="quick-open-directory-heading" title={`${directory.path}/`}>
        <span className="quick-open-directory-marker" aria-hidden="true">▾</span>
        <span className="quick-open-directory-name">{directory.name}/</span>
        {directory.path !== directory.name && (
          <span className="quick-open-directory-path">{directory.path}/</span>
        )}
      </h3>
      {directory.files.length > 0 && (
        <ul className="quick-open-list" role="group" aria-label={`${directory.path} 파일`}>
          {directory.files.map((match) => renderMatch(match, matches, selected, setSelected, onOpen))}
        </ul>
      )}
      {directory.directories.map((child) =>
        renderDirectory(child, matches, selected, setSelected, onOpen),
      )}
    </section>
  );
}

function renderMatch(
  match: QuickOpenMatch,
  matches: QuickOpenMatch[],
  selected: number,
  setSelected: (index: number) => void,
  onOpen: (path: string) => void,
) {
  const index = matches.findIndex(({ file }) => file.path === match.file.path);
  const parts = splitQuickOpenPath(match.file.relativePath);
  const isSelected = index === selected;
  return (
    <li key={match.file.path} className="quick-open-list-item">
      <button
        id={optionIdForIndex(index)}
        type="button"
        role="option"
        aria-selected={isSelected}
        tabIndex={-1}
        className={`quick-open-item ${isSelected ? "selected" : ""}`}
        aria-label={match.file.relativePath}
        title={match.file.relativePath}
        onMouseDown={(event) => event.preventDefault()}
        onMouseEnter={() => setSelected(index)}
        onClick={() => onOpen(match.file.path)}
      >
        <span className="quick-open-item-copy">
          <span className="quick-open-item-name">{parts.name}</span>
          {parts.directory && <span className="quick-open-item-path">{parts.directory}/</span>}
        </span>
        <small className="quick-open-item-size">{formatBytes(match.file.size)}</small>
      </button>
    </li>
  );
}

function optionIdForIndex(index: number): string {
  return `quick-open-option-${index}`;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
