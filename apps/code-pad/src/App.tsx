import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useReducer, useRef, useState } from "react";
import type { CompletionSource } from "@codemirror/autocomplete";
import type { HoverTooltipSource } from "@codemirror/view";
import {
  canonicalizeWorkspace,
  deleteFileAction,
  listWorkspaceFiles,
  loadSession,
  loadRecovery,
  openFile,
  renameFileAction,
  revealFileAction,
  renderPreview,
  saveFile,
  saveSession,
  takePendingOpen,
  validateEncoding,
  unwatchFile,
  watchFile,
} from "./api";
import DocHost from "./components/DocHost";
import ChangeSetPreview, { type ChangeSetItem } from "./components/ChangeSetPreview";
import PreviewPane from "./components/PreviewPane";
import ProblemsPanel from "./components/ProblemsPanel";
import QuickOpen from "./components/QuickOpen";
import RecoveryDialog from "./components/RecoveryDialog";
import StatusBar from "./components/StatusBar";
import ViewPane from "./components/ViewPane";
import LspControlPanel from "./components/LspControlPanel";
import LspNavigationPanel from "./components/LspNavigationPanel";
import type { TabContextAction } from "./components/TabBar";
import { currentDocumentWordCompletion } from "./editor/extensions";
import { APPLINK_OPEN_EVENT, routeOpenRequest } from "./lib/applink";
import { completionOptions, diagnosticsForCodeMirror, hoverText, offsetForPosition, pathFromFileUri } from "./lspFeatures";
import type { BookmarkCommands } from "./editor/bookmarks";
import { normalizeBookmarkLines } from "./editor/bookmarks";
import { LspDocumentSync } from "./lspDocumentSync";
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
  EditedLspDocument,
  LspRenameApplyResult,
  LspRenamePreview,
  LspDiagnosticsEvent,
  LspStatusEvent,
  OpenRequest,
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

function fileNameForPath(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

function relativeWorkspacePath(path: string, workspaceRoot: string): string | null {
  const normalize = (value: string) => {
    const slashValue = value.replace(/\\/gu, "/");
    const prefix = slashValue.startsWith("//") ? "//" : "";
    const normalized = `${prefix}${slashValue.slice(prefix.length).replace(/\/{2,}/gu, "/")}`;
    if (normalized === "/" || /^[A-Za-z]:\/$/u.test(normalized)) return normalized;
    return normalized.replace(/\/$/u, "");
  };
  const normalizedPath = normalize(path);
  const normalizedRoot = normalize(workspaceRoot);
  const windowsPath = /^[A-Za-z]:\//u.test(normalizedPath)
    || /^[A-Za-z]:\//u.test(normalizedRoot)
    || normalizedPath.startsWith("//")
    || normalizedRoot.startsWith("//");
  const candidate = windowsPath ? normalizedPath.toLowerCase() : normalizedPath;
  const root = windowsPath ? normalizedRoot.toLowerCase() : normalizedRoot;
  if (candidate === root) return "";
  const prefix = root === "/" || /^[a-z]:\/$/u.test(root) ? root : `${root}/`;
  if (!candidate.startsWith(prefix)) return null;
  return normalizedPath.slice(prefix.length);
}

function renameFileStatusLabel(status: LspRenameApplyResult["files"][number]["status"]): string {
  switch (status) {
    case "applied": return "적용됨";
    case "rolledBack": return "되돌림";
    case "failed": return "실패";
    case "notApplied": return "미적용";
    case "conflict": return "충돌";
    case "rollbackFailed": return "되돌리기 실패";
  }
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

export interface NavEntry {
  docId: DocId;
  path: string;
  cursor: number;
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
  const [pendingCloseDocIds, setPendingCloseDocIds] = useState<DocId[]>([]);
  const [pendingEncodingReopen, setPendingEncodingReopen] = useState<{
    docId: DocId;
    encoding: Encoding;
  } | null>(null);
  const [lspPanelOpen, setLspPanelOpen] = useState(false);
  const [problemsOpen, setProblemsOpen] = useState(false);
  const [navBack, setNavBack] = useState<NavEntry[]>([]);
  const [navForward, setNavForward] = useState<NavEntry[]>([]);
  const [recoveryOpen, setRecoveryOpen] = useState(false);
  const [recoveryChecked, setRecoveryChecked] = useState(false);
  const [lspSync] = useState(() => new LspDocumentSync());
  const [lspSyncState, setLspSyncState] = useState(lspSync.getState());
  const [lspDiagnostics, setLspDiagnostics] = useState<Record<DocId, import("@codemirror/lint").Diagnostic[]>>({});
  const [lspNavigation, setLspNavigation] = useState<{
    kind: "definition" | "references";
    locations: import("./types").LspLocationTarget[];
    rejected: number;
  } | null>(null);
  const [lspBusy, setLspBusy] = useState(false);
  const [renamePreview, setRenamePreview] = useState<{
    preview: LspRenamePreview;
    revisions: Map<DocId, number>;
    workspaceRoot: string;
  } | null>(null);
  const [renameResult, setRenameResult] = useState<LspRenameApplyResult | null>(null);
  const [renameApplyBusy, setRenameApplyBusy] = useState(false);
  const renameApplyBusyRef = useRef(false);
  // `cancelRename` can race the pre-apply mirror flush, before the native
  // transaction has registered its cancellation token. Keep an explicit UI
  // intent bit so that a late flush cannot turn a cancelled approval into a
  // disk mutation.
  const renameCancelRequestedRef = useRef(false);
  renameApplyBusyRef.current = renameApplyBusy;

  const discardRenamePreview = () => {
    const planId = renamePreview?.preview.planId;
    if (planId) void lspSync.discardRename(planId);
    setRenamePreview(null);
  };

  const renameApplyGuard = () => {
    if (!renameApplyBusyRef.current) return false;
    setError("이름 변경 적용이 끝난 뒤 파일을 열거나 이동할 수 있습니다.");
    return true;
  };

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
  const workspaceChangeTokenRef = useRef(0);
  const lspFeatureRequestRef = useRef(0);
  const lspBusyRef = useRef(false);
  const quickOpenRef = useRef<() => void>(() => undefined);

  useEffect(() => lspSync.subscribe(setLspSyncState), [lspSync]);

  useEffect(() => lspSync.subscribeDiagnostics((snapshot) => {
    const doc = stateRef.current.docs.find((item) => item.id === snapshot.documentId);
    if (!doc) return;
    if (snapshot.response.stale) {
      setLspDiagnostics((current) => {
        if (!(snapshot.documentId in current)) return current;
        const next = { ...current };
        delete next[snapshot.documentId];
        return next;
      });
      return;
    }
    const encoding = lspSync.statusForDocument(doc.id)?.capabilities.positionEncoding ?? "utf-16";
    setLspDiagnostics((current) => ({
      ...current,
      [doc.id]: diagnosticsForCodeMirror(doc.text, snapshot.response.value, encoding),
    }));
  }), [lspSync]);

  // 비정상 종료 후 미저장 버퍼 복구 확인 (§12.1)
  useEffect(() => {
    let active = true;
    void loadRecovery().then((entries) => {
      if (!active) return;
      setRecoveryChecked(true);
      if (entries.length > 0) {
        setRecoveryOpen(true);
      }
    }).catch(() => undefined);
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    let stopDiagnostics: (() => void) | undefined;
    let stopStatus: (() => void) | undefined;
    void Promise.all([
      listen<LspDiagnosticsEvent>("lsp/diagnostics", (event) => lspSync.acceptDiagnosticsEvent(event.payload)),
      listen<LspStatusEvent>("lsp/status", (event) => lspSync.acceptStatusEvent(event.payload)),
    ]).then(([diagnosticsStop, statusStop]) => {
      if (disposed) {
        diagnosticsStop();
        statusStop();
      } else {
        stopDiagnostics = diagnosticsStop;
        stopStatus = statusStop;
      }
    }).catch(() => undefined);
    return () => {
      disposed = true;      stopDiagnostics?.();
      stopStatus?.();
    };
  }, [lspSync]);

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
  const pendingCloseDocId = pendingCloseDocIds[0] ?? null;
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

  const dispatchAction = (action: EditorAction): EditorState => {
    // Keep the ref in lockstep before React commits the reducer update. This
    // closes promise-resolution races for save/reload and makes the session
    // scheduler always see the latest logical state.
    const nextState = editorReducer(stateRef.current, action);
    stateRef.current = nextState;
    dispatch(action);
    if (
      hydratedRef.current &&
      !persistenceAllowedRef.current &&
      action.type !== "restoreSession"
    ) {
      persistenceAllowedRef.current = true;
      setSessionPersistenceAllowed(true);
    }
    return nextState;
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

  const unregisterWatch = (path: string) =>
    enqueueWatchOperation(path, async () => {
      try {
        await unwatchFile(path);
      } catch {
        // Closing a document remains successful even if its watcher was
        // already unavailable; no native resources are held by the UI.
      }
    });

  const openPath = async (path: string, metadata?: SessionState["docs"][number]) => {
    if (renameApplyGuard()) {
      throw new Error("이름 변경 적용이 끝난 뒤 파일을 열거나 이동할 수 있습니다.");
    }
    const opened = await openFile(path, null);
    if (renameApplyGuard()) {
      throw new Error("이름 변경 적용이 끝난 뒤 파일을 열거나 이동할 수 있습니다.");
    }
    // The document registry is global across both views. Reopening an
    // already-open path only activates its existing entry; it must not add a
    // second native watch reference that a single close could not release.
    let watchRegistered = false;
    if (!stateRef.current.docs.some((doc) => doc.path === opened.path)) {
      await registerWatch(opened.path);
      watchRegistered = true;
    }
    if (renameApplyGuard()) {
      if (watchRegistered) await unregisterWatch(opened.path);
      throw new Error("이름 변경 적용이 끝난 뒤 파일을 열거나 이동할 수 있습니다.");
    }
    const alreadyOpen = stateRef.current.docs.some((doc) => doc.path === opened.path);
    const doc = docFromOpenedFile(opened, metadata);
    dispatchAction({ type: "addDoc", doc });
    if (!alreadyOpen) void lspSync.open(doc);
    return stateRef.current.docs.find((item) => item.id === doc.id) ?? doc;
  };

  const handleOpen = () => {
    if (!hydrated || renameApplyGuard()) return;
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
    if (renameApplyBusyRef.current) return undefined;
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
    if (renameApplyBusyRef.current) {
      throw new Error("이름 변경 적용이 시작되어 저장 결과를 반영하지 않았습니다.");
    }
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
    void lspSync.save(docId);
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
    if (!hydrated || renameApplyBusyRef.current) return;
    if (!activeDoc) {
      setError("저장할 문서가 없습니다.");
      return;
    }
    void saveDocument(activeDoc.id);
  };

  const removeDocument = (docId: DocId) => {
    const doc = stateRef.current.docs.find((item) => item.id === docId);
    void lspSync.close(docId);
    dispatchAction({ type: "removeDoc", docId });
    if (doc) void unregisterWatch(doc.path);
    if (doc) removeExternalChange(doc.path);
    if (activeDoc?.id === docId) setPreview(null);
  };

  const requestCloseDocuments = (docIds: readonly DocId[]) => {
    if (!hydrated || renameApplyBusyRef.current) return;
    const requested = [...new Set(docIds)]
      .map((docId) => stateRef.current.docs.find((doc) => doc.id === docId))
      .filter((doc): doc is Doc => doc !== undefined);
    for (const doc of requested) {
      if (!doc.dirty) removeDocument(doc.id);
    }
    const dirtyIds = requested.filter((doc) => doc.dirty).map((doc) => doc.id);
    if (dirtyIds.length > 0) {
      setPendingCloseDocIds((current) => [...current, ...dirtyIds.filter((id) => !current.includes(id))]);
    }
  };

  const handleCloseRequest = (docId: DocId) => requestCloseDocuments([docId]);

  const advanceCloseQueue = (docId: DocId) => {
    setPendingCloseDocIds((current) => current.filter((id) => id !== docId));
  };

  const handleDiscardClose = () => {
    if (!pendingCloseDocId || renameApplyBusyRef.current) return;
    removeDocument(pendingCloseDocId);
    advanceCloseQueue(pendingCloseDocId);
  };

  const handleSaveAndClose = () => {
    if (!pendingCloseDocId || renameApplyBusyRef.current) return;
    const docId = pendingCloseDocId;
    void (async () => {
      const outcome = await saveDocument(docId);
      if (!outcome) return;
      if (outcome.matchedSnapshot) {
        removeDocument(docId);
        advanceCloseQueue(docId);
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

  const lspCapabilityFor = (
    docId: DocId | null | undefined,
    capability: keyof NonNullable<ReturnType<LspDocumentSync["statusForDocument"]>>["capabilities"],
  ) => {
    if (!docId) return false;
    const status = lspSync.statusForDocument(docId);
    if (capability === "rename" && status?.capabilities.syncKind == null) return false;
    return Boolean(status?.capabilities[capability]);
  };

  const lspCapability = (capability: keyof NonNullable<ReturnType<LspDocumentSync["statusForDocument"]>>["capabilities"]) =>
    lspCapabilityFor(activeDoc?.id, capability);

  const runLspOperation = async <T,>(operation: () => Promise<T>): Promise<T | undefined> => {
    if (lspBusyRef.current) return undefined;
    lspBusyRef.current = true;
    setLspBusy(true);
    setError(null);
    try {
      return await operation();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      return undefined;
    } finally {
      lspBusyRef.current = false;
      setLspBusy(false);
    }
  };

  const openLspLocation = async (target: import("./types").LspLocationTarget) => {
    if (renameApplyGuard()) return;
    const path = pathFromFileUri(target.uri);
    if (!path) {
      setError("LSP가 파일이 아닌 탐색 대상을 반환했습니다.");
      return;
    }
    // 내비게이션 히스토리 기록 (정의/참조 이동 전 현재 위치 저장)
    if (activeDoc) {
      setNavBack((prev) => [...prev, { docId: activeDoc.id, path: activeDoc.path, cursor: activeDoc.cursor }]);
      setNavForward([]);
    }
    const doc = await openPath(path);
    const status = lspSync.statusForDocument(doc.id);
    const position = target.selectionRange?.start ?? target.range.start;
    dispatchAction({
      type: "setCursor",
      docId: doc.id,
      cursor: offsetForPosition(doc.text, position, status?.capabilities.positionEncoding ?? "utf-16"),
    });
  };
  const handleProblemsNavigate = async (docId: string, offset: number) => {
    if (renameApplyGuard()) return;
    const doc = stateRef.current.docs.find((d) => d.id === docId);
    if (!doc) return;
    if (activeDoc) {
      setNavBack((prev) => [...prev, { docId: activeDoc.id, path: activeDoc.path, cursor: activeDoc.cursor }]);
      setNavForward([]);
    }
    await openPath(doc.path);
    dispatchAction({ type: "setCursor", docId, cursor: offset });
  };

  const navigateTo = async (entry: NavEntry) => {
    if (renameApplyGuard()) return;
    const doc = stateRef.current.docs.find((d) => d.id === entry.docId);
    if (doc) {
      dispatchAction({ type: "setCursor", docId: entry.docId, cursor: entry.cursor });
      return;
    }
    const opened = await openPath(entry.path);
    dispatchAction({ type: "setCursor", docId: opened.id, cursor: entry.cursor });
  };

  const goNav = (dir: "back" | "forward") => {
    if (dir === "back") {
      const entry = navBack[navBack.length - 1];
      if (!entry) return;
      setNavBack((prev) => prev.slice(0, -1));
      if (activeDoc) setNavForward((prev) => [...prev, { docId: activeDoc.id, path: activeDoc.path, cursor: activeDoc.cursor }]);
      void navigateTo(entry);
    } else {
      const entry = navForward[navForward.length - 1];
      if (!entry) return;
      setNavForward((prev) => prev.slice(0, -1));
      if (activeDoc) setNavBack((prev) => [...prev, { docId: activeDoc.id, path: activeDoc.path, cursor: activeDoc.cursor }]);
      void navigateTo(entry);
    }
  };

  const problemsServerStatus = lspSyncState.lastError
    ? `degraded — ${lspSyncState.lastError}`
    : lspSyncState.configuredLanguages.length === 0
      ? "언어 서버 미구성"
      : lspSyncState.runningLanguages.length === 0
        ? "서버 없음"
        : `${lspSyncState.runningLanguages.join(", ")} 실행 중`;

  const handleLspNavigation = (
    kind: "definition" | "references",
    docId: DocId | null = activeDoc?.id ?? null,
    cursor?: number,
  ) => {
    if (renameApplyBusyRef.current) return;
    const sourceDoc = stateRef.current.docs.find((doc) => doc.id === docId);
    if (!sourceDoc || !lspCapabilityFor(sourceDoc.id, kind)) return;
    const requestId = ++lspFeatureRequestRef.current;
    void runLspOperation(async () => {
      const response = kind === "definition"
        ? await lspSync.requestDefinition(sourceDoc.id, cursor ?? sourceDoc.cursor)
        : await lspSync.requestReferences(sourceDoc.id, cursor ?? sourceDoc.cursor);
      if (requestId !== lspFeatureRequestRef.current || !response || response.stale) return;
      setLspNavigation({ kind, locations: response.value.locations, rejected: response.value.rejected });
    });
  };

  const applyLspEdits = (result: { documents: EditedLspDocument[] }, snapshots: Map<DocId, number>) => {
    const documents = result.documents.map((edited) => ({
      ...edited,
      docId: lspSync.documentIdForUri(edited.uri),
    }));
    if (documents.some((edited) => !edited.docId)) {
      throw new Error("LSP가 열려 있지 않은 문서에 변경을 반환했습니다.");
    }
    const typed = documents as Array<EditedLspDocument & { docId: DocId }>;
    if (typed.some((edited) => stateRef.current.docs.find((doc) => doc.id === edited.docId)?.revision !== snapshots.get(edited.docId))) {
      throw new Error("LSP 변경을 적용하는 동안 문서가 변경되었습니다. 다시 시도하세요.");
    }
    const expectedRevisions = Object.fromEntries(
      typed.map((edited) => [edited.docId, snapshots.get(edited.docId)]),
    ) as Record<DocId, number>;
    const before = stateRef.current;
    const next = dispatchAction({ type: "applyLspDocuments", documents: typed, expectedRevisions });
    if (next === before) {
      throw new Error("LSP 변경을 적용하는 동안 문서가 변경되었습니다. 다시 시도하세요.");
    }
    lspSync.applyDocuments(result.documents);
    setLspDiagnostics((current) => {
      const next = { ...current };
      for (const edited of typed) next[edited.docId] = [];
      return next;
    });
  };

  const applyLspRenameResult = (
    result: LspRenameApplyResult,
    revisions: Map<DocId, number>,
    expectedWorkspaceRoot: string,
  ) => {
    if (stateRef.current.workspaceFolder !== expectedWorkspaceRoot) {
      throw new Error("작업 폴더가 변경되어 이름 변경 결과를 적용하지 않았습니다.");
    }
    if (!result.success) {
      setRenameResult(result);
      setRenamePreview(null);
      return;
    }
    const workspaceRoot = stateRef.current.workspaceFolder;
    if (!workspaceRoot) throw new Error("이름 변경 결과의 작업 폴더가 없습니다.");
    const documents = result.documents.map((edited) => {
      const doc = stateRef.current.docs.find((item) =>
        relativeWorkspacePath(item.path, workspaceRoot) === edited.path,
      );
      const docId = doc?.id;
      if (!docId) throw new Error("이름 변경 결과가 열려 있지 않은 문서를 반환했습니다.");
      if (!doc || doc.revision !== revisions.get(docId)) {
        throw new Error("이름 변경을 적용하는 동안 문서가 변경되었습니다. 다시 시도하세요.");
      }
      const file = result.files.find((item) => item.path === edited.path && item.status === "applied");
      if (!file || file.mtimeNanos === null || file.size === null || file.contentHash === null) {
        throw new Error("이름 변경 결과의 파일 스냅샷이 없습니다.");
      }
      const uri = lspSync.documentUri(docId);
      if (!uri) throw new Error("이름 변경 결과의 LSP 문서가 열려 있지 않습니다.");
      return {
        ...edited,
        docId,
        uri,
        mtimeNanos: file.mtimeNanos,
        size: file.size,
        contentHash: file.contentHash,
      };
    });
    const before = stateRef.current;
    const next = dispatchAction({
      type: "applyLspRename",
      documents,
      expectedRevisions: Object.fromEntries(revisions),
    });
    if (next === before) {
      throw new Error("이름 변경을 적용하는 동안 문서가 변경되었습니다. 다시 시도하세요.");
    }
    lspSync.applyRenameDocuments(result.documents, true);
    for (const file of result.files) {
      if (file.status !== "applied") continue;
      const document = stateRef.current.docs.find((candidate) =>
        relativeWorkspacePath(candidate.path, workspaceRoot) === file.path,
      );
      if (document) removeExternalChange(document.path);
    }
    setRenamePreview(null);
    setRenameResult(result);
  };

  const applyPendingRename = () => {
    const pending = renamePreview;
    if (!pending || renameApplyBusyRef.current) return;
    if (busyRef.current) {
      setError("진행 중인 파일 작업이 끝난 뒤 이름 변경을 적용하세요.");
      return;
    }
    renameCancelRequestedRef.current = false;
    renameApplyBusyRef.current = true;
    setRenameApplyBusy(true);
    void runLspOperation(async () => {
      // A local edit may still be queued behind the editor event. Let the
      // mirror observe it before the native plan is consumed, otherwise the
      // disk commit could race a just-typed change.
      await lspSync.flush();
      if (renameCancelRequestedRef.current) {
        // The native plan is still pending until applyRename is called. Drop
        // it explicitly so cancellation during flush cannot be converted into
        // an approval after the user has already pressed Cancel.
        await lspSync.discardRename(pending.preview.planId);
        return undefined;
      }
      const result = await lspSync.applyRename(pending.preview.planId);
      applyLspRenameResult(result, pending.revisions, pending.workspaceRoot);
      return result;
    }).then((result) => {
      // A native transport failure consumes the opaque plan before returning
      // the error. Do not leave an approval dialog whose handle can never be
      // applied again.
      if (!result) {
        setRenamePreview((current) => (
          current?.preview.planId === pending.preview.planId ? null : current
        ));
      }
    }).finally(() => {
      renameCancelRequestedRef.current = false;
      renameApplyBusyRef.current = false;
      setRenameApplyBusy(false);
    });
  };

  const cancelPendingRename = () => {
    const planId = renamePreview?.preview.planId;
    if (!planId) return;
    if (renameApplyBusyRef.current) {
      renameCancelRequestedRef.current = true;
      void lspSync.cancelRename(planId);
      return;
    }
    discardRenamePreview();
  };

  const handleLspRename = () => {
    if (!activeDoc || !lspCapability("rename") || renamePreview) return;
    const requestedName = window.prompt("새 이름", "");
    if (!requestedName?.trim()) return;
    const requestedDocumentId = activeDoc.id;
    const requestedCursor = activeDoc.cursor;
    const requestedRevision = activeDoc.revision;
    const requestedWorkspaceRoot = stateRef.current.workspaceFolder;
    const requestedWorkspaceChangeToken = workspaceChangeTokenRef.current;
    if (!requestedWorkspaceRoot) {
      setError("이름 변경을 사용할 작업 폴더가 없습니다.");
      return;
    }
    const revisions = new Map(stateRef.current.docs.map((doc) => [doc.id, doc.revision]));
    void runLspOperation(async () => {
      const preview = await lspSync.requestRename(
        requestedDocumentId,
        requestedCursor,
        requestedName.trim(),
      );
      if (!preview || preview.files.length === 0) {
        setError("LSP가 적용할 이름 변경을 반환하지 않았습니다.");
        return;
      }
      const current = stateRef.current.docs.find((doc) => doc.id === requestedDocumentId);
      if (
        stateRef.current.workspaceFolder !== requestedWorkspaceRoot
        || workspaceChangeTokenRef.current !== requestedWorkspaceChangeToken
        || !current
        || current.revision !== requestedRevision
      ) {
        await lspSync.discardRename(preview.planId);
        setError("문서 또는 작업 폴더가 변경되어 이름 변경 미리보기를 폐기했습니다.");
        return;
      }
      setRenameResult(null);
      setRenamePreview({ preview, revisions, workspaceRoot: requestedWorkspaceRoot });
    });
  };

  const handleLspFormatting = () => {
    if (renameApplyBusyRef.current || !activeDoc || !lspCapability("formatting")) return;
    const snapshots = new Map(stateRef.current.docs.map((doc) => [doc.id, doc.revision]));
    void runLspOperation(async () => {
      const result = await lspSync.requestFormatting(activeDoc.id);
      if (result) applyLspEdits(result, snapshots);
    });
  };

  const handleLspRestart = () => {
    if (renameApplyBusyRef.current || !activeDoc) return;
    const status = lspSync.statusForDocument(activeDoc.id);
    if (status) void runLspOperation(() => lspSync.restart(status.languageId));
  };

  const canManuallyRestartLsp = (() => {
    if (!activeDoc) return false;
    const status = lspSync.statusForDocument(activeDoc.id);
    return Boolean(status && (
      status.autoRestartDisabled
      || status.status === "crashed"
      || status.status === "degraded"
      || status.status === "stopped"
    ));
  })();

  const completionSourceFor = (docId: DocId): CompletionSource => async (context) => {
    const response = await lspSync.requestCompletion(docId, context.pos);
    if (!response || response.stale || response.value.items.length === 0) {
      return currentDocumentWordCompletion(context);
    }
    const text = context.state.doc.toString();
    const version = lspSync.documentVersion(docId);
    const encoding = lspSync.statusForDocument(docId)?.capabilities.positionEncoding ?? "utf-16";
    const options = completionOptions(response.value, {
      text,
      encoding,
      isCurrent: () => lspSync.documentVersion(docId) === version && lspSync.documentText(docId) === text,
    });
    if (options.length === 0) return currentDocumentWordCompletion(context);
    const word = context.matchBefore(/[\w$-]*/u);
    return {
      from: word?.from ?? context.pos,
      options,
    };
  };

  const hoverSourceFor = (docId: DocId): HoverTooltipSource => async (_view, pos) => {
    const response = await lspSync.requestHover(docId, pos);
    if (!response || response.stale) return null;
    const text = hoverText(response.value);
    if (!text) return null;
    return {
      pos,
      end: pos,
      above: true,
      create: () => {
        const dom = document.createElement("pre");
        dom.className = "lsp-hover-tooltip";
        dom.textContent = text;
        return { dom };
      },
    };
  };

  const handleLineEndingChange = (docId: DocId, lineEnding: LineEnding) => {
    if (renameApplyBusyRef.current) return;
    const doc = stateRef.current.docs.find((item) => item.id === docId);
    if (doc?.readOnly) {
      setError("읽기 전용 문서는 저장 형식을 바꿀 수 없습니다.");
      return;
    }
    dispatchAction({ type: "setLineEnding", docId, lineEnding });
  };

  const handleEncodingConversion = (docId: DocId, encoding: Encoding) => {
    if (renameApplyBusyRef.current) return;
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
    if (renameApplyBusyRef.current) {
      throw new Error("이름 변경 적용이 끝난 뒤 인코딩을 다시 열 수 있습니다.");
    }
    const before = stateRef.current.docs.find((doc) => doc.id === docId);
    if (!before) return false;
    const expectedChangeVersion = externalChangeVersionRef.current.get(before.path);
    const opened = await openFile(before.path, encoding);
    if (renameApplyBusyRef.current) {
      throw new Error("이름 변경이 시작되어 인코딩 다시 열기 결과를 반영하지 않았습니다.");
    }
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
    const reloaded = stateRef.current.docs.find((doc) => doc.id === docId);
    if (reloaded) void lspSync.reload(reloaded);
    removeExternalChange(before.path, expectedChangeVersion);
    return true;
  };

  const requestEncodingReopen = (docId: DocId, encoding: Encoding) => {
    if (renameApplyBusyRef.current) return;
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
    if (!hydrated || renameApplyGuard()) return;
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

  const refreshCurrentWorkspace = async () => {
    const root = stateRef.current.workspaceFolder;
    if (root) await loadWorkspaceSnapshot(root);
  };

  const renameDocumentFile = (doc: Doc) => {
    const requested = window.prompt("새 파일 이름", fileNameForPath(doc.path));
    const newName = requested?.trim();
    if (!newName || newName === fileNameForPath(doc.path)) return;
    void runFileOperation(async () => {
      const renamed = await renameFileAction({
        path: doc.path,
        mtimeNanos: doc.mtimeNanos,
        size: doc.size,
        contentHash: doc.contentHash,
      }, newName);
      const closeOldLsp = lspSync.close(doc.id);
      const stopOldWatch = unregisterWatch(doc.path);
      removeExternalChange(doc.path);
      lspFeatureRequestRef.current += 1;
      setLspNavigation(null);
      setLspDiagnostics((current) => {
        const next = { ...current };
        delete next[doc.id];
        return next;
      });
      setNavBack((current) => current.map((entry) => entry.docId === doc.id
        ? { ...entry, path: renamed.path }
        : entry));
      setNavForward((current) => current.map((entry) => entry.docId === doc.id
        ? { ...entry, path: renamed.path }
        : entry));
      if (!stateRef.current.docs.some((candidate) => candidate.id === doc.id)) {
        await Promise.all([closeOldLsp, stopOldWatch]);
        await refreshCurrentWorkspace();
        return;
      }
      dispatchAction({
        type: "renameDoc",
        docId: doc.id,
        path: renamed.path,
        mtimeNanos: renamed.mtimeNanos,
        size: renamed.size,
        contentHash: renamed.contentHash,
      });
      await registerWatch(renamed.path);
      await Promise.all([closeOldLsp, stopOldWatch]);
      const latest = stateRef.current.docs.find((candidate) => candidate.id === doc.id);
      if (latest) await lspSync.open(latest);
      await refreshCurrentWorkspace();
    });
  };

  const deleteDocumentFile = (doc: Doc) => {
    const confirmed = window.confirm(
      `${doc.path}\n\n파일을 영구 삭제합니다. 미저장 변경 사항도 복구할 수 없습니다. 계속할까요?`,
    );
    if (!confirmed) return;
    void runFileOperation(async () => {
      await deleteFileAction({
        path: doc.path,
        mtimeNanos: doc.mtimeNanos,
        size: doc.size,
        contentHash: doc.contentHash,
      });
      removeDocument(doc.id);
      await refreshCurrentWorkspace();
    });
  };

  const handleTabContextAction = (
    view: import("./types").ViewId,
    docId: DocId,
    action: TabContextAction,
  ) => {
    if (renameApplyBusyRef.current) return;
    const current = stateRef.current;
    const doc = current.docs.find((candidate) => candidate.id === docId);
    if (!doc) return;
    const viewDocIds = current.views[view];
    const index = viewDocIds.indexOf(docId);
    if (action === "close") {
      requestCloseDocuments([docId]);
    } else if (action === "close-others") {
      requestCloseDocuments(viewDocIds.filter((candidate) => candidate !== docId));
    } else if (action === "close-right") {
      requestCloseDocuments(index < 0 ? [] : viewDocIds.slice(index + 1));
    } else if (action === "copy-path") {
      void runFileOperation(() => navigator.clipboard.writeText(doc.path));
    } else if (action === "reveal") {
      void runFileOperation(() => revealFileAction(doc.path));
    } else if (action === "rename") {
      renameDocumentFile(doc);
    } else if (action === "delete") {
      deleteDocumentFile(doc);
    }
  };

  // Shared by the toolbar "작업 폴더" button and applink `workspace` targets
  // (§1.4) — both open a folder as the workspace through the same path.
  const setWorkspaceRoot = async (path: string) => {
    if (renameApplyBusyRef.current) {
      setError("이름 변경 적용이 끝난 뒤 작업 폴더를 변경할 수 있습니다.");
      return;
    }
    const workspaceChangeToken = ++workspaceChangeTokenRef.current;
    discardRenamePreview();
    setRenameResult(null);
    const root = await canonicalizeWorkspace(path);
    const listing = await listWorkspaceFiles(root);
    if (renameApplyGuard() || workspaceChangeToken !== workspaceChangeTokenRef.current) {
      throw new Error("이름 변경 적용이 끝난 뒤 작업 폴더를 변경할 수 있습니다.");
    }
    dispatchAction({ type: "setWorkspace", workspaceFolder: root });
    void lspSync.setWorkspace(root);
    setWorkspaceFiles(listing.files);
    setWorkspaceTruncated(listing.truncated);
    setWorkspaceListingRoot(root);
  };

  const handleSetWorkspace = () => {
    if (!hydrated || renameApplyGuard()) return;
    const path = pathInput.trim();
    if (!path) {
      setError("작업 폴더 경로를 입력하세요.");
      return;
    }
    void runFileOperation(async () => {
      await setWorkspaceRoot(path);
      setPathInput("");
    });
  };

  // applink `path` target (§1.4): reuses openPath, then moves the cursor if a
  // line was given. line/column follow 1-based editor convention (not
  // specified by the applink contract itself); column defaults to the start
  // of the line when omitted.
  const openApplinkPath = async (path: string, line: number | null, column: number | null) => {
    const doc = await openPath(path);
    if (line === null) return;
    const position = {
      line: Math.max(0, line - 1),
      character: column !== null ? Math.max(0, column - 1) : 0,
    };
    const status = lspSync.statusForDocument(doc.id);
    const cursor = offsetForPosition(doc.text, position, status?.capabilities.positionEncoding ?? "utf-16");
    dispatchAction({ type: "setCursor", docId: doc.id, cursor });
  };

  const handleQuickOpen = () => {
    if (!hydrated || renameApplyGuard()) return;
    setQuickOpen(true);
    if (state.workspaceFolder && workspaceListingRoot !== state.workspaceFolder) {
      void loadWorkspaceSnapshot(state.workspaceFolder).catch((cause) => {
        setError(cause instanceof Error ? cause.message : String(cause));
      });
    }
  };
  // The global Ctrl/⌘+P listener is intentionally installed once. Keep the
  // latest workspace-aware handler available so keyboard opening also refreshes
  // a restored workspace snapshot instead of capturing the initial null root.
  quickOpenRef.current = handleQuickOpen;

  const reloadExternallyChanged = (path: string) => {
    if (renameApplyBusyRef.current) return;
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
      if (renameApplyBusyRef.current || !current || !snapshotMatches(current, expected)) {
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
      const reloaded = stateRef.current.docs.find((doc) => doc.id === current.id);
      if (reloaded) void lspSync.reload(reloaded);
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
        const restoredDocs = restored.filter((doc): doc is Doc => doc !== null);
        dispatchAction({
          type: "restoreSession",
          session: loaded.session,
          docs: restoredDocs,
        });
        for (const doc of restoredDocs) void lspSync.open(doc);
        void lspSync.setWorkspace(loaded.session.workspace_folder);
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
      if (renameApplyBusyRef.current) {
        // The native transaction owns its snapshot boundary while applying.
        // Queue watcher evidence without replacing the editor buffer; a
        // successful result clears entries for its committed paths below.
        enqueueExternalChange(payload.path);
        return;
      }
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
          if (renameApplyBusyRef.current || !latest || !snapshotMatches(latest, expected)) {
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
          const reloaded = stateRef.current.docs.find((doc) => doc.id === latest.id);
          if (reloaded) void lspSync.reload(reloaded);
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

  // Inbound cross-app open requests (§3): a cold-start argv parse is pulled
  // once via take_pending_open, and a relaunch of this same running instance
  // arrives as the devbox://open event. Both converge on handleOpenRequest so
  // the two paths behave identically. Gated on hydrated — restoreSession
  // replaces the whole docs array, so acting earlier risks the applink open
  // being clobbered by session restore landing after it.
  useEffect(() => {
    if (!hydrated) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;

    const handleOpenRequest = (request: OpenRequest) => {
      const action = routeOpenRequest(request);
      switch (action.kind) {
        case "openFile":
          void runFileOperation(() => openApplinkPath(action.path, action.line, action.column));
          break;
        case "openWorkspace":
          void runFileOperation(() => setWorkspaceRoot(action.path));
          break;
        case "noop":
          console.info(`applink: ${action.reason}`);
          break;
      }
    };

    const consumePendingOpen = () => {
      void takePendingOpen()
        .then((request) => {
          if (!disposed && request) handleOpenRequest(request);
        })
        .catch(() => undefined);
    };
    let coldStartConsumed = false;
    const consumeColdStart = () => {
      if (disposed || coldStartConsumed) return;
      coldStartConsumed = true;
      consumePendingOpen();
    };

    // In browser/Vitest the Tauri bridge is absent; treat that as no relaunch
    // forwarding rather than an unhandled rejection during mount (same
    // pattern as the file-changed listener above).
    void Promise.resolve()
      .then(() => listen<OpenRequest>(APPLINK_OPEN_EVENT, () => consumePendingOpen()))
      .then((stop) => {
        if (disposed) stop();
        else {
          unlisten = stop;
          consumeColdStart();
        }
      })
      .catch(() => {
        consumeColdStart();
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
          quickOpenRef.current();
        }
      } else if (key === "h") {
        if (renameApplyBusyRef.current) return;
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
    setPendingCloseDocIds((current) => {
      const next = current.filter((docId) => state.docs.some((doc) => doc.id === docId));
      return next.length === current.length ? current : next;
    });
  }, [state.docs]);

  return (
    <main className="app-shell">
      {recoveryOpen && recoveryChecked && (
        <RecoveryDialog
          onDone={() => {
            setRecoveryOpen(false);
          }}
        />
      )}
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
      {lspSyncState.lastError && (
        <p className="lsp-sync-status" role="status" aria-live="polite">
          LSP 동기화가 일시적으로 비활성화되었습니다: {lspSyncState.lastError}
        </p>
      )}
      {lspSyncState.staleDiagnostics && (
        <p className="lsp-warning" role="status" aria-live="polite">
          LSP 진단이 최신 문서 상태와 맞지 않아 갱신 중입니다.
        </p>
      )}

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
          Problems
        </button>
        <button type="button" className="toolbar-button" onClick={() => goNav("back")} disabled={navBack.length === 0} title="뒤로">
          ←
        </button>
        <button type="button" className="toolbar-button" onClick={() => goNav("forward")} disabled={navForward.length === 0} title="앞으로">
          →
        </button>
        <button type="button" className="toolbar-button" onClick={() => handleLspNavigation("definition")} disabled={!hydrated || !lspCapability("definition") || lspBusy}>
          정의
        </button>
        <button type="button" className="toolbar-button" onClick={() => handleLspNavigation("references")} disabled={!hydrated || !lspCapability("references") || lspBusy}>
          참조
        </button>
        <button type="button" className="toolbar-button" onClick={handleLspRename} disabled={!hydrated || !lspCapability("rename") || lspBusy}>
          이름 변경
        </button>
        <button type="button" className="toolbar-button" onClick={handleLspFormatting} disabled={!hydrated || !lspCapability("formatting") || lspBusy}>
          포맷
        </button>
        {canManuallyRestartLsp && (
          <button type="button" className="toolbar-button" onClick={handleLspRestart} disabled={lspBusy}>
            LSP 재시작
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
            onTabContextAction={handleTabContextAction}
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
            onTabContextAction={handleTabContextAction}
          />
          <DocHost
            docs={state.docs}
            views={state.views}
            activeDocByView={state.activeDocByView}
            split={state.split}
            renameApplyBusy={renameApplyBusy}
            fontSize={(13 * zoom) / 100}
            onChange={(docId, text) => {
              if (renameApplyBusyRef.current) return;
              const before = stateRef.current.docs.find((doc) => doc.id === docId);
              dispatchAction({ type: "setDocText", docId, text });
              const latest = stateRef.current.docs.find((doc) => doc.id === docId);
              if (before && latest && latest.revision !== before.revision) void lspSync.change(latest);
            }}
            onCursorChange={(docId, cursor) => dispatchAction({ type: "setCursor", docId, cursor })}
            onBookmarksChange={(docId, bookmarks) => dispatchAction({ type: "setBookmarks", docId, bookmarks })}
            onFocusDoc={(view, docId) => dispatchAction({ type: "activateDoc", view, docId })}
            onReplaceCommandReady={handleReplaceCommandReady}
            onBookmarkCommandsReady={handleBookmarkCommandsReady}
            diagnostics={(docId) => lspDiagnostics[docId] ?? []}
            completionSource={completionSourceFor}
            hoverSource={hoverSourceFor}
            canNavigate={(docId, kind) => hydrated && lspCapabilityFor(docId, kind)}
            navigationBusy={lspBusy}
            onNavigate={(docId, kind, cursor) => handleLspNavigation(kind, docId, cursor)}
            onError={setError}
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
      {problemsOpen && (
        <ProblemsPanel
          docs={state.docs.map((doc) => ({ id: doc.id, path: doc.path }))}
          diagnosticsFor={(docId) => lspDiagnostics[docId] ?? []}
          serverStatus={problemsServerStatus}
          onNavigate={(docId, offset) => void handleProblemsNavigate(docId, offset)}
          onClose={() => setProblemsOpen(false)}
        />
      )}
      {lspNavigation && (
        <LspNavigationPanel
          kind={lspNavigation.kind}
          locations={lspNavigation.locations}
          rejected={lspNavigation.rejected}
          onOpen={(location) => {
            setLspNavigation(null);
            void runFileOperation(() => openLspLocation(location));
          }}
          onClose={() => setLspNavigation(null)}
        />
      )}
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
                setPendingCloseDocIds([]);
              }
            }}
          >
            <h2>저장되지 않은 변경 사항</h2>
            <p>
              {pendingCloseDoc.path}에 저장되지 않은 변경 사항이 있습니다. 어떻게 하시겠습니까?
              {pendingCloseDocIds.length > 1 && ` (이후 ${pendingCloseDocIds.length - 1}개 대기)`}
            </p>
            <div className="confirm-dialog-actions">
              <button type="button" className="toolbar-button" autoFocus onClick={() => setPendingCloseDocIds([])}>취소</button>
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

      {(renamePreview || renameResult) && (
        <div className="modal-backdrop" role="presentation">
          <div
            className="rename-dialog"
            role="dialog"
            aria-modal="true"
            aria-label="여러 파일 이름 변경 미리보기"
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                cancelPendingRename();
                if (!renameApplyBusy) setRenameResult(null);
              }
            }}
          >
            {renamePreview && (
              <>
                <h2>여러 파일 이름 변경 미리보기</h2>
                <p className="rename-note">
                  변경 범위와 위치를 확인한 뒤 적용하세요. 모든 파일은 적용 직전에 mtime·크기·SHA-256을 다시 확인하며,
                  하나라도 실패하면 이미 바뀐 파일을 백업으로 되돌립니다.
                </p>
                <ChangeSetPreview
                  items={renamePreview.preview.files.map((file): ChangeSetItem => ({
                    path: file.path,
                    before: file.before,
                    after: file.after,
                    meta: file.ranges.map(({ range }) => `${range.start.line + 1}:${range.start.character + 1}–${range.end.line + 1}:${range.end.character + 1}`).join(", "),
                  }))}
                  title="LSP 이름 변경"
                  approveLabel="전체 적용"
                  selectable={false}
                  disabled={renameApplyBusy}
                  cancelDisabled={false}
                  onApprove={() => applyPendingRename()}
                  onCancel={() => cancelPendingRename()}
                />
              </>
            )}
            {renameResult && (
              <>
                <h2>{renameResult.success ? "이름 변경 완료" : "이름 변경 결과"}</h2>
                <p className="rename-note">
                  {renameResult.error ?? "변경된 파일별 결과를 확인하세요."}
                </p>
                <ul className="rename-results">
                  {renameResult.files.map((file) => (
                    <li key={file.path} className={`rename-result ${file.status}`}>
                      <code>{file.path}</code>
                      <span>{renameFileStatusLabel(file.status)}</span>
                      {file.error && <small>{file.error}</small>}
                    </li>
                  ))}
                </ul>
                <div className="confirm-dialog-actions">
                  <button type="button" className="toolbar-button selected" autoFocus onClick={() => setRenameResult(null)}>
                    닫기
                  </button>
                </div>
              </>
            )}
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
      {lspPanelOpen && (
        <LspControlPanel
          workspaceRoot={state.workspaceFolder}
          onClose={() => setLspPanelOpen(false)}
          onConfigChanged={(config) => void lspSync.setConfig(config)}
        />
      )}
    </main>
  );
}
