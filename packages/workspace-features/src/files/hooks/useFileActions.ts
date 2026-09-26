import { docFromOpenedFile, fileNameForPath, snapshotMatches } from "../lib/documentPresentation";
import {
  deleteFileAction,
  listWorkspaceFiles,
  openFile,
  renameFileAction,
  revealFileAction,
  validateEncoding,
} from "../api";
import type { TabContextAction } from "../components/TabBar";
import type { Doc, DocId, Encoding } from "../types";
import type * as React from "react";

interface Props {
  renameApplyBusyRef: React.RefObject<boolean>;
  stateRef: React.RefObject<import("../types").EditorState>;
  setError: React.Dispatch<React.SetStateAction<string | null>>;
  runFileOperation: <T>(operation: () => Promise<T>) => Promise<T | undefined>;
  dispatchAction: (action: import("../store/documentStore").EditorAction) => import("../types").EditorState;
  externalChangeVersionRef: React.RefObject<Map<string, number>>;
  lspSync: import("../lspDocumentSync").LspDocumentSync;
  removeExternalChange: (path: string, expectedVersion?: number | undefined) => void;
  setPendingEncodingReopen: React.Dispatch<
    React.SetStateAction<{ docId: string; encoding: import("../types").Encoding } | null>
  >;
  pendingEncodingReopen: { docId: string; encoding: import("../types").Encoding } | null;
  hydrated: boolean;
  renameApplyGuard: () => boolean;
  setQuickOpen: React.Dispatch<React.SetStateAction<boolean>>;
  openPath: (
    path: string,
    metadata?: import("../types").SessionDoc | undefined,
    receivedReference?: string | undefined,
  ) => Promise<import("../types").Doc>;
  setWorkspaceLoading: React.Dispatch<React.SetStateAction<boolean>>;
  setWorkspaceFiles: React.Dispatch<React.SetStateAction<import("../types").WorkspaceFile[]>>;
  setWorkspaceTruncated: React.Dispatch<React.SetStateAction<boolean>>;
  setWorkspaceIncomplete: React.Dispatch<React.SetStateAction<boolean>>;
  setWorkspaceListingRoot: React.Dispatch<React.SetStateAction<string | null>>;
  unregisterWatch: (path: string) => Promise<void>;
  lspFeatureRequestRef: React.RefObject<number>;
  setLspNavigation: React.Dispatch<
    React.SetStateAction<{
      kind: "definition" | "references";
      locations: import("../types").LspLocationTarget[];
      rejected: number;
    } | null>
  >;
  setLspDiagnostics: React.Dispatch<
    React.SetStateAction<
      Record<
        string,
        import("../../../../../node_modules/.pnpm/@codemirror+lint@6.9.7/node_modules/@codemirror/lint/dist/index").Diagnostic[]
      >
    >
  >;
  setNavBack: React.Dispatch<React.SetStateAction<import("../App").NavEntry[]>>;
  setNavForward: React.Dispatch<React.SetStateAction<import("../App").NavEntry[]>>;
  registerWatch: (path: string) => Promise<void>;
  removeDocument: (docId: string) => void;
  requestCloseDocuments: (docIds: readonly string[]) => void;
}

export function useFileActions({
  renameApplyBusyRef,
  stateRef,
  setError,
  runFileOperation,
  dispatchAction,
  externalChangeVersionRef,
  lspSync,
  removeExternalChange,
  setPendingEncodingReopen,
  pendingEncodingReopen,
  hydrated,
  renameApplyGuard,
  setQuickOpen,
  openPath,
  setWorkspaceLoading,
  setWorkspaceFiles,
  setWorkspaceTruncated,
  setWorkspaceIncomplete,
  setWorkspaceListingRoot,
  unregisterWatch,
  lspFeatureRequestRef,
  setLspNavigation,
  setLspDiagnostics,
  setNavBack,
  setNavForward,
  registerWatch,
  removeDocument,
  requestCloseDocuments,
}: Props) {
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
    if (doc.encoding.encodingKind === encoding.encodingKind && doc.encoding.bom === encoding.bom) {
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
      if (stateRef.current.workspaceFolder !== root) return;
      setWorkspaceFiles(listing.files);
      setWorkspaceTruncated(listing.truncated);
      setWorkspaceIncomplete(listing.incomplete);
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
      const renamed = await renameFileAction(
        {
          ...(doc.nativeRevision !== undefined ? { nativeRevision: doc.nativeRevision } : {}),
          path: doc.path,
          mtimeNanos: doc.mtimeNanos,
          size: doc.size,
          contentHash: doc.contentHash,
        },
        newName,
      );
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
      setNavBack((current) =>
        current.map((entry) => (entry.docId === doc.id ? { ...entry, path: renamed.path } : entry)),
      );
      setNavForward((current) =>
        current.map((entry) => (entry.docId === doc.id ? { ...entry, path: renamed.path } : entry)),
      );
      if (!stateRef.current.docs.some((candidate) => candidate.id === doc.id)) {
        await Promise.all([closeOldLsp, stopOldWatch]);
        await refreshCurrentWorkspace();
        return;
      }
      dispatchAction({
        type: "renameDoc",
        ...(renamed.nativeRevision !== undefined ? { nativeRevision: renamed.nativeRevision } : {}),
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
        ...(doc.nativeRevision !== undefined ? { nativeRevision: doc.nativeRevision } : {}),
        path: doc.path,
        mtimeNanos: doc.mtimeNanos,
        size: doc.size,
        contentHash: doc.contentHash,
      });
      removeDocument(doc.id);
      await refreshCurrentWorkspace();
    });
  };

  const handleTabContextAction = (view: import("../types").ViewId, docId: DocId, action: TabContextAction) => {
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
  return {
    loadWorkspaceSnapshot,
    handleTabContextAction,
    requestEncodingReopen,
    handleEncodingConversion,
    confirmEncodingReopen,
    handleOpenFromQuickOpen,
  };
}
