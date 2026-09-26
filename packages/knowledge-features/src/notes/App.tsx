import { useUndo } from "@devbox/product-shell/undo";
import { undoCreated } from "./undoCreated";
import { usePolling } from "@devbox/hooks";
import { NoteAutosave, readAutosavePreference, writeAutosavePreference } from "./autosave";
import { NoteJournal } from "./journal";
import RecoveryControls from "./components/RecoveryControls";
import { MetadataRefresh } from "./metadataRefresh";
import { isProductHosted } from "../transport";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { ContextMenu, useContextMenu, type ContextMenuEntry } from "@devbox/context-menu";
import { focusFirst, isImeComposing, restoreFocus, trapDialogKeyDown } from "@devbox/a11y";
import ChangeSetPreview from "@devbox/diff-view";
import {
  saveNoteJournal,
  clearNoteJournal,
  applyRename,
  analyzeWikilinks,
  backlinks as listBacklinks,
  createFile,
  createDirectory,
  dailyNote,
  deleteFile,
  discardKnowledgeDraft,
  discardRenamePreview,
  entryPath,
  listTags,
  listTree,
  knowledgeWatcherStatus,
  onDocsChanged,
  onKnowledgeWatcherStatus,
  onOpenRequest,
  onQuickCaptureRequested,
  onQuickCaptureShortcutStatusChanged,
  openInboundNote,
  openIn,
  openTargets,
  previewKnowledgeDraft,
  readFile,
  renewKnowledgeDraft,
  revealEntry,
  previewRename,
  quickCaptureShortcutStatus,
  renderMarkdown,
  saveImageAsset,
  searchDocs,
  saveKnowledgeDraft,
  takePendingOpen,
  writeFile,
  wikilinkCandidates,
  type OpenRequest,
  type KnowledgeOpenTarget,
  type KnowledgeDraftPreview,
  type RenamePreview,
} from "./api";
import { NoteDocument } from "./noteDocument";
import { registerNoteEditor, useNoteSession } from "./lifecycle";
import MarkdownEditor from "./components/MarkdownEditor";
import MarkdownPreview from "./components/MarkdownPreview";
import QuickCaptureDialog from "./components/QuickCaptureDialog";
import TemplateManager from "./components/TemplateManager";
import type {
  Backlink,
  EditorCursorRequest,
  RenderedDoc,
  SearchResult,
  TreeEntry,
  WikilinkOccurrence,
  QuickCaptureShortcutStatus,
  KnowledgeWatcherStatus,
} from "./types";
import { routeOpenRequest } from "./lib/applink";
import { IMAGE_STALE_ERROR, readImageBytes } from "./lib/imageAssets";
import "./App.css";

type ViewMode = "edit" | "split" | "preview";
type TreeContextTarget = { path: string; isDir: boolean };

const RENDER_DEBOUNCE_MS = 300;
const WIKILINK_DEBOUNCE_MS = 220;
const MAX_DRAFT_TITLE_BYTES = 256;
const MAX_DRAFT_BODY_BYTES = 512 * 1024;

function indent(path: string): number {
  return path.split("/").length - 1;
}

function isMarkdown(path: string | null): boolean {
  return !!path && path.endsWith(".md");
}

function normalizeRelativePath(path: string): string {
  return path.trim().replace(/\\/g, "/").replace(/\/+/g, "/");
}

function parentPath(path: string): string {
  const separator = path.lastIndexOf("/");
  return separator < 0 ? "" : path.slice(0, separator);
}

function childPath(parent: string, name: string): string {
  return parent ? `${parent}/${name}` : name;
}

function isSameOrChild(path: string | null, parent: string): boolean {
  return path === parent || path?.startsWith(`${parent}/`) === true;
}

function utf8Bytes(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function draftNeedsRegeneration(cause: unknown): boolean {
  return cause instanceof Error && cause.name === "draft_stale";
}

function remapPath(path: string | null, from: string, to: string): string | null {
  if (path === from) return to;
  if (path?.startsWith(`${from}/`)) return `${to}${path.slice(from.length)}`;
  return path;
}

function watcherStatusLabel(status: KnowledgeWatcherStatus): string {
  const source = status.sourceKind === "wsl" ? "WSL 저장소 · 5초 폴링" : "Windows 저장소 · 실시간 감시";
  const error =
    status.error === "watcher_state_poisoned"
      ? "색인 중단 · 앱을 다시 시작하세요"
      : status.error === "vault_unconfigured"
        ? "저장소 미설정"
        : status.error === "vault_unavailable"
          ? "저장소 연결 끊김 · 마지막 색인 유지"
          : status.error === "vault_scan_limit"
            ? "안전 색인 한도 초과 · 마지막 색인 유지"
            : status.error === "vault_scan_incomplete"
              ? "일부 파일 읽기 실패 · 마지막 색인 유지"
              : status.error === "vault_index_failed"
                ? "색인 갱신 실패 · 마지막 색인 유지"
                : null;
  return error ? `${source} · ${error}` : source;
}

export default function App({
  active = true,
  onActivate,
  onDaily,
  onVaultSettings,
  openRequest,
  captureRequest,
}: {
  active?: boolean;
  onActivate?: () => void;
  onDaily?: () => void;
  onVaultSettings?: () => void;
  openRequest?: { id: number; path: string };
  captureRequest?: string;
} = {}) {
  const activeRef = useRef(active);
  activeRef.current = active;
  const activateRef = useRef(onActivate);
  activateRef.current = onActivate;
  const [tree, setTree] = useState<TreeEntry[]>([]);
  const [tags, setTags] = useState<string[]>([]);
  const session = useNoteSession();
  const [localDocument] = useState(() => new NoteDocument(readFile, writeFile));
  const editorDocument = session ?? localDocument;
  const note = useSyncExternalStore(editorDocument.subscribe, editorDocument.snapshot);
  const { path: selected, content, dirty, sourceVersion } = note;
  const [autosaveEnabled, setAutosaveEnabled] = useState(() => readAutosavePreference());
  const autosaveRef = useRef<NoteAutosave | null>(null);
  const journalRef = useRef<NoteJournal | null>(null);
  const [recoveryBusy, setRecoveryBusy] = useState(false);
  const recoveryBusyRef = useRef(recoveryBusy);
  recoveryBusyRef.current = recoveryBusy;

  useEffect(() => (session ? undefined : registerNoteEditor(editorDocument)), [session, editorDocument]);
  const [selectedTreePath, setSelectedTreePath] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [mode, setMode] = useState<ViewMode>("edit");
  const [anchorRequest, setAnchorRequest] = useState<{ path: string; fragment: string; id: number } | null>(null);
  const [rendered, setRendered] = useState<{ path: string; sourceVersion: number; doc: RenderedDoc } | null>(null);
  // Gate during render, before passive cleanup; never pair old HTML with a new path.
  const preview = rendered?.sourceVersion === sourceVersion && rendered.path === selected ? rendered : null;
  const [contextTarget, setContextTarget] = useState<TreeContextTarget | null>(null);
  const [availableTargets, setAvailableTargets] = useState<KnowledgeOpenTarget[] | null>(null);
  const [wikilinks, setWikilinks] = useState<WikilinkOccurrence[]>([]);
  const [backlinks, setBacklinks] = useState<Backlink[]>([]);
  const [showBacklinks, setShowBacklinks] = useState(true);
  const [metadataRevision, setMetadataRevision] = useState(0);
  const [cursorRequest, setCursorRequest] = useState<EditorCursorRequest | null>(null);
  const [renamePreview, setRenamePreview] = useState<RenamePreview | null>(null);
  const [renameBusy, setRenameBusy] = useState(false);
  const { offer: offerUndo, toast: undoToast } = useUndo();
  const [quickCaptureOpen, setQuickCaptureOpen] = useState(false);
  useEffect(() => {
    if (captureRequest) {
      activateRef.current?.();
      setQuickCaptureOpen(true);
    }
  }, [captureRequest]);
  const [templateManagerOpen, setTemplateManagerOpen] = useState(false);
  const [quickCaptureShortcut, setQuickCaptureShortcut] = useState<QuickCaptureShortcutStatus | null>(null);
  const [draftPreview, setDraftPreview] = useState<KnowledgeDraftPreview | null>(null);
  const [draftBusy, setDraftBusy] = useState(false);
  const [watcherStatus, setWatcherStatus] = useState<KnowledgeWatcherStatus | null>(null);
  const renameBusyRef = useRef(false);
  const renameDialogRef = useRef<HTMLElement | null>(null);
  const draftBusyRef = useRef(false);
  const draftPreviewRef = useRef<KnowledgeDraftPreview | null>(null);
  const draftDialogRef = useRef<HTMLElement | null>(null);
  const draftRestoreFocusRef = useRef<HTMLElement | null>(null);
  const draftRequestRef = useRef(0);
  const draftMountedRef = useRef(true);
  const cursorTokenRef = useRef(0);
  const quickCaptureButtonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    draftMountedRef.current = true;
    return () => {
      draftMountedRef.current = false;
      draftRequestRef.current += 1;
      const preview = draftPreviewRef.current;
      if (preview && !draftBusyRef.current) {
        draftPreviewRef.current = null;
        void discardKnowledgeDraft(preview.id).catch(() => undefined);
      }
    };
  }, []);

  useEffect(() => {
    draftPreviewRef.current = draftPreview;
  }, [draftPreview]);

  // 인플라이트 렌더 응답이 도착했을 때 "그사이 다른 문서로 전환했는지"를 판단하기 위해
  // 최신 선택값을 ref로도 들고 있는다(설계 결정 3 — 응답 시점에 최신값과 비교).
  const selectedRef = useRef<string | null>(selected);
  // An editor event can arrive immediately after a render (before passive
  // effects run), so image import must not observe the previous note during
  // that short commit window.
  selectedRef.current = selected;
  useEffect(() => {
    selectedRef.current = selected;
  }, [selected]);

  // .md가 아닌 파일로 전환되면 분할/프리뷰를 유지할 수 없으므로 편집 모드로 되돌린다.
  useEffect(() => {
    if (!isMarkdown(selected) && mode !== "edit") setMode("edit");
  }, [selected, mode]);

  // Capture path and source version for the debounced request. Apply only to the same
  // opening/edit generation, including same-path reopenings.
  useEffect(() => {
    if (mode === "edit" || !isMarkdown(selected)) {
      setRendered(null);
      return;
    }
    const rel = selected as string;
    let invalidated = false;
    const timer = setTimeout(() => {
      void renderMarkdown(rel, content)
        .then((doc) => {
          if (invalidated || !draftMountedRef.current || editorDocument.snapshot().sourceVersion !== sourceVersion)
            return;
          setRendered({ path: rel, sourceVersion, doc });
        })
        .catch((e) => {
          if (invalidated || !draftMountedRef.current || editorDocument.snapshot().sourceVersion !== sourceVersion)
            return;
          setError(e instanceof Error ? e.message : String(e));
        });
    }, RENDER_DEBOUNCE_MS);
    return () => {
      invalidated = true;
      clearTimeout(timer);
    };
  }, [content, selected, mode, sourceVersion, editorDocument]);

  const [metadataRefresh] = useState(
    () =>
      new MetadataRefresh(
        async () => {
          // Settle both reads before starting another pair, even when one fails.
          // The filesystem tree and asynchronously indexed tags are not an atomic
          // snapshot; the UI publishes only one complete, current request pair.
          const [t, ts] = await Promise.allSettled([listTree(), listTags()]);
          if (t.status === "rejected") throw t.reason;
          if (ts.status === "rejected") throw ts.reason;
          return [t.value, ts.value] as const;
        },
        ([t, ts]) => {
          setTree(t);
          setTags(ts);
          setMetadataRevision((revision) => revision + 1);
        },
        (e) => setError(e instanceof Error ? e.message : String(e)),
      ),
  );
  const loadMeta = metadataRefresh.request;
  const loadMetaRef = useRef(loadMeta);
  loadMetaRef.current = loadMeta;
  useEffect(() => {
    const autosave = new NoteAutosave(editorDocument, readAutosavePreference(), () => {
      void loadMetaRef.current();
    });
    const journal = new NoteJournal(editorDocument, { save: saveNoteJournal, clear: clearNoteJournal }, (code) => {
      setError(
        code === "journal_limit"
          ? "복구용 임시 저장이 8개로 가득 찼습니다. 남은 복구본을 복원하거나 버린 뒤 다시 편집해 주세요."
          : "복구용 임시 저장을 기록하지 못했습니다. 편집 내용은 유지됩니다.",
      );
    });
    autosaveRef.current = autosave;
    journalRef.current = journal;
    const release = editorDocument.setBeforeSwitch(() => autosave.flush());
    const onBlur = () => {
      if (!recoveryBusyRef.current) void autosave.flush();
    };
    const onVisibility = () => {
      if (document.hidden) onBlur();
    };
    window.addEventListener("blur", onBlur);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.removeEventListener("blur", onBlur);
      document.removeEventListener("visibilitychange", onVisibility);
      release();
      journal.dispose();
      autosave.dispose();
      journalRef.current = null;
      autosaveRef.current = null;
    };
  }, [editorDocument]);
  useEffect(() => {
    autosaveRef.current?.setEnabled(autosaveEnabled);
    writeAutosavePreference(autosaveEnabled);
  }, [autosaveEnabled]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: editorDocument identifies the metadata subscription lifetime; replacing it must restart observation even when request identity is stable.
  useEffect(() => {
    metadataRefresh.start();
    void loadMeta();
    return () => metadataRefresh.stop();
  }, [loadMeta, metadataRefresh, editorDocument]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: metadataRevision invalidates link targets on disk even when content and selected path are unchanged.
  useEffect(() => {
    let disposed = false;
    if (!isMarkdown(selected)) {
      setWikilinks([]);
      return;
    }
    const rel = selected as string;
    const timer = setTimeout(() => {
      void analyzeWikilinks(content)
        .then((links) => {
          if (!disposed && selectedRef.current === rel) setWikilinks(links);
        })
        .catch(() => {
          if (!disposed && selectedRef.current === rel) {
            setWikilinks([]);
            setError("위키링크를 분석하지 못했습니다");
          }
        });
    }, WIKILINK_DEBOUNCE_MS);
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [content, metadataRevision, selected]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: metadataRevision invalidates backlinks after another note changes without changing the selected path.
  useEffect(() => {
    let disposed = false;
    if (!isMarkdown(selected)) {
      setBacklinks([]);
      return;
    }
    const rel = selected as string;
    void listBacklinks(rel)
      .then((links) => {
        if (!disposed && selectedRef.current === rel) setBacklinks(links);
      })
      .catch(() => {
        if (!disposed && selectedRef.current === rel) {
          setBacklinks([]);
          setError("백링크를 불러오지 못했습니다");
        }
      });
    return () => {
      disposed = true;
    };
  }, [metadataRevision, selected]);

  useEffect(() => {
    let disposed = false;
    void openTargets()
      .then((targets) => {
        if (!disposed) setAvailableTargets(targets);
      })
      .catch((cause: unknown) => {
        if (!disposed) {
          setAvailableTargets([]);
          setError(cause instanceof Error ? cause.message : String(cause));
        }
      });
    return () => {
      disposed = true;
    };
  }, []);

  // 외부 편집 watcher가 docs-changed를 보내면 트리·태그를 새로고침한다
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void onDocsChanged(() => {
      if (!disposed) {
        void loadMeta();
        void editorDocument.inspect();
      }
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [loadMeta, editorDocument]);

  useEffect(() => {
    let disposed = false;
    let eventSeen = false;
    let unlisten: (() => void) | undefined;
    void knowledgeWatcherStatus()
      .then((status) => {
        if (!disposed && !eventSeen) setWatcherStatus(status);
      })
      .catch(() => {
        if (!disposed && !eventSeen) {
          setWatcherStatus({
            sourceKind: "native",
            watchMode: "unavailable",
            lastSyncedAt: null,
            error: "vault_unavailable",
          });
        }
      });
    void onKnowledgeWatcherStatus((status) => {
      if (!disposed) {
        eventSeen = true;
        setWatcherStatus(status);
      }
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // Native owns the global registration.  The frontend only receives a
  // bounded event and opens the same modal as the in-app button; it never
  // reads clipboard or filesystem data in the background.
  useEffect(() => {
    let disposed = false;
    let stopRequest: (() => void) | undefined;
    let stopStatus: (() => void) | undefined;
    void onQuickCaptureRequested(() => {
      if (!disposed) {
        activateRef.current?.();
        setQuickCaptureOpen(true);
      }
    })
      .then((stop) => {
        if (disposed) stop();
        else stopRequest = stop;
      })
      .catch(() => {
        if (!disposed)
          setQuickCaptureShortcut(
            (current) =>
              current ?? {
                shortcut: "Ctrl+Alt+K",
                state: "unavailable",
              },
          );
      });
    void onQuickCaptureShortcutStatusChanged((status) => {
      if (!disposed) setQuickCaptureShortcut(status);
    })
      .then((stop) => {
        if (disposed) stop();
        else stopStatus = stop;
      })
      .catch(() => {
        if (!disposed)
          setQuickCaptureShortcut(
            (current) =>
              current ?? {
                shortcut: "Ctrl+Alt+K",
                state: "unavailable",
              },
          );
      });
    void quickCaptureShortcutStatus().then((status) => {
      if (!disposed) setQuickCaptureShortcut(status);
    });
    return () => {
      disposed = true;
      stopRequest?.();
      stopStatus?.();
    };
  }, []);

  const confirmDiscard = useCallback(() => confirm("저장하지 않은 변경사항이 있습니다. 계속할까요?"), []);
  const openFile = async (path: string, fragment?: string) => {
    if (recoveryBusyRef.current) return;
    setError(null);
    const opened =
      (fragment !== undefined && editorDocument.snapshot().path === path) ||
      (await editorDocument.openPath(path, confirmDiscard));
    if (opened) {
      setSelectedTreePath(editorDocument.snapshot().path);
      setCursorRequest(null);
      setAnchorRequest(fragment === undefined ? null : { path, fragment, id: Date.now() });
    }
  };

  const productOpenRef = useRef(openFile);
  productOpenRef.current = openFile;
  const handledProductOpen = useRef<number | null>(null);
  useEffect(() => {
    if (!active || !openRequest || handledProductOpen.current === openRequest.id) return;
    handledProductOpen.current = openRequest.id;
    void productOpenRef.current(openRequest.path);
  }, [active, openRequest]);

  const openIndexedNoteAt = async (path: string, line = 1, column = 1) => {
    if (recoveryBusyRef.current) return;
    setError(null);
    if (
      await editorDocument.open(
        () =>
          openInboundNote(path).catch(() => {
            throw new Error("요청한 노트를 열 수 없습니다");
          }),
        confirmDiscard,
      )
    ) {
      setSelectedTreePath(editorDocument.snapshot().path);
      setMode("edit");
      cursorTokenRef.current += 1;
      setCursorRequest({ line, column, token: cursorTokenRef.current });
    }
  };

  const save = async () => {
    if (!recoveryBusyRef.current && (await editorDocument.save())) await loadMeta();
  };

  const importImageAsset = useCallback(async (file: File) => {
    const note = selectedRef.current;
    if (!note || !isMarkdown(note)) {
      throw new Error("이미지 자산 저장은 마크다운 노트에서만 사용할 수 있습니다");
    }
    const bytes = await readImageBytes(file);
    if (selectedRef.current !== note) {
      throw new Error(IMAGE_STALE_ERROR);
    }
    return saveImageAsset(note, bytes);
  }, []);

  const cancelDraftPreview = useCallback(async () => {
    const preview = draftPreview;
    if (!preview || draftBusyRef.current) return;
    draftBusyRef.current = true;
    setDraftBusy(true);
    setError(null);
    try {
      await discardKnowledgeDraft(preview.id);
      if (!draftMountedRef.current) return;
      draftPreviewRef.current = null;
      setDraftPreview(null);
      setNotice("Knowledge 초안 미리보기를 취소했습니다. 다시 열 수 있습니다.");
    } catch (cause) {
      if (!draftMountedRef.current) return;
      if (draftNeedsRegeneration(cause)) {
        draftPreviewRef.current = null;
        setDraftPreview(null);
        setError("Knowledge 초안이 만료되었거나 저장 위치가 변경되었습니다. 보낸 앱에서 새로 생성하세요.");
      } else {
        setError("Knowledge 초안을 취소하지 못했습니다. 잠시 후 다시 시도하세요.");
      }
    } finally {
      draftBusyRef.current = false;
      if (draftMountedRef.current) setDraftBusy(false);
    }
  }, [draftPreview]);

  const commitDraftPreview = useCallback(async () => {
    const preview = draftPreview;
    if (!preview || draftBusyRef.current) return;
    draftBusyRef.current = true;
    setDraftBusy(true);
    setError(null);
    try {
      const savedDraft: { result?: Awaited<ReturnType<typeof saveKnowledgeDraft>>; failure?: unknown } = {};
      await editorDocument.open(async () => {
        let result: Awaited<ReturnType<typeof saveKnowledgeDraft>>;
        try {
          result = await saveKnowledgeDraft(preview.id);
        } catch (cause) {
          savedDraft.failure = cause;
          throw new Error("Knowledge 초안을 저장하지 못했습니다. 미리보기는 유지됩니다.");
        }
        savedDraft.result = result;
        const saved = await readFile(result.path);
        if (saved.content === null) throw new Error("저장한 초안을 다시 읽지 못했습니다. 현재 편집 내용은 유지됩니다.");
        return { ...saved, path: result.path, content: saved.content };
      }, confirmDiscard);
      if (savedDraft.failure) throw savedDraft.failure;
      const result = savedDraft.result;
      if (!draftMountedRef.current || !result) return;
      draftPreviewRef.current = null;
      setDraftPreview(null);
      setSelectedTreePath(editorDocument.snapshot().path);
      setCursorRequest(null);
      await loadMeta();
      if (!draftMountedRef.current) return;
      setNotice(
        result.handoffDeleted && result.handoffStatusRecorded !== false
          ? "Knowledge 초안을 저장했습니다. handoff는 소비되어 삭제되었습니다."
          : "Knowledge 초안은 저장했지만 소비 상태 기록을 완료하지 못했습니다. 보낸 앱의 상태가 sent 또는 expired로 남을 수 있습니다.",
      );
    } catch (cause) {
      if (!draftMountedRef.current) return;
      if (draftNeedsRegeneration(cause)) {
        draftPreviewRef.current = null;
        setDraftPreview(null);
        setError("Knowledge 저장 위치가 변경되었거나 초안이 만료되었습니다. 보낸 앱에서 새로 생성하세요.");
      } else {
        setError("Knowledge 초안을 저장하지 못했습니다. 미리보기는 유지됩니다.");
      }
    } finally {
      draftBusyRef.current = false;
      if (draftMountedRef.current) setDraftBusy(false);
    }
  }, [draftPreview, loadMeta, editorDocument, confirmDiscard]);

  const openDraftPreview = useCallback(
    async (id: string, kind: KnowledgeDraftPreview["kind"]) => {
      if (dirty && !confirm("저장하지 않은 변경사항이 있습니다. 계속할까요?")) return;
      if (draftBusyRef.current) return;
      const request = draftRequestRef.current + 1;
      draftRequestRef.current = request;
      draftRestoreFocusRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      draftBusyRef.current = true;
      setDraftBusy(true);
      setError(null);
      setNotice(null);
      try {
        const preview = await previewKnowledgeDraft(id, kind);
        if (!draftMountedRef.current || draftRequestRef.current !== request) {
          void discardKnowledgeDraft(preview.id).catch(() => undefined);
          return;
        }
        draftPreviewRef.current = preview;
        setDraftPreview(preview);
      } catch {
        if (!draftMountedRef.current || draftRequestRef.current !== request) return;
        setError("Knowledge 초안을 미리볼 수 없습니다. 보낸 앱에서 새로 생성하세요.");
      } finally {
        draftBusyRef.current = false;
        if (draftMountedRef.current && draftRequestRef.current === request) setDraftBusy(false);
      }
    },
    [dirty],
  );

  // A draft is a modal transaction, not a passive notification.  Keep focus
  // inside it, make Escape equivalent to an explicit cancel, and restore the
  // invoking control after the claim is released.
  useEffect(() => {
    if (!active) return;
    if (!draftPreview) {
      const opener = draftRestoreFocusRef.current;
      draftRestoreFocusRef.current = null;
      if (opener && document.contains(opener)) {
        window.setTimeout(() => {
          if (activeRef.current && draftMountedRef.current && document.contains(opener)) opener.focus();
        }, 0);
      }
      return;
    }
    const dialog = draftDialogRef.current;
    const focusTask = window.setTimeout(() => {
      dialog
        ?.querySelector<HTMLElement>(
          "button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])",
        )
        ?.focus();
    }, 0);
    const onKeyDown = (event: KeyboardEvent) => {
      if (isImeComposing(event)) return;
      if (event.key === "Escape") {
        if (!draftBusyRef.current) void cancelDraftPreview();
        event.preventDefault();
        return;
      }
      if (event.key !== "Tab" || !dialog) return;
      const focusable = Array.from(
        dialog.querySelectorAll<HTMLElement>(
          "button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])",
        ),
      );
      if (focusable.length === 0) {
        event.preventDefault();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.clearTimeout(focusTask);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [active, cancelDraftPreview, draftPreview]);

  // A preview may outlive the generic 60-second claim lease. Renewal never
  // extends the envelope TTL. Expiry/invalid claims close the preview with a
  // fixed regeneration message; transient failures leave it visible.
  usePolling(
    async () => {
      const preview = draftPreviewRef.current;
      if (!preview) return;
      const id = preview.id;
      try {
        const result = await renewKnowledgeDraft(id);
        if (!draftMountedRef.current) return;
        setDraftPreview((current) =>
          current?.id === id ? { ...current, leaseUntilMs: result.leaseUntilMs } : current,
        );
      } catch (cause) {
        if (draftPreviewRef.current?.id !== id || !draftMountedRef.current) return;
        if (draftNeedsRegeneration(cause)) {
          draftPreviewRef.current = null;
          setDraftPreview(null);
          setError("Knowledge 초안이 만료되었거나 더 이상 유효하지 않습니다. 보낸 앱에서 새로 생성하세요.");
        } else {
          setError("Knowledge 초안 미리보기 시간이 만료될 수 있습니다. 저장하거나 취소하세요.");
        }
      }
    },
    { intervalMs: 30_000, active: Boolean(draftPreview), immediate: false },
  );

  const runSearch = async (requestedQuery = query) => {
    const normalized = requestedQuery.trim();
    if (!normalized) {
      setResults([]);
      return;
    }
    setError(null);
    try {
      setResults(await searchDocs(normalized));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleOpenRequest = async (request: OpenRequest) => {
    const action = routeOpenRequest(request);
    switch (action.kind) {
      case "openNote":
        await openIndexedNoteAt(action.path);
        break;
      case "search":
        setQuery(action.query);
        await runSearch(action.query);
        break;
      case "draft":
        await openDraftPreview(action.id, action.handoffKind);
        break;
      case "error":
        setError(action.message);
        break;
    }
  };
  const handleOpenRequestRef = useRef(handleOpenRequest);
  handleOpenRequestRef.current = handleOpenRequest;

  // listener를 먼저 등록한 뒤 cold-start pending slot을 pull한다. Hot relaunch
  // event도 payload를 직접 적용하지 않고 같은 slot을 take해 중복 처리를 막는다.
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    const consumePendingOpen = () => {
      void takePendingOpen()
        .then((request) => {
          if (!disposed && request) {
            activateRef.current?.();
            void handleOpenRequestRef.current(request);
          }
        })
        .catch(() => {
          if (!disposed) setError("열기 요청을 처리하지 못했습니다");
        });
    };
    let coldStartConsumed = false;
    const consumeColdStart = () => {
      if (disposed || coldStartConsumed) return;
      coldStartConsumed = true;
      consumePendingOpen();
    };

    void onOpenRequest(() => consumePendingOpen())
      .then((stop) => {
        if (disposed) stop();
        else {
          unlisten = stop;
          consumeColdStart();
        }
      })
      .catch(() => consumeColdStart());

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const openDaily = async () => {
    if (onDaily) {
      onDaily();
      return;
    }
    await editorDocument.open(async () => {
      const [path] = await dailyNote();
      const saved = await readFile(path);
      if (saved.content === null) throw new Error("일일 노트를 열지 못했습니다.");
      return { ...saved, path, content: saved.content };
    }, confirmDiscard);
    setSelectedTreePath(editorDocument.snapshot().path);
    setCursorRequest(null);
    await loadMeta();
  };

  const newFile = async () => {
    const name = prompt("새 파일 이름 (예: Notes/idea.md)");
    const normalized = name ? normalizeRelativePath(name) : "";
    if (!normalized) return;
    setError(null);
    try {
      await createFile(normalized, "---\ntitle: \n---\n\n");
      await loadMeta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const rename = async (path: string) => {
    if (renameBusyRef.current) return;
    if (dirty) {
      setError("이름을 변경하기 전에 편집 중인 노트를 저장하세요");
      return;
    }
    const name = prompt("새 이름", path);
    const normalized = name ? normalizeRelativePath(name) : "";
    if (!normalized || normalized === path) return;
    setError(null);
    renameBusyRef.current = true;
    setRenameBusy(true);
    try {
      setRenamePreview(await previewRename(path, normalized));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      renameBusyRef.current = false;
      setRenameBusy(false);
    }
  };

  const cancelRename = useCallback(() => {
    const planId = renamePreview?.planId;
    setRenamePreview(null);
    if (planId) void discardRenamePreview(planId);
  }, [renamePreview]);

  const renamePlanId = renamePreview?.planId ?? null;
  useLayoutEffect(() => {
    if (!active || !renamePlanId) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    if (renameDialogRef.current) focusFirst(renameDialogRef.current);
    return () => {
      if (activeRef.current) restoreFocus(opener);
    };
  }, [active, renamePlanId]);

  const commitRename = async () => {
    if (!renamePreview || renameBusyRef.current) return;
    const planId = renamePreview.planId;
    renameBusyRef.current = true;
    setRenameBusy(true);
    setError(null);
    try {
      const applied = await applyRename(planId);
      setSelectedTreePath((value) => remapPath(value, applied.from, applied.to));
      await editorDocument.renamed(applied.from, applied.to);
      setCursorRequest(null);
      setRenamePreview(null);
      await loadMeta();
    } catch (cause) {
      // apply plan은 성공 여부와 무관하게 one-shot이다. 실패한 미리보기를 다시
      // 승인하지 않고 새 스냅샷부터 만들게 한다.
      setRenamePreview(null);
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      renameBusyRef.current = false;
      setRenameBusy(false);
    }
  };

  const remove = async (path: string, isDir = false) => {
    const kind = isDir ? "폴더와 그 안의 모든 항목" : "파일";
    if (!confirm(`'${path}' ${kind}을(를) 삭제할까요? 이 작업은 되돌릴 수 없습니다.`)) return;
    setError(null);
    const completeRemoval = editorDocument.approveRemoval(path);
    try {
      await deleteFile(path);
      if (completeRemoval()) {
        setCursorRequest(null);
      }
      setSelectedTreePath((current) => (isSameOrChild(current, path) ? null : current));
      await loadMeta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const prepareTreeContext = useCallback((_reason: "pointer" | "keyboard", target: HTMLElement) => {
    const path = target.dataset.treePath;
    if (!path) return;
    const next = { path, isDir: target.dataset.treeDir === "true" };
    setSelectedTreePath(path);
    setContextTarget(next);
  }, []);

  const treeContextMenu = useContextMenu({ onBeforeOpen: prepareTreeContext });

  useEffect(() => {
    if (contextTarget && !tree.some((entry) => entry.path === contextTarget.path)) {
      treeContextMenu.close();
      setContextTarget(null);
    }
  }, [contextTarget, tree, treeContextMenu.close]);

  const treeContextItems = useMemo<readonly ContextMenuEntry[]>(() => {
    if (!contextTarget) return [];
    const targetItems: ContextMenuEntry[] = (availableTargets ?? []).map((target) => ({
      type: "item",
      id: `open-in:${target.id}`,
      label: target.displayName,
    }));
    return [
      { type: "item", id: "new-file", label: "새 파일" },
      { type: "item", id: "new-folder", label: "새 폴더" },
      { type: "separator", id: "mutate-separator" },
      { type: "item", id: "rename", label: "이름 변경" },
      { type: "item", id: "delete", label: "삭제", danger: true },
      { type: "separator", id: "path-separator" },
      { type: "item", id: "copy-path", label: "경로 복사" },
      { type: "item", id: "reveal", label: "탐색기에서 열기" },
      {
        type: "submenu",
        id: "open-in",
        label: "다른 앱으로 열기",
        disabled: availableTargets === null || targetItems.length === 0,
        items: targetItems,
      },
    ];
  }, [availableTargets, contextTarget]);

  const createFromContext = async (kind: "file" | "folder", target: TreeContextTarget) => {
    const parent = target.isDir ? target.path : parentPath(target.path);
    const suggestion = childPath(parent, kind === "file" ? "새 노트.md" : "새 폴더");
    const requested = prompt(kind === "file" ? "새 파일 이름" : "새 폴더 이름", suggestion);
    const rel = requested ? normalizeRelativePath(requested) : "";
    if (!rel) return;
    setError(null);
    try {
      if (kind === "file") await createFile(rel, "---\ntitle: \n---\n\n");
      else await createDirectory(rel);
      setSelectedTreePath(rel);
      await loadMeta();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  };

  const runTreeContextAction = async (id: string) => {
    const target = contextTarget;
    if (!target) return;
    if (id === "new-file" || id === "new-folder") {
      await createFromContext(id === "new-file" ? "file" : "folder", target);
      return;
    }
    if (id === "rename") {
      await rename(target.path);
      return;
    }
    if (id === "delete") {
      await remove(target.path, target.isDir);
      return;
    }
    setError(null);
    try {
      if (id === "copy-path") {
        await navigator.clipboard.writeText(await entryPath(target.path));
      } else if (id === "reveal") {
        await revealEntry(target.path);
      } else {
        const destination = availableTargets?.find((candidate) => `open-in:${candidate.id}` === id);
        if (destination) await openIn(destination.id, target.path);
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  };

  return (
    <div className="app">
      {undoToast}
      {quickCaptureOpen && (
        <QuickCaptureDialog
          open={quickCaptureOpen}
          active={active}
          onClose={() => setQuickCaptureOpen(false)}
          onSaved={(created) => {
            offerUndo("노트를 만들었습니다.", async () => {
              await undoCreated(created, editorDocument);
              await loadMeta();
            });
            void loadMeta();
          }}
          restoreFocusRef={quickCaptureButtonRef}
        />
      )}
      {templateManagerOpen && (
        <TemplateManager
          active={active}
          onClose={() => setTemplateManagerOpen(false)}
          onSaved={(result) => {
            setTemplateManagerOpen(false);
            offerUndo("노트를 만들었습니다.", async () => {
              await undoCreated(result, editorDocument);
              await loadMeta();
            });
            void loadMeta();
          }}
        />
      )}
      {draftPreview && (
        <div className="modal-backdrop" role="presentation">
          <section
            className="rename-dialog handoff-dialog"
            ref={draftDialogRef}
            role="dialog"
            aria-modal="true"
            aria-busy={draftBusy}
            aria-labelledby="knowledge-draft-title"
            aria-describedby="knowledge-draft-description"
          >
            <h2 id="knowledge-draft-title">
              {draftPreview.kind === "knowledge-draft/v1"
                ? "Life Log 초안 미리보기"
                : draftPreview.kind === "knowledge-session/v1"
                  ? "개발 세션 요약 미리보기"
                  : draftPreview.kind === "knowledge-result/v1"
                    ? "API Studio 결과 초안 미리보기"
                    : "Developer Toolbox 초안 미리보기"}
            </h2>
            <p className="rename-note" id="knowledge-draft-description">
              저장하기 전 본문과 태그를 확인하세요. 취소하면 파일을 만들지 않고 handoff를 다시 대기 상태로 돌립니다.
            </p>
            <div className="handoff-meta">
              <div>
                <span className="dim">제목</span>
                <strong>{draftPreview.title}</strong>
              </div>
              <div>
                <span className="dim">태그</span>
                <span>{draftPreview.tags.join(", ")}</span>
              </div>
              {draftPreview.summary ? (
                <div>
                  <span className="dim">기간</span>
                  <span>
                    {draftPreview.summary.startDate} ~ {draftPreview.summary.endDate} · {draftPreview.summary.timezone}
                  </span>
                </div>
              ) : (
                <div>
                  <span className="dim">소스</span>
                  <span>
                    {draftPreview.kind === "knowledge-session/v1"
                      ? "Workspace · 선택한 세션 메타데이터"
                      : draftPreview.kind === "knowledge-result/v1"
                        ? "API Studio · 보관한 마스킹 결과"
                        : "Developer Toolbox · 명시적 변환 결과"}
                  </span>
                </div>
              )}
            </div>
            <pre className="handoff-body" aria-label="Knowledge 초안 본문">
              {draftPreview.body}
            </pre>
            <div className="handoff-size" aria-label="Knowledge 초안 크기">
              제목 {utf8Bytes(draftPreview.title).toLocaleString()} / {MAX_DRAFT_TITLE_BYTES.toLocaleString()}바이트 ·
              본문 {utf8Bytes(draftPreview.body).toLocaleString()} / {MAX_DRAFT_BODY_BYTES.toLocaleString()}바이트
            </div>
            <div className="handoff-actions">
              <button type="button" className="btn" onClick={() => void cancelDraftPreview()} disabled={draftBusy}>
                취소
              </button>
              <button
                type="button"
                className="btn active"
                onClick={() => void commitDraftPreview()}
                disabled={draftBusy}
              >
                {draftBusy ? "처리 중…" : "초안 저장"}
              </button>
            </div>
          </section>
        </div>
      )}
      {renamePreview && (
        <div className="modal-backdrop" role="presentation">
          <section
            className="rename-dialog"
            ref={renameDialogRef}
            role="dialog"
            aria-modal="true"
            aria-busy={renameBusy}
            aria-labelledby="rename-dialog-title"
            onKeyDown={(event) => {
              if (!renameDialogRef.current) return;
              trapDialogKeyDown(event, renameDialogRef.current, () => {
                if (!renameBusyRef.current) cancelRename();
              });
            }}
          >
            <h2 id="rename-dialog-title">이름 변경 미리보기</h2>
            <p className="rename-note">
              경로 이동과 연결된 위키링크 변경을 한 번에 적용합니다. 적용 직전에 파일이 달라졌거나 충돌이 생기면 전체
              작업을 중단합니다.
            </p>
            <ChangeSetPreview
              items={renamePreview.items}
              title="변경 파일·링크"
              approveLabel="전체 적용"
              selectable={false}
              disabled={renameBusy}
              onApprove={() => void commitRename()}
              onCancel={cancelRename}
            />
          </section>
        </div>
      )}
      {note.error && <p role="alert">{note.error}</p>}
      {error && (
        <div className="error" role="alert">
          {error}
        </div>
      )}

      {quickCaptureShortcut && ["conflict", "unavailable"].includes(quickCaptureShortcut.state) && (
        <div className="quick-capture-shortcut-warning" role="status">
          전역 단축키 {quickCaptureShortcut.shortcut}를 등록하지 못했습니다. 다른 앱이 사용 중일 수 있습니다. 해당 앱의
          단축키 설정을 변경한 뒤 Knowledge를 다시 시작하거나, 아래 버튼으로 계속 빠르게 기록할 수 있습니다.
        </div>
      )}
      {notice && (
        <div className="notice" role="status">
          {notice}
        </div>
      )}
      <aside className="sidebar" inert={recoveryBusy}>
        <h1 className="app-title">Knowledge</h1>
        {watcherStatus && (
          <p
            className={`vault-watch-status ${watcherStatus.error ? "warning" : ""}`}
            role="status"
            title={
              watcherStatus.lastSyncedAt
                ? `마지막 동기화 ${new Date(watcherStatus.lastSyncedAt).toLocaleString()}`
                : undefined
            }
          >
            {watcherStatusLabel(watcherStatus)}
          </p>
        )}
        <div className="sidebar-row">
          <button
            ref={quickCaptureButtonRef}
            className="btn small quick-capture-trigger"
            type="button"
            aria-keyshortcuts={isProductHosted() ? "Control+Alt+N" : "Control+Alt+K"}
            onClick={() => setQuickCaptureOpen(true)}
          >
            빠른 캡처 <span className="dim">{isProductHosted() ? "Ctrl+Alt+N" : "Ctrl+Alt+K"}</span>
          </button>
          <button className="btn small" onClick={() => void openDaily()}>
            일일 노트
          </button>
          {onVaultSettings && (
            <button className="btn small" type="button" onClick={onVaultSettings}>
              노트 폴더
            </button>
          )}
          <button className="btn small" onClick={() => setTemplateManagerOpen(true)}>
            템플릿
          </button>
          <button className="btn small" onClick={() => void newFile()}>
            + 파일
          </button>
        </div>
        <input
          className="search"
          placeholder="문서 검색..."
          value={query}
          onChange={(e) => setQuery(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (!isImeComposing(e) && e.key === "Enter") void runSearch();
          }}
        />
        <div className="tree">
          {query.trim()
            ? results.map((r) => (
                <button
                  key={r.path}
                  className={`tree-node ${selected === r.path ? "active" : ""}`}
                  onClick={() => void openFile(r.path)}
                >
                  <span className="dim"># </span>
                  {r.title}
                </button>
              ))
            : tree.map((t) => (
                <div key={t.path} className="tree-node-wrap">
                  <button
                    className={`tree-node ${selectedTreePath === t.path ? "active" : ""}`}
                    style={{ paddingLeft: `${8 + indent(t.path) * 14}px` }}
                    data-tree-path={t.path}
                    data-tree-dir={String(t.is_dir)}
                    aria-pressed={selectedTreePath === t.path}
                    onClick={() => {
                      setSelectedTreePath(t.path);
                      if (!t.is_dir) void openFile(t.path);
                    }}
                    {...treeContextMenu.triggerProps}
                  >
                    <span className={t.is_dir ? "dir" : "file"}>
                      {t.is_dir ? "▾ " : ""}
                      {t.path.split("/").pop()}
                    </span>
                  </button>
                  {!t.is_dir && (
                    <span className="tree-actions">
                      <button className="mini" title="이름 변경" onClick={() => void rename(t.path)}>
                        ✎
                      </button>
                      <button className="mini" title="삭제" onClick={() => void remove(t.path)}>
                        ✕
                      </button>
                    </span>
                  )}
                </div>
              ))}
        </div>
        <ContextMenu
          open={treeContextMenu.open}
          anchor={treeContextMenu.anchor}
          items={treeContextItems}
          onSelect={(id) => void runTreeContextAction(id)}
          onClose={treeContextMenu.close}
          restoreFocusTo={treeContextMenu.restoreFocusTo}
          ariaLabel="Knowledge 트리 작업"
        />
        <div className="tags">
          <div className="dim">태그</div>
          {tags.map((t) => (
            <span key={t} className="tag">
              {t}
            </span>
          ))}
        </div>
      </aside>

      <main className="content">
        <RecoveryControls
          document={editorDocument}
          autosave={autosaveRef}
          journal={journalRef}
          onBusy={setRecoveryBusy}
        />
        <div className="note-editor-region" inert={recoveryBusy}>
          {selected ? (
            <>
              <div className="editor-head">
                <span className="path">{selected}</span>
                <div className="mode-toggle">
                  <button className={`btn small ${mode === "edit" ? "active" : ""}`} onClick={() => setMode("edit")}>
                    편집
                  </button>
                  <button
                    className={`btn small ${mode === "split" ? "active" : ""}`}
                    disabled={!isMarkdown(selected)}
                    title={isMarkdown(selected) ? undefined : "마크다운(.md) 파일에서만 프리뷰를 볼 수 있습니다"}
                    onClick={() => setMode("split")}
                  >
                    분할
                  </button>
                  <button
                    className={`btn small ${mode === "preview" ? "active" : ""}`}
                    disabled={!isMarkdown(selected)}
                    title={isMarkdown(selected) ? undefined : "마크다운(.md) 파일에서만 프리뷰를 볼 수 있습니다"}
                    onClick={() => setMode("preview")}
                  >
                    프리뷰
                  </button>
                </div>
                <span className="spacer" />
                {isMarkdown(selected) && (
                  <>
                    <span
                      className={`link-health ${wikilinks.some((link) => link.status !== "resolved") ? "has-unresolved" : ""}`}
                    >
                      미해결 {wikilinks.filter((link) => link.status !== "resolved").length}개
                    </span>
                    <button
                      className={`btn small ${showBacklinks ? "active" : ""}`}
                      aria-pressed={showBacklinks}
                      onClick={() => setShowBacklinks((visible) => !visible)}
                    >
                      백링크 ({backlinks.length})
                    </button>
                  </>
                )}
                <label className="row">
                  <input
                    type="checkbox"
                    checked={autosaveEnabled}
                    onChange={(event) => setAutosaveEnabled(event.currentTarget.checked)}
                  />
                  자동 저장
                </label>
                {note.saving ? (
                  <span role="status">저장 중…</span>
                ) : dirty ? (
                  <span className="dirty">● 저장되지 않음</span>
                ) : null}
                <button className="btn" disabled={note.saving} onClick={() => void save()}>
                  저장
                </button>
              </div>
              {note.conflict && (
                <section aria-label="노트 저장 충돌">
                  <p role="alert">파일이 외부에서 변경되거나 삭제되었습니다. 편집 중인 내용은 유지됩니다.</p>
                  <ChangeSetPreview
                    selectable={false}
                    disabled={note.saving}
                    approveLabel="비교한 내용에 덮어쓰기"
                    onApprove={() => {
                      const revision = note.conflict?.revision;
                      if (revision && confirm("비교한 디스크 내용을 현재 편집 내용으로 덮어쓸까요?"))
                        void editorDocument.save(revision).then((saved) => {
                          if (saved) void loadMeta();
                        });
                    }}
                    items={[
                      {
                        path: selected ?? "노트",
                        before: note.conflict.content ?? "(삭제된 파일)",
                        after: content,
                        meta: "디스크 내용 → 현재 편집 내용",
                      },
                    ]}
                  />
                  <button
                    disabled={note.saving || note.conflict.content === null}
                    onClick={() => {
                      if (selected) void openFile(selected);
                    }}
                  >
                    디스크에서 다시 읽기
                  </button>
                  <button disabled={note.saving} onClick={() => void editorDocument.inspect()}>
                    디스크 상태 다시 확인
                  </button>
                </section>
              )}
              <div className="note-workspace">
                <div className={`editor-body mode-${mode}`}>
                  {mode !== "preview" && (
                    <MarkdownEditor
                      value={content}
                      onChange={(text) => {
                        if (!recoveryBusyRef.current) editorDocument.edit(text);
                      }}
                      onSave={() => void save()}
                      onError={setError}
                      wikilinks={wikilinks}
                      loadWikilinkCandidates={wikilinkCandidates}
                      onNavigateWikilink={(path) => void openIndexedNoteAt(path)}
                      cursorRequest={cursorRequest}
                      documentKey={selected}
                      onImageImport={isMarkdown(selected) ? importImageAsset : undefined}
                    />
                  )}
                  {mode !== "edit" && (
                    <MarkdownPreview
                      anchorRequest={anchorRequest?.path === selected ? anchorRequest : null}
                      doc={preview?.doc ?? null}
                      baseRel={preview?.path ?? selected}
                      onNavigate={(rel, fragment) => void openFile(rel, fragment)}
                      onNavigateWikilink={(rel) => void openIndexedNoteAt(rel)}
                    />
                  )}
                </div>
                {showBacklinks && isMarkdown(selected) && (
                  <aside className="backlink-panel" aria-label="백링크">
                    <div className="backlink-head">백링크</div>
                    {backlinks.length > 0 ? (
                      backlinks.map((link, index) => (
                        <button
                          key={`${link.source_path}-${link.line}-${link.column}-${index}`}
                          className="backlink-item"
                          onClick={() => void openIndexedNoteAt(link.source_path, link.line, link.column)}
                        >
                          <span>{link.source_path}</span>
                          <span className="dim">
                            줄 {link.line}:{link.column}
                          </span>
                        </button>
                      ))
                    ) : (
                      <div className="backlink-empty">백링크가 없습니다.</div>
                    )}
                  </aside>
                )}
              </div>
            </>
          ) : (
            <div className="empty">노트를 선택하거나 일일 노트를 만드세요</div>
          )}
        </div>
      </main>
    </div>
  );
}
