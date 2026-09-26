import type * as React from "react";

interface Props {
  state: import("../types").EditorState;
  dispatchAction: (action: import("../store/documentStore").EditorAction) => import("../types").EditorState;
  hydrated: boolean;
  setZoom: React.Dispatch<React.SetStateAction<number>>;
  zoom: number;
  invokeBookmarkCommand: (kind: keyof import("../editor/bookmarks").BookmarkCommands) => void;
  activeDoc: import("../types").Doc | null;
  canPreview: boolean;
  previewOpen: boolean;
  setPreviewOpen: React.Dispatch<React.SetStateAction<boolean>>;
  lspPanelOpen: boolean;
  setLspPanelOpen: React.Dispatch<React.SetStateAction<boolean>>;
  problemsOpen: boolean;
  setProblemsOpen: React.Dispatch<React.SetStateAction<boolean>>;
  goNav: (dir: "back" | "forward") => void;
  navBack: import("../App").NavEntry[];
  navForward: import("../App").NavEntry[];
  handleLspNavigation: (kind: "definition" | "references", docId?: string | null, cursor?: number | undefined) => void;
  lspCapability: (capability: keyof import("../types").LspCapabilities) => boolean;
  lspBusy: boolean;
  handleLspRename: () => void;
  handleLspFormatting: () => void;
  canManuallyRestartLsp: boolean;
  handleLspRestart: () => void;
}

export function EditorToolbar({
  state,
  dispatchAction,
  hydrated,
  setZoom,
  zoom,
  invokeBookmarkCommand,
  activeDoc,
  canPreview,
  previewOpen,
  setPreviewOpen,
  lspPanelOpen,
  setLspPanelOpen,
  problemsOpen,
  setProblemsOpen,
  goNav,
  navBack,
  navForward,
  handleLspNavigation,
  lspCapability,
  lspBusy,
  handleLspRename,
  handleLspFormatting,
  canManuallyRestartLsp,
  handleLspRestart,
}: Props) {
  return (
    <div className="editor-toolbar" role="toolbar" aria-label="편집기 도구">
      <button
        type="button"
        className={`toolbar-button ${state.split ? "selected" : ""}`}
        onClick={() => dispatchAction({ type: "toggleSplit" })}
        aria-pressed={state.split}
        disabled={!hydrated}
      >
        {state.split ? "분할 닫기" : "뷰 분할"}
      </button>
      <span className="toolbar-divider" />
      <button
        type="button"
        className="toolbar-button"
        aria-label="편집기 글꼴 크기 축소"
        onClick={() => setZoom((value) => Math.max(75, value - 10))}
        disabled={!hydrated}
      >
        A−
      </button>
      <output className="zoom-label" aria-label={`편집기 확대 ${zoom}%`} aria-live="polite">
        {zoom}%
      </output>
      <button
        type="button"
        className="toolbar-button"
        aria-label="편집기 글꼴 크기 확대"
        onClick={() => setZoom((value) => Math.min(200, value + 10))}
        disabled={!hydrated}
      >
        A+
      </button>
      <span className="toolbar-divider" />
      <button
        type="button"
        className="toolbar-button"
        onClick={() => invokeBookmarkCommand("toggle")}
        disabled={!hydrated || !activeDoc}
      >
        북마크
      </button>
      <button
        type="button"
        className="toolbar-button"
        onClick={() => invokeBookmarkCommand("previous")}
        disabled={!hydrated || !activeDoc}
        aria-label="이전 북마크"
      >
        ◀
      </button>
      <button
        type="button"
        className="toolbar-button"
        onClick={() => invokeBookmarkCommand("next")}
        disabled={!hydrated || !activeDoc}
        aria-label="다음 북마크"
      >
        ▶
      </button>
      {canPreview && (
        <button
          type="button"
          className={`toolbar-button ${previewOpen ? "selected" : ""}`}
          onClick={() => setPreviewOpen((open) => !open)}
          aria-pressed={previewOpen}
        >
          프리뷰
        </button>
      )}
      <button
        type="button"
        className={`toolbar-button ${lspPanelOpen ? "selected" : ""}`}
        onClick={() => setLspPanelOpen(true)}
        disabled={!hydrated}
      >
        언어 서버
      </button>
      <button
        type="button"
        className={`toolbar-button ${problemsOpen ? "selected" : ""}`}
        onClick={() => setProblemsOpen((prev) => !prev)}
        disabled={!hydrated}
      >
        문제
      </button>
      <button
        type="button"
        className="toolbar-button"
        onClick={() => goNav("back")}
        disabled={navBack.length === 0}
        title="뒤로"
      >
        ←
      </button>
      <button
        type="button"
        className="toolbar-button"
        onClick={() => goNav("forward")}
        disabled={navForward.length === 0}
        title="앞으로"
      >
        →
      </button>
      <button
        type="button"
        className="toolbar-button"
        onClick={() => handleLspNavigation("definition")}
        disabled={!hydrated || !lspCapability("definition") || lspBusy}
      >
        정의
      </button>
      <button
        type="button"
        className="toolbar-button"
        onClick={() => handleLspNavigation("references")}
        disabled={!hydrated || !lspCapability("references") || lspBusy}
      >
        참조
      </button>
      <button
        type="button"
        className="toolbar-button"
        onClick={handleLspRename}
        disabled={!hydrated || !lspCapability("rename") || lspBusy}
      >
        이름 변경
      </button>
      <button
        type="button"
        className="toolbar-button"
        onClick={handleLspFormatting}
        disabled={!hydrated || !lspCapability("formatting") || lspBusy}
      >
        포맷
      </button>
      {canManuallyRestartLsp && (
        <button type="button" className="toolbar-button" onClick={handleLspRestart} disabled={lspBusy}>
          LSP 재시작
        </button>
      )}
      <span className="toolbar-hint">Ctrl/⌘+P 빠른 열기 · Ctrl/⌘+H 바꾸기 · Ctrl/⌘+S 저장</span>
    </div>
  );
}
