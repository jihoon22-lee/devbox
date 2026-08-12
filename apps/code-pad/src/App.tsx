import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useReducer, useRef, useState } from "react";
import {
  canonicalizeWorkspace,
  listWorkspaceFiles,
  loadSession,
  openFile,
  renderPreview,
  saveFile,
  saveSession,
  validateEncoding,
  unwatchFile,
  watchFile,
} from "./api";
import DocHost from "./components/DocHost";
import PreviewPane from "./components/PreviewPane";
import QuickOpen from "./components/QuickOpen";
import StatusBar from "./components/StatusBar";
import ViewPane from "./components/ViewPane";
import type { BookmarkCommands } from "./editor/bookmarks";
import { normalizeBookmarkLines } from "./editor/bookmarks";
import {
  createInitialEditorState,
  docIdForPath,
  editorReducer,
  stateToSession,
  type EditorAction,
} from "./store/documentStore";
import type {
  Doc,
  DocId,
  EditorState,
  FileChangedEvent,
  Encoding,
  LineEnding,
  OpenedFile,
  PreviewResponse,
  SavedFile,
  SessionState,
  WorkspaceFile,
} from "./types";

function docFromOpenedFile(file: OpenedFile, metadata?: SessionState["docs"][number]): Doc {
  return {
    id: metadata?.id ?? docIdForPath(file.path),
    path: file.path,
    text: file.text,
    encoding: file.encoding,
    lineEnding: file.lineEnding,
    readOnly: file.readOnly,
    size: file.size,
    mtimeNanos: file.mtimeNanos,
    contentHash: file.contentHash,
    lossy: file.lossy,
    durabilityWarning: file.durabilityWarning ?? null,
    dirty: false,
    revision: 0,
    cursor: Math.min(metadata?.cursor ?? 0, file.text.length),
    bookmarks: normalizeBookmarkLines(file.text, metadata?.bookmarks ?? []),
  };
}

function activeDocForState(state: EditorState): Doc | null {
  const activeId = state.activeDocByView[state.activeView];
  return state.docs.find((doc) => doc.id === activeId) ?? null;
}

function isPreviewable(path: string): boolean {
  const normalized = path.split("\\").join("/").toLowerCase();
  return normalized.endsWith(".md") || normalized.endsWith(".markdown") || normalized.endsWith(".mmd");
}

function snapshotMatches(
  doc: Doc | undefined,
  snapshot: Pick<
    Doc,
    "revision" | "text" | "mtimeNanos" | "size" | "contentHash" | "dirty" | "encoding" | "lineEnding"
  >,
): boolean {
  return (
    doc !== undefined &&
    doc.dirty === snapshot.dirty &&
    doc.revision === snapshot.revision &&
    doc.text === snapshot.text &&
    doc.mtimeNanos === snapshot.mtimeNanos &&
    doc.size === snapshot.size &&
    doc.contentHash === snapshot.contentHash &&
    doc.lineEnding === snapshot.lineEnding &&
    doc.encoding.encodingKind === snapshot.encoding.encodingKind &&
    doc.encoding.bom === snapshot.encoding.bom
  );
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
  const [hydrated, setHydrated] = useState(false);
  const [quickOpen, setQuickOpen] = useState(false);
  const [workspaceFiles, setWorkspaceFiles] = useState<WorkspaceFile[]>([]);
  const [workspaceListingRoot, setWorkspaceListingRoot] = useState<string | null>(null);
  const [workspaceTruncated, setWorkspaceTruncated] = useState(false);
  const [workspaceLoading, setWorkspaceLoading] = useState(false);
  const [sessionPersistenceAllowed, setSessionPersistenceAllowed] = useState(true);
  const [previewOpen, setPreviewOpen] = useState(false);
  const [preview, setPreview] = useState<PreviewResponse | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [externalChanges, setExternalChanges] = useState<string[]>([]);
  const [pendingCloseDocId, setPendingCloseDocId] = useState<DocId | null>(null);
  const [pendingEncodingReopen, setPendingEncodingReopen] = useState<{
    docId: DocId;
    encoding: Encoding;
  } | null>(null);

  const stateRef = useRef(state);
  stateRef.current = state;
  const hydratedRef = useRef(hydrated);
  hydratedRef.current = hydrated;
  const persistenceAllowedRef = useRef(sessionPersistenceAllowed);
  persistenceAllowedRef.current = sessionPersistenceAllowed;
  const busyRef = useRef(false);
  const operationTokenRef = useRef(0);
  const replaceCommandsRef = useRef(new Map<DocId, () => boolean>());
  const bookmarkCommandsRef = useRef(new Map<DocId, BookmarkCommands>());
  const sessionSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingSessionRef = useRef<SessionState | null>(null);
  const sessionSaveInFlightRef = useRef<Promise<void> | null>(null);
  const watchOperationRef = useRef(new Map<string, Promise<void>>());
  const externalChangeVersionRef = useRef(new Map<string, number>());

  const externalChange = externalChanges[0] ?? null;
  const enqueueExternalChange = (path: string) => {
    externalChangeVersionRef.current.set(
      path,
      (externalChangeVersionRef.current.get(path) ?? 0) + 1,
    );
    setExternalChanges((current) => current.includes(path) ? current : [...current, path]);
  };
  const removeExternalChange = (path: string, expectedVersion?: number) => {
    if (
      expectedVersion !== undefined
      && externalChangeVersionRef.current.get(path) !== expectedVersion
    ) {
      return;
    }
    externalChangeVersionRef.current.delete(path);
    setExternalChanges((current) => current.filter((item) => item !== path));
  };

  const activeDoc = useMemo(() => activeDocForState(state), [state]);
  const pendingCloseDoc = pendingCloseDocId
    ? state.docs.find((doc) => doc.id === pendingCloseDocId) ?? null
    : null;
  const canPreview = Boolean(activeDoc && state.workspaceFolder && isPreviewable(activeDoc.path));

  async function runFileOperation<T>(operation: () => Promise<T>): Promise<T | undefined> {
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

  const dispatchAction = (action: EditorAction) => {
    // Keep the ref in lockstep before React commits the reducer update. This
    // closes promise-resolution races for save/reload and makes the session
    // scheduler always see the latest logical state.
    stateRef.current = editorReducer(stateRef.current, action);
    dispatch(action);
    if (
      hydratedRef.current &&
      !persistenceAllowedRef.current &&
      action.type !== "restoreSession"
    ) {
      persistenceAllowedRef.current = true;
      setSessionPersistenceAllowed(true);
    }
  };

  const enqueueWatchOperation = (path: string, operation: () => Promise<void>) => {
    const previous = watchOperationRef.current.get(path) ?? Promise.resolve();
    const next = previous
      .catch(() => undefined)
      .then(operation)
      .finally(() => {
        if (watchOperationRef.current.get(path) === next) {
          watchOperationRef.current.delete(path);
        }
      });
    watchOperationRef.current.set(path, next);
    return next;
  };

  const registerWatch = async (path: string) => {
    await enqueueWatchOperation(path, async () => {
      try {
        await watchFile(path);
      } catch {
        // File saves still use the native snapshot conflict check if watching
        // is unavailable for this one path.
      }
    });
  };

  const unregisterWatch = (path: string) => {
    void enqueueWatchOperation(path, async () => {
      try {
        await unwatchFile(path);
      } catch {
        // Closing a document remains successful even if its watcher was
        // already unavailable; no native resources are held by the UI.
      }
    });
  };

  const openPath = async (path: string, metadata?: SessionState["docs"][number]) => {
    const opened = await openFile(path, null);
    // The document registry is global across both views. Reopening an
    // already-open path only activates its existing entry; it must not add a
    // second native watch reference that a single close could not release.
    if (!stateRef.current.docs.some((doc) => doc.path === opened.path)) {
      await registerWatch(opened.path);
    }
    dispatchAction({ type: "addDoc", doc: docFromOpenedFile(opened, metadata) });
    return opened;
  };

  const handleOpen = () => {
    if (!hydrated) return;
    const path = pathInput.trim();
    if (!path) {
      setError("열 파일 경로를 입력하세요.");
      return;
    }
    void runFileOperation(async () => {
      await openPath(path);
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
        doc.contentHash,
        doc.lossy,
      ),
    );
    if (!saved) return undefined;
    dispatchAction({
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
      matchedSnapshot:
        latestDoc !== undefined &&
        latestDoc.revision === submittedRevision &&
        latestDoc.text === submittedText,
    };
  };

  const handleSave = () => {
    if (!hydrated) return;
    if (!activeDoc) {
      setError("저장할 문서가 없습니다.");
      return;
    }
    void saveDocument(activeDoc.id);
  };

  const removeDocument = (docId: DocId) => {
    const doc = stateRef.current.docs.find((item) => item.id === docId);
    dispatchAction({ type: "removeDoc", docId });
    if (doc) unregisterWatch(doc.path);
    if (doc) removeExternalChange(doc.path);
    if (activeDoc?.id === docId) setPreview(null);
  };

  const handleCloseRequest = (docId: DocId) => {
    if (!hydrated) return;
    const doc = stateRef.current.docs.find((item) => item.id === docId);
    if (!doc) return;
    if (doc.dirty) {
      setPendingCloseDocId(docId);
      return;
    }
    removeDocument(docId);
  };

  const handleDiscardClose = () => {
    if (!pendingCloseDocId) return;
    removeDocument(pendingCloseDocId);
    setPendingCloseDocId(null);
  };

  const handleSaveAndClose = () => {
    if (!pendingCloseDocId) return;
    const docId = pendingCloseDocId;
    void (async () => {
      const outcome = await saveDocument(docId);
      if (!outcome) return;
      if (outcome.matchedSnapshot) {
        removeDocument(docId);
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

  const handleBookmarkCommandsReady = (docId: DocId, commands: BookmarkCommands | null) => {
    if (commands) bookmarkCommandsRef.current.set(docId, commands);
    else bookmarkCommandsRef.current.delete(docId);
  };

  const invokeBookmarkCommand = (kind: keyof BookmarkCommands) => {
    if (!activeDoc) return;
    bookmarkCommandsRef.current.get(activeDoc.id)?.[kind]();
  };

  const handleLineEndingChange = (docId: DocId, lineEnding: LineEnding) => {
    const doc = stateRef.current.docs.find((item) => item.id === docId);
    if (doc?.readOnly) {
      setError("읽기 전용 문서는 저장 형식을 바꿀 수 없습니다.");
      return;
    }
    dispatchAction({ type: "setLineEnding", docId, lineEnding });
  };

  const handleEncodingConversion = (docId: DocId, encoding: Encoding) => {
    const doc = stateRef.current.docs.find((item) => item.id === docId);
    if (!doc) return;
    if (doc.readOnly) {
      setError("읽기 전용 문서는 저장 형식을 바꿀 수 없습니다.");
      return;
    }
    if (doc.lossy) {
      setError("손실 디코딩된 문서는 먼저 명시적 인코딩으로 다시 열어야 합니다.");
      return;
    }
    if (
      doc.encoding.encodingKind === encoding.encodingKind
      && doc.encoding.bom === encoding.bom
    ) {
      return;
    }
    void runFileOperation(async () => {
      await validateEncoding(doc.text, encoding);
      const latest = stateRef.current.docs.find((item) => item.id === docId);
      if (!latest || !snapshotMatches(latest, doc)) {
        throw new Error("인코딩 변환 중 문서가 변경되었습니다. 다시 시도하세요.");
      }
      dispatchAction({ type: "setEncoding", docId, encoding });
    });
  };

  const reopenWithEncoding = async (docId: DocId, encoding: Encoding): Promise<boolean> => {
    const before = stateRef.current.docs.find((doc) => doc.id === docId);
    if (!before) return false;
    const expectedChangeVersion = externalChangeVersionRef.current.get(before.path);
    const opened = await openFile(before.path, encoding);
    const latest = stateRef.current.docs.find((doc) => doc.id === docId);
    // Explicit reopen is intentionally transactional. A response arriving
    // after another edit must not discard that edit or its metadata.
    if (!latest || !snapshotMatches(latest, before)) {
      throw new Error("인코딩을 다시 여는 동안 문서가 변경되었습니다. 다시 시도하세요.");
    }
    dispatchAction({
      type: "replaceDoc",
      doc: {
        ...docFromOpenedFile(opened),
        id: latest.id,
        cursor: latest.cursor,
        bookmarks: latest.bookmarks.slice(),
      },
    });
    removeExternalChange(before.path, expectedChangeVersion);
    return true;
  };

  const requestEncodingReopen = (docId: DocId, encoding: Encoding) => {
    const doc = stateRef.current.docs.find((item) => item.id === docId);
    if (!doc) return;
    if (doc.dirty) {
      setPendingEncodingReopen({ docId, encoding });
      return;
    }
    void runFileOperation(() => reopenWithEncoding(docId, encoding));
  };

  const confirmEncodingReopen = () => {
    if (!pendingEncodingReopen) return;
    const request = pendingEncodingReopen;
    void runFileOperation(() => reopenWithEncoding(request.docId, request.encoding)).then((opened) => {
      if (opened !== undefined) setPendingEncodingReopen(null);
    });
  };

  const handleOpenFromQuickOpen = (path: string) => {
    if (!hydrated) return;
    setQuickOpen(false);
    void runFileOperation(async () => {
      await openPath(path);
    });
  };

  const loadWorkspaceSnapshot = async (root: string) => {
    setWorkspaceLoading(true);
    try {
      const listing = await listWorkspaceFiles(root);
      setWorkspaceFiles(listing.files);
      setWorkspaceTruncated(listing.truncated);
      setWorkspaceListingRoot(root);
    } finally {
      setWorkspaceLoading(false);
    }
  };

  const handleSetWorkspace = () => {
    if (!hydrated) return;
    const path = pathInput.trim();
    if (!path) {
      setError("작업 폴더 경로를 입력하세요.");
      return;
    }
    void runFileOperation(async () => {
      const root = await canonicalizeWorkspace(path);
      const listing = await listWorkspaceFiles(root);
      dispatchAction({ type: "setWorkspace", workspaceFolder: root });
      setWorkspaceFiles(listing.files);
      setWorkspaceTruncated(listing.truncated);
      setWorkspaceListingRoot(root);
      setPathInput("");
    });
  };

  const handleQuickOpen = () => {
    if (!hydrated) return;
    setQuickOpen(true);
    if (state.workspaceFolder && workspaceListingRoot !== state.workspaceFolder) {
      void loadWorkspaceSnapshot(state.workspaceFolder).catch((cause) => {
        setError(cause instanceof Error ? cause.message : String(cause));
      });
    }
  };

  const reloadExternallyChanged = (path: string) => {
    const before = stateRef.current.docs.find((doc) => doc.path === path);
    if (!before) {
      removeExternalChange(path);
      return;
    }
    const expected = { ...before };
    const expectedChangeVersion = externalChangeVersionRef.current.get(path);
    void runFileOperation(async () => {
      const opened = await openFile(path, null);
      const current = stateRef.current.docs.find((doc) => doc.path === path);
      // An explicit reload is allowed to discard the dirty buffer, but never
      // clobbers an edit made while the asynchronous open was in flight.
      if (!current || !snapshotMatches(current, expected)) {
        enqueueExternalChange(path);
        return;
      }
      dispatchAction({
        type: "replaceDoc",
        doc: {
          ...docFromOpenedFile(opened),
          id: current.id,
          cursor: current.cursor,
          bookmarks: current.bookmarks.slice(),
        },
      });
      removeExternalChange(path, expectedChangeVersion);
    });
  };

  const handleSaveRef = useRef(handleSave);
  handleSaveRef.current = handleSave;

  // Restore only metadata from session.json. Every buffer is read fresh from
  // disk; missing files are skipped individually. The UI remains gated until
  // all restore reads and watcher registrations have settled.
  useEffect(() => {
    let cancelled = false;
    const hydrationWatchPaths = new Set<string>();
    void loadSession()
      .then(async (loaded) => {
        if (cancelled) return;
        persistenceAllowedRef.current = loaded.persistAllowed;
        setSessionPersistenceAllowed(loaded.persistAllowed);
        const restored = await Promise.all(
          loaded.session.docs.map(async (metadata) => {
          try {
            const opened = await openFile(metadata.path, null);
            if (cancelled) return null;
            if (hydrationWatchPaths.has(opened.path)) return null;
            hydrationWatchPaths.add(opened.path);
            await registerWatch(opened.path);
            if (cancelled) return null;
            return docFromOpenedFile(opened, metadata);
          } catch {
            return null;
          }
          }),
        );
        if (cancelled) return;
        dispatchAction({
          type: "restoreSession",
          session: loaded.session,
          docs: restored.filter((doc): doc is Doc => doc !== null),
        });
        hydratedRef.current = true;
        setHydrated(true);
      })
      .catch((cause) => {
        if (cancelled) return;
        setError(cause instanceof Error ? cause.message : String(cause));
        // A failed read may be a permission or filesystem error rather than
        // an empty session. Preserve the on-disk evidence until the user
        // performs a meaningful action, just as for corrupt JSON.
        persistenceAllowedRef.current = false;
        setSessionPersistenceAllowed(false);
        hydratedRef.current = true;
        setHydrated(true);
      });
    return () => {
      cancelled = true;
      for (const path of hydrationWatchPaths) unregisterWatch(path);
      hydrationWatchPaths.clear();
    };
    // Session restoration is one app-lifetime operation.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Native watcher events are authoritative only for disk metadata. A clean
  // document reloads automatically; a dirty document gets an explicit choice.
  useEffect(() => {
    if (!hydrated) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const handleEvent = (payload: FileChangedEvent) => {
      const current = stateRef.current.docs.find((doc) => doc.path === payload.path);
      if (!current) return;
      if (
        current.mtimeNanos === payload.mtimeNanos &&
        current.size === payload.size &&
        current.contentHash === payload.contentHash
      ) {
        return;
      }
      if (current.dirty) {
        enqueueExternalChange(payload.path);
        return;
      }
      const expected = { ...current };
      void openFile(payload.path, null)
        .then((opened) => {
          const latest = stateRef.current.docs.find((doc) => doc.path === payload.path);
          // Recheck every in-memory condition immediately before applying the
          // response. The user may have typed while open_file was pending.
          if (!latest || !snapshotMatches(latest, expected)) {
            enqueueExternalChange(payload.path);
            return;
          }
          dispatchAction({
            type: "replaceDoc",
            doc: {
              ...docFromOpenedFile(opened),
              id: latest.id,
              cursor: latest.cursor,
              bookmarks: latest.bookmarks.slice(),
            },
          });
        })
        .catch(() => enqueueExternalChange(payload.path));
    };
    // In browser/Vitest the Tauri bridge is absent; treat that as a disabled
    // watcher rather than creating an unhandled rejection during mount.
    void Promise.resolve()
      .then(() => listen<FileChangedEvent>("file-changed", (event) => handleEvent(event.payload)))
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [hydrated]);

  // Preview requests are tied to the document revision and discarded if a
  // newer edit arrives before the native render returns.
  useEffect(() => {
    if (!previewOpen || !activeDoc || !state.workspaceFolder || !isPreviewable(activeDoc.path)) {
      setPreview(null);
      setPreviewError(null);
      return;
    }
    const expected = { ...activeDoc };
    let cancelled = false;
    setPreviewError(null);
    void renderPreview(activeDoc.path, activeDoc.text, state.workspaceFolder)
      .then((response) => {
        const latest = stateRef.current.docs.find((doc) => doc.id === expected.id);
        if (cancelled || !latest || !snapshotMatches(latest, expected)) return;
        setPreview(response);
      })
      .catch((cause) => {
        const latest = stateRef.current.docs.find((doc) => doc.id === expected.id);
        if (cancelled || !latest || !snapshotMatches(latest, expected)) return;
        setPreviewError(cause instanceof Error ? cause.message : String(cause));
      });
    return () => {
      cancelled = true;
    };
  }, [previewOpen, activeDoc?.id, activeDoc?.revision, activeDoc?.text, state.workspaceFolder]);

  // A single debounced, serialized writer means a slow save cannot let an old
  // request finish after a newer request and overwrite the newest session.
  useEffect(() => {
    if (!hydrated || !sessionPersistenceAllowed) return;
    if (sessionSaveTimerRef.current) clearTimeout(sessionSaveTimerRef.current);
    pendingSessionRef.current = stateToSession(state);
    const startDrain = () => {
      if (sessionSaveInFlightRef.current) return;
      const drain = async () => {
        while (pendingSessionRef.current) {
          const next = pendingSessionRef.current;
          pendingSessionRef.current = null;
          try {
            await saveSession(next);
          } catch (cause) {
            setError(cause instanceof Error ? cause.message : String(cause));
          }
        }
      };
      const inFlight = drain();
      sessionSaveInFlightRef.current = inFlight;
      void inFlight.finally(() => {
        if (sessionSaveInFlightRef.current === inFlight) sessionSaveInFlightRef.current = null;
        if (pendingSessionRef.current && !sessionSaveTimerRef.current) {
          // Keep the same quiet debounce for edits that arrived while the
          // previous native write was in flight.
          sessionSaveTimerRef.current = setTimeout(() => {
            sessionSaveTimerRef.current = null;
            startDrain();
          }, 1_000);
        }
      });
    };
    sessionSaveTimerRef.current = setTimeout(() => {
      sessionSaveTimerRef.current = null;
      startDrain();
    }, 1_000);
    return () => {
      if (sessionSaveTimerRef.current) {
        clearTimeout(sessionSaveTimerRef.current);
        sessionSaveTimerRef.current = null;
      }
    };
  }, [hydrated, sessionPersistenceAllowed, state]);

  useEffect(() => {
    const handleShortcut = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey)) return;
      const key = event.key.toLowerCase();
      if (key === "s") {
        event.preventDefault();
        handleSaveRef.current();
      } else if (key === "o") {
        event.preventDefault();
        if (hydratedRef.current) document.getElementById("path-input")?.focus();
      } else if (key === "p") {
        if (hydratedRef.current) {
          event.preventDefault();
          setQuickOpen(true);
        }
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
            placeholder="파일 또는 작업 폴더 경로"
            aria-label="열 파일 경로"
            disabled={!hydrated}
            onChange={(event) => setPathInput(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") handleOpen();
            }}
          />
          <button type="button" className="toolbar-button" onClick={handleOpen} disabled={busy || !hydrated}>
            파일 열기
          </button>
          <button type="button" className="toolbar-button" onClick={handleSetWorkspace} disabled={busy || !hydrated}>
            작업 폴더
          </button>
          <button type="button" className="toolbar-button" onClick={handleQuickOpen} disabled={!hydrated || !state.workspaceFolder}>
            빠른 열기
          </button>
          <button type="button" className="toolbar-button" onClick={handleSave} disabled={busy || !hydrated || !activeDoc}>
            저장
          </button>
        </div>
      </header>

      {!hydrated && <p className="hydration-banner" role="status">세션을 복원하는 중...</p>}

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
        <button type="button" className="toolbar-button" aria-label="편집기 글꼴 크기 축소" onClick={() => setZoom((value) => Math.max(75, value - 10))} disabled={!hydrated}>
          A−
        </button>
        <output className="zoom-label" aria-label={`편집기 확대 ${zoom}%`} aria-live="polite">{zoom}%</output>
        <button type="button" className="toolbar-button" aria-label="편집기 글꼴 크기 확대" onClick={() => setZoom((value) => Math.min(200, value + 10))} disabled={!hydrated}>
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
          <button type="button" className={`toolbar-button ${previewOpen ? "selected" : ""}`} onClick={() => setPreviewOpen((open) => !open)} aria-pressed={previewOpen}>
            프리뷰
          </button>
        )}
        <span className="toolbar-hint">Ctrl/⌘+P 빠른 열기 · Ctrl/⌘+H 바꾸기 · Ctrl/⌘+S 저장</span>
      </div>

      {error && (
        <div className="error-banner" role="alert">
          <span>{error}</span>
          <button type="button" aria-label="오류 닫기" onClick={() => setError(null)}>×</button>
        </div>
      )}

      {externalChange && (
        <div className="external-change-banner" role="alert">
          <span>
            디스크에서 파일이 변경되었습니다: <code>{externalChange}</code> · 현재 편집 내용을 어떻게 할까요?
            {externalChanges.length > 1 && ` (대기 중 ${externalChanges.length - 1}개)`}
          </span>
          <button type="button" onClick={() => reloadExternallyChanged(externalChange)}>다시 읽기</button>
          <button type="button" onClick={() => removeExternalChange(externalChange)}>현재 내용 유지</button>
        </div>
      )}

      <section className={`content-area ${previewOpen ? "with-preview" : ""}`}>
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
            fontSize={(13 * zoom) / 100}
            onChange={(docId, text) => dispatchAction({ type: "setDocText", docId, text })}
            onCursorChange={(docId, cursor) => dispatchAction({ type: "setCursor", docId, cursor })}
            onBookmarksChange={(docId, bookmarks) => dispatchAction({ type: "setBookmarks", docId, bookmarks })}
            onFocusDoc={(view, docId) => dispatchAction({ type: "activateDoc", view, docId })}
            onReplaceCommandReady={handleReplaceCommandReady}
            onBookmarkCommandsReady={handleBookmarkCommandsReady}
          />
        </section>
        {previewOpen && activeDoc && (
          <PreviewPane docPath={activeDoc.path} response={preview} error={previewError} />
        )}
      </section>

      <StatusBar
        doc={activeDoc}
        zoom={zoom}
        onEncodingReopen={requestEncodingReopen}
        onEncodingConvert={handleEncodingConversion}
        onLineEndingChange={handleLineEndingChange}
      />
      <p className="scope-note">
        작업 폴더: {state.workspaceFolder ?? "지정되지 않음"} · {workspaceFiles.length}개 파일
        {workspaceTruncated && " · 일부 목록만 표시"}
      </p>

      {pendingCloseDoc && (
        <div className="modal-backdrop" role="presentation">
          <div
            className="confirm-dialog"
            role="dialog"
            aria-modal="true"
            aria-label="저장되지 않은 변경 사항"
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                setPendingCloseDocId(null);
              }
            }}
          >
            <h2>저장되지 않은 변경 사항</h2>
            <p>{pendingCloseDoc.path}에 저장되지 않은 변경 사항이 있습니다. 어떻게 하시겠습니까?</p>
            <div className="confirm-dialog-actions">
              <button type="button" className="toolbar-button" autoFocus onClick={() => setPendingCloseDocId(null)}>취소</button>
              <button type="button" className="toolbar-button" onClick={handleDiscardClose}>변경 내용 버리고 닫기</button>
              <button type="button" className="toolbar-button selected" onClick={handleSaveAndClose} disabled={busy}>저장 후 닫기</button>
            </div>
          </div>
        </div>
      )}

      {pendingEncodingReopen && (
        <div className="modal-backdrop" role="presentation">
          <div
            className="confirm-dialog"
            role="dialog"
            aria-modal="true"
            aria-label="인코딩 다시 열기"
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                setPendingEncodingReopen(null);
              }
            }}
          >
            <h2>인코딩을 바꿔 다시 열까요?</h2>
            <p>저장되지 않은 변경 사항이 버려집니다. 선택한 인코딩으로 디스크 파일을 엄격하게 다시 읽습니다.</p>
            <div className="confirm-dialog-actions">
              <button type="button" className="toolbar-button" autoFocus onClick={() => setPendingEncodingReopen(null)}>
                취소
              </button>
              <button type="button" className="toolbar-button selected" onClick={confirmEncodingReopen} disabled={busy}>
                변경 내용 버리고 다시 열기
              </button>
            </div>
          </div>
        </div>
      )}

      {quickOpen && (
        <QuickOpen
          files={workspaceFiles}
          truncated={workspaceTruncated}
          loading={workspaceLoading}
          workspaceFolder={state.workspaceFolder}
          onOpen={handleOpenFromQuickOpen}
          onClose={() => setQuickOpen(false)}
        />
      )}
    </main>
  );
}
