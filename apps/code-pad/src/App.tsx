import { useEffect, useMemo, useReducer, useRef, useState } from "react";
import { openFile, saveFile } from "./api";
import DocHost from "./components/DocHost";
import StatusBar from "./components/StatusBar";
import ViewPane from "./components/ViewPane";
import {
  createInitialEditorState,
  docIdForPath,
  editorReducer,
  type EditorAction,
} from "./store/documentStore";
import type { Doc, DocId, EditorState, OpenedFile, SavedFile } from "./types";

function docFromOpenedFile(file: OpenedFile): Doc {
  return {
    id: docIdForPath(file.path),
    path: file.path,
    text: file.text,
    encoding: file.encoding,
    lineEnding: file.lineEnding,
    readOnly: file.readOnly,
    size: file.size,
    mtimeNanos: file.mtimeNanos,
    contentHash: file.contentHash ?? "",
    lossy: file.lossy,
    durabilityWarning: file.durabilityWarning ?? null,
    dirty: false,
    revision: 0,
    cursor: 0,
    bookmarks: [],
  };
}

function activeDocForState(state: EditorState): Doc | null {
  const activeId = state.activeDocByView[state.activeView];
  return state.docs.find((doc) => doc.id === activeId) ?? null;
}

function matchesSnapshot(doc: Doc | undefined, revision: number, text: string): boolean {
  return doc !== undefined && doc.revision === revision && doc.text === text;
}

interface SaveOutcome {
  saved: SavedFile;
  matchedSnapshot: boolean;
}

export default function App() {
  const [state, dispatch] = useReducer(editorReducer, undefined, createInitialEditorState);
  const [pathInput, setPathInput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [zoom, setZoom] = useState(100);
  const [pendingCloseDocId, setPendingCloseDocId] = useState<DocId | null>(null);
  const stateRef = useRef(state);
  stateRef.current = state;
  const busyRef = useRef(false);
  const operationTokenRef = useRef(0);
  const replaceCommandsRef = useRef(new Map<DocId, () => boolean>());
  const activeDoc = useMemo(() => activeDocForState(state), [state]);
  const pendingCloseDoc = pendingCloseDocId
    ? state.docs.find((doc) => doc.id === pendingCloseDocId) ?? null
    : null;

  async function runFileOperation<T>(operation: () => Promise<T>): Promise<T | undefined> {
    // React state updates are asynchronous. The ref is the immediate lock that
    // closes the click + keyboard race before a rerender can disable buttons.
    if (busyRef.current) return undefined;
    const token = ++operationTokenRef.current;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      return await operation();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      return undefined;
    } finally {
      if (operationTokenRef.current === token) {
        busyRef.current = false;
        setBusy(false);
      }
    }
  }

  const handleOpen = () => {
    const path = pathInput.trim();
    if (!path) {
      setError("열 파일 경로를 입력하세요.");
      return;
    }
    void runFileOperation(async () => {
      const opened = await openFile(path);
      dispatch({ type: "addDoc", doc: docFromOpenedFile(opened) });
      setPathInput("");
    });
  };

  const saveDocument = async (docId: DocId): Promise<SaveOutcome | undefined> => {
    const doc = stateRef.current.docs.find((item) => item.id === docId);
    if (!doc) {
      setError("저장할 문서가 없습니다.");
      return undefined;
    }
    if (doc.readOnly) {
      setError("읽기 전용 문서는 저장할 수 없습니다.");
      return undefined;
    }
    if (doc.lossy) {
      setError("손실 디코딩된 문서는 저장할 수 없습니다. 원본 인코딩을 선택해 다시 여세요.");
      return undefined;
    }
    const submittedRevision = doc.revision;
    const submittedText = doc.text;
    const saved = await runFileOperation(() =>
      saveFile(
        doc.path,
        submittedText,
        doc.encoding,
        doc.lineEnding,
        doc.mtimeNanos,
        doc.size,
        doc.contentHash ?? "",
        doc.lossy,
      ),
    );
    if (!saved) return undefined;
    dispatch({
      type: "saveDoc",
      docId,
      submittedRevision,
      submittedText,
      mtimeNanos: saved.mtimeNanos,
      size: saved.size,
      contentHash: saved.contentHash,
      durabilityWarning: saved.durabilityWarning,
    });
    const latestDoc = stateRef.current.docs.find((item) => item.id === docId);
    return {
      saved,
      matchedSnapshot: matchesSnapshot(latestDoc, submittedRevision, submittedText),
    };
  };

  const handleSave = () => {
    if (!activeDoc) {
      setError("저장할 문서가 없습니다.");
      return;
    }
    void saveDocument(activeDoc.id);
  };

  const handleCloseRequest = (docId: DocId) => {
    const doc = stateRef.current.docs.find((item) => item.id === docId);
    if (!doc) return;
    if (doc.dirty) {
      setPendingCloseDocId(docId);
      return;
    }
    dispatch({ type: "removeDoc", docId });
  };

  const handleDiscardClose = () => {
    if (!pendingCloseDocId) return;
    dispatch({ type: "removeDoc", docId: pendingCloseDocId });
    setPendingCloseDocId(null);
  };

  const handleSaveAndClose = () => {
    if (!pendingCloseDocId) return;
    const docId = pendingCloseDocId;
    void (async () => {
      const outcome = await saveDocument(docId);
      if (!outcome) return;
      if (outcome.matchedSnapshot) {
        dispatch({ type: "removeDoc", docId });
        setPendingCloseDocId(null);
      } else {
        setError("저장 중 새 편집이 발생해 탭을 닫지 않았습니다.");
      }
    })();
  };

  const handleReplaceCommandReady = (docId: DocId, command: (() => boolean) | null) => {
    if (command) replaceCommandsRef.current.set(docId, command);
    else replaceCommandsRef.current.delete(docId);
  };

  const handleSaveRef = useRef(handleSave);
  handleSaveRef.current = handleSave;

  useEffect(() => {
    const handleShortcut = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey)) return;
      const key = event.key.toLowerCase();
      if (key === "s") {
        event.preventDefault();
        handleSaveRef.current();
      } else if (key === "o") {
        event.preventDefault();
        document.getElementById("path-input")?.focus();
      } else if (key === "h") {
        const current = activeDocForState(stateRef.current);
        const command = current ? replaceCommandsRef.current.get(current.id) : undefined;
        if (command) {
          event.preventDefault();
          command();
        }
      }
    };
    window.addEventListener("keydown", handleShortcut);
    return () => window.removeEventListener("keydown", handleShortcut);
  }, []);

  useEffect(() => {
    if (pendingCloseDocId && !state.docs.some((doc) => doc.id === pendingCloseDocId)) {
      setPendingCloseDocId(null);
    }
  }, [pendingCloseDocId, state.docs]);

  const dispatchAction = (action: EditorAction) => {
    // Keep the snapshot ref current even before React commits the reducer
    // update. This closes the small promise-resolution race in Save & Close.
    if (action.type === "setDocText") {
      const current = stateRef.current;
      const currentDoc = current.docs.find((doc) => doc.id === action.docId);
      if (currentDoc && currentDoc.text !== action.text) {
        stateRef.current = {
          ...current,
          docs: current.docs.map((doc) =>
            doc.id === action.docId
              ? { ...doc, text: action.text, dirty: true, revision: doc.revision + 1 }
              : doc,
          ),
        };
      }
    }
    dispatch(action);
  };
  const activeView = state.activeView;

  return (
    <main className="app-shell">
      <header className="app-header">
        <div className="app-heading">
          <p className="eyebrow">WORKBENCH</p>
          <h1>Code Pad</h1>
        </div>
        <div className="file-toolbar">
          <input
            id="path-input"
            className="path-input"
            value={pathInput}
            placeholder="파일 경로를 입력하세요 (dev shell)"
            aria-label="열 파일 경로"
            onChange={(event) => setPathInput(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") handleOpen();
            }}
          />
          <button type="button" className="toolbar-button" onClick={handleOpen} disabled={busy}>
            파일 열기
          </button>
          <button type="button" className="toolbar-button" onClick={handleSave} disabled={busy || !activeDoc}>
            저장
          </button>
        </div>
      </header>

      <div className="editor-toolbar" role="toolbar" aria-label="편집기 도구">
        <button
          type="button"
          className={`toolbar-button ${state.split ? "selected" : ""}`}
          onClick={() => dispatchAction({ type: "toggleSplit" })}
          aria-pressed={state.split}
        >
          {state.split ? "분할 닫기" : "뷰 분할"}
        </button>
        <span className="toolbar-divider" />
        <button
          type="button"
          className="toolbar-button"
          aria-label="편집기 글꼴 크기 축소"
          title="편집기 글꼴 크기 축소"
          onClick={() => setZoom((value) => Math.max(75, value - 10))}
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
          title="편집기 글꼴 크기 확대"
          onClick={() => setZoom((value) => Math.min(200, value + 10))}
        >
          A+
        </button>
        <span className="toolbar-hint">Ctrl/⌘+F 찾기 · Ctrl/⌘+H 바꾸기 · Ctrl/⌘+S 저장</span>
      </div>

      {error && (
        <div className="error-banner" role="alert">
          <span>{error}</span>
          <button type="button" aria-label="오류 닫기" onClick={() => setError(null)}>
            ×
          </button>
        </div>
      )}

      <section className={`editor-area ${state.split ? "split" : "single"}`}>
        <ViewPane
          view={0}
          docs={state.docs}
          docIds={state.views[0]}
          activeDocId={state.activeDocByView[0]}
          onActivateDoc={(view, docId) => dispatchAction({ type: "activateDoc", view, docId })}
          onCloseDoc={handleCloseRequest}
          onMoveDoc={(docId, toView) => dispatchAction({ type: "moveDoc", docId, toView })}
        />
        <ViewPane
          view={1}
          docs={state.docs}
          docIds={state.views[1]}
          activeDocId={state.activeDocByView[1]}
          hidden={!state.split}
          onActivateDoc={(view, docId) => dispatchAction({ type: "activateDoc", view, docId })}
          onCloseDoc={handleCloseRequest}
          onMoveDoc={(docId, toView) => dispatchAction({ type: "moveDoc", docId, toView })}
        />
        <DocHost
          docs={state.docs}
          views={state.views}
          activeDocByView={state.activeDocByView}
          split={state.split}
          fontSize={13 * zoom / 100}
          onChange={(docId, text) => dispatchAction({ type: "setDocText", docId, text })}
          onFocusDoc={(view, docId) => dispatchAction({ type: "activateDoc", view, docId })}
          onReplaceCommandReady={handleReplaceCommandReady}
        />
      </section>

      {pendingCloseDoc && (
        <div className="modal-backdrop" role="presentation">
          <div
            className="confirm-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="close-dialog-title"
            aria-describedby="close-dialog-description"
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                setPendingCloseDocId(null);
                return;
              }
              if (event.key === "Tab") {
                const focusable = Array.from(
                  event.currentTarget.querySelectorAll<HTMLElement>(
                    "button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled])",
                  ),
                );
                const first = focusable[0];
                const last = focusable[focusable.length - 1];
                if (!first || !last) return;
                if (event.shiftKey && document.activeElement === first) {
                  event.preventDefault();
                  last.focus();
                } else if (!event.shiftKey && document.activeElement === last) {
                  event.preventDefault();
                  first.focus();
                }
              }
            }}
          >
            <h2 id="close-dialog-title">저장되지 않은 변경 사항</h2>
            <p id="close-dialog-description">
              {pendingCloseDoc.path}에 저장되지 않은 변경 사항이 있습니다. 어떻게 하시겠습니까?
            </p>
            <div className="confirm-dialog-actions">
              <button type="button" className="toolbar-button" autoFocus onClick={() => setPendingCloseDocId(null)}>
                취소
              </button>
              <button type="button" className="toolbar-button" onClick={handleDiscardClose}>
                변경 내용 버리고 닫기
              </button>
              <button type="button" className="toolbar-button selected" onClick={handleSaveAndClose} disabled={busy}>
                저장 후 닫기
              </button>
            </div>
          </div>
        </div>
      )}

      <StatusBar doc={activeDoc} zoom={zoom} />
      <p className="scope-note">
        경로 입력으로 파일을 열 수 있습니다. 파일 대화상자·미리보기·감시·북마크·LSP는 다음 단계에서 연결됩니다.
        {activeView === 1 && " 현재 2번 뷰가 활성화되어 있습니다."}
      </p>
    </main>
  );
}
