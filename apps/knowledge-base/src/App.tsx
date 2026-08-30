import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import ChangeSetPreview from "@devbox/diff-view";
import {
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
  onDocsChanged,
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
  const message = cause instanceof Error
    ? cause.message
    : typeof cause === "string" ? cause : "";
  return /만료|사용할 수 없|처리할 수 없|저장 위치가 변경/u.test(message);
}

function remapPath(path: string | null, from: string, to: string): string | null {
  if (path === from) return to;
  if (path?.startsWith(`${from}/`)) return `${to}${path.slice(from.length)}`;
  return path;
}

export default function App() {
  const [tree, setTree] = useState<TreeEntry[]>([]);
  const [tags, setTags] = useState<string[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [selectedTreePath, setSelectedTreePath] = useState<string | null>(null);
  const [content, setContent] = useState("");
  const [dirty, setDirty] = useState(false);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [mode, setMode] = useState<ViewMode>("edit");
  const [rendered, setRendered] = useState<RenderedDoc | null>(null);
  const [contextTarget, setContextTarget] = useState<TreeContextTarget | null>(null);
  const [availableTargets, setAvailableTargets] = useState<KnowledgeOpenTarget[] | null>(null);
  const [wikilinks, setWikilinks] = useState<WikilinkOccurrence[]>([]);
  const [backlinks, setBacklinks] = useState<Backlink[]>([]);
  const [showBacklinks, setShowBacklinks] = useState(true);
  const [metadataRevision, setMetadataRevision] = useState(0);
  const [cursorRequest, setCursorRequest] = useState<EditorCursorRequest | null>(null);
  const [renamePreview, setRenamePreview] = useState<RenamePreview | null>(null);
  const [renameBusy, setRenameBusy] = useState(false);
  const [quickCaptureOpen, setQuickCaptureOpen] = useState(false);
  const [templateManagerOpen, setTemplateManagerOpen] = useState(false);
  const [quickCaptureNotice, setQuickCaptureNotice] = useState<string | null>(null);
  const [quickCaptureShortcut, setQuickCaptureShortcut] = useState<QuickCaptureShortcutStatus | null>(null);
  const [draftPreview, setDraftPreview] = useState<KnowledgeDraftPreview | null>(null);
  const [draftBusy, setDraftBusy] = useState(false);
  const renameBusyRef = useRef(false);
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

  // 300ms 디바운스 후 렌더 요청. 요청 시점의 rel을 캡처해두고, 응답이 도착했을 때
  // 그 시점의 선택 문서와 다르면 버린다(늦게 도착한 이전 문서의 결과가 덮어쓰지 않도록).
  useEffect(() => {
    if (mode === "edit" || !isMarkdown(selected)) {
      setRendered(null);
      return;
    }
    const rel = selected as string;
    const timer = setTimeout(() => {
      void renderMarkdown(rel, content)
        .then((doc) => {
          if (!draftMountedRef.current || selectedRef.current !== rel) return;
          setRendered(doc);
        })
        .catch((e) => {
          if (!draftMountedRef.current || selectedRef.current !== rel) return;
          setError(e instanceof Error ? e.message : String(e));
        });
    }, RENDER_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [content, selected, mode]);

  const loadMeta = useCallback(async () => {
    try {
      const [t, ts] = await Promise.all([listTree(), listTags()]);
      if (!draftMountedRef.current) return;
      setTree(t);
      setTags(ts);
      setMetadataRevision((revision) => revision + 1);
    } catch (e) {
      if (!draftMountedRef.current) return;
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void loadMeta();
  }, [loadMeta]);

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
    let unlisten: (() => void) | null = null;
    void onDocsChanged(() => void loadMeta()).then((u) => {
      unlisten = u;
    });
    return () => unlisten?.();
  }, [loadMeta]);

  // Native owns the global registration.  The frontend only receives a
  // bounded event and opens the same modal as the in-app button; it never
  // reads clipboard or filesystem data in the background.
  useEffect(() => {
    let disposed = false;
    let stopRequest: (() => void) | undefined;
    let stopStatus: (() => void) | undefined;
    void onQuickCaptureRequested(() => {
      if (!disposed) setQuickCaptureOpen(true);
    }).then((stop) => {
      if (disposed) stop();
      else stopRequest = stop;
    }).catch(() => {
      if (!disposed) setQuickCaptureShortcut((current) => current ?? {
        shortcut: "Ctrl+Alt+K",
        state: "unavailable",
      });
    });
    void onQuickCaptureShortcutStatusChanged((status) => {
      if (!disposed) setQuickCaptureShortcut(status);
    }).then((stop) => {
      if (disposed) stop();
      else stopStatus = stop;
    }).catch(() => {
      if (!disposed) setQuickCaptureShortcut((current) => current ?? {
        shortcut: "Ctrl+Alt+K",
        state: "unavailable",
      });
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

  useEffect(() => {
    if (!quickCaptureNotice) return;
    const timer = window.setTimeout(() => setQuickCaptureNotice(null), 4_000);
    return () => window.clearTimeout(timer);
  }, [quickCaptureNotice]);

  const openFile = async (path: string) => {
    if (dirty && !confirm("저장하지 않은 변경사항이 있습니다. 계속할까요?")) return;
    setError(null);
    try {
      const text = await readFile(path);
      setSelected(path);
      setSelectedTreePath(path);
      setContent(text);
      setDirty(false);
      setCursorRequest(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const openIndexedNoteAt = async (path: string, line = 1, column = 1) => {
    if (dirty && !confirm("저장하지 않은 변경사항이 있습니다. 계속할까요?")) return;
    setError(null);
    try {
      // Link/backlink path는 raw target이 아니라 backend index가 유일하게 해석한
      // 상대 경로다. 실제 열기 직전에도 canonical root/.md/size 경계를 다시 검증한다.
      const note = await openInboundNote(path);
      setSelected(note.path);
      setSelectedTreePath(note.path);
      setContent(note.content);
      setDirty(false);
      setMode("edit");
      cursorTokenRef.current += 1;
      setCursorRequest({ line, column, token: cursorTokenRef.current });
    } catch {
      setError("연결된 노트를 열 수 없습니다");
    }
  };

  const save = async () => {
    if (!selected) return;
    setError(null);
    try {
      await writeFile(selected, content);
      setDirty(false);
      await loadMeta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
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
      setNotice("Knowledge draft 미리보기를 취소했습니다. 다시 열 수 있습니다.");
    } catch (cause) {
      if (!draftMountedRef.current) return;
      if (draftNeedsRegeneration(cause)) {
        draftPreviewRef.current = null;
        setDraftPreview(null);
        setError("Knowledge draft가 만료되었거나 저장 위치가 변경되었습니다. 보낸 앱에서 새로 생성하세요.");
      } else {
        setError("Knowledge draft를 취소하지 못했습니다. 잠시 후 다시 시도하세요.");
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
      const result = await saveKnowledgeDraft(preview.id);
      if (!draftMountedRef.current) return;
      draftPreviewRef.current = null;
      setDraftPreview(null);
      try {
        const saved = await readFile(result.path);
        if (!draftMountedRef.current) return;
        setSelected(result.path);
        setSelectedTreePath(result.path);
        setContent(saved);
      } catch {
        if (!draftMountedRef.current) return;
        // The native save/index transaction succeeded. Keep the selected path
        // visible without fabricating stale editor content if a reread fails.
        setSelected(result.path);
        setSelectedTreePath(result.path);
        setContent("");
      }
      setDirty(false);
      setCursorRequest(null);
      await loadMeta();
      if (!draftMountedRef.current) return;
      setNotice(result.handoffDeleted && result.handoffStatusRecorded !== false
        ? "Knowledge draft를 저장했습니다. handoff는 소비되어 삭제되었습니다."
        : "Knowledge draft는 저장했지만 소비 상태 기록을 완료하지 못했습니다. 보낸 앱의 상태가 sent 또는 expired로 남을 수 있습니다.");
    } catch (cause) {
      if (!draftMountedRef.current) return;
      if (draftNeedsRegeneration(cause)) {
        draftPreviewRef.current = null;
        setDraftPreview(null);
        setError("Knowledge 저장 위치가 변경되었거나 draft가 만료되었습니다. 보낸 앱에서 새로 생성하세요.");
      } else {
        setError("Knowledge draft를 저장하지 못했습니다. 미리보기는 유지됩니다.");
      }
    } finally {
      draftBusyRef.current = false;
      if (draftMountedRef.current) setDraftBusy(false);
    }
  }, [draftPreview, loadMeta]);

  const openDraftPreview = useCallback(async (
    id: string,
    kind: KnowledgeDraftPreview["kind"],
  ) => {
    if (dirty && !confirm("저장하지 않은 변경사항이 있습니다. 계속할까요?")) return;
    if (draftBusyRef.current) return;
    const request = draftRequestRef.current + 1;
    draftRequestRef.current = request;
    draftRestoreFocusRef.current = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
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
      setError("Knowledge draft를 미리볼 수 없습니다. 보낸 앱에서 새로 생성하세요.");
    } finally {
      draftBusyRef.current = false;
      if (draftMountedRef.current && draftRequestRef.current === request) setDraftBusy(false);
    }
  }, [dirty]);

  // A draft is a modal transaction, not a passive notification.  Keep focus
  // inside it, make Escape equivalent to an explicit cancel, and restore the
  // invoking control after the claim is released.
  useEffect(() => {
    if (!draftPreview) {
      const opener = draftRestoreFocusRef.current;
      draftRestoreFocusRef.current = null;
      if (opener && document.contains(opener)) {
        window.setTimeout(() => {
          if (draftMountedRef.current && document.contains(opener)) opener.focus();
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
  }, [cancelDraftPreview, draftPreview]);

  // A preview may outlive the generic 60-second claim lease. Renewal never
  // extends the envelope TTL. Expiry/invalid claims close the preview with a
  // fixed regeneration message; transient failures leave it visible.
  useEffect(() => {
    if (!draftPreview) return;
    const id = draftPreview.id;
    const renew = () => {
      void renewKnowledgeDraft(id)
        .then((result) => {
          if (!draftMountedRef.current) return;
          setDraftPreview((current) => current?.id === id
            ? { ...current, leaseUntilMs: result.leaseUntilMs }
            : current);
        })
        .catch((cause) => {
          if (draftPreviewRef.current?.id !== id || !draftMountedRef.current) return;
          if (draftNeedsRegeneration(cause)) {
            draftPreviewRef.current = null;
            setDraftPreview(null);
            setError("Knowledge draft가 만료되었거나 더 이상 유효하지 않습니다. 보낸 앱에서 새로 생성하세요.");
          } else {
            setError("Knowledge draft 미리보기 시간이 만료될 수 있습니다. 저장하거나 취소하세요.");
          }
        });
    };
    const timer = window.setInterval(renew, 30_000);
    return () => window.clearInterval(timer);
  }, [draftPreview]);

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
        if (dirty && !confirm("저장하지 않은 변경사항이 있습니다. 계속할까요?")) return;
        setError(null);
        try {
          const note = await openInboundNote(action.path);
          setSelected(note.path);
          setSelectedTreePath(note.path);
          setContent(note.content);
          setDirty(false);
          setCursorRequest(null);
        } catch {
          setError("요청한 노트를 열 수 없습니다");
        }
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
          if (!disposed && request) void handleOpenRequestRef.current(request);
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
    setError(null);
    try {
      const [rel, text] = await dailyNote();
      setSelected(rel);
      setSelectedTreePath(rel);
      setContent(text);
      setDirty(false);
      setCursorRequest(null);
      await loadMeta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
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

  useLayoutEffect(() => {
    if (!renamePreview) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || renameBusyRef.current) return;
      event.preventDefault();
      cancelRename();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [cancelRename, renamePreview]);

  const commitRename = async () => {
    if (!renamePreview || renameBusyRef.current) return;
    const planId = renamePreview.planId;
    renameBusyRef.current = true;
    setRenameBusy(true);
    setError(null);
    try {
      const applied = await applyRename(planId);
      const current = selectedRef.current;
      const mapped = remapPath(current, applied.from, applied.to);
      setSelected(mapped);
      setSelectedTreePath((path) => remapPath(path, applied.from, applied.to));
      if (mapped) {
        try {
          setContent(await readFile(mapped));
        } catch {
          // Native transaction은 이미 성공했다. 이전 경로의 stale editor 내용을 새
          // 경로 아래에 표시하거나 저장하지 않고 metadata refresh는 계속한다.
          setContent("");
          setError("이름은 변경했지만 현재 노트를 다시 읽지 못했습니다");
        }
        setDirty(false);
        setCursorRequest(null);
      }
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
    try {
      await deleteFile(path);
      if (isSameOrChild(selected, path)) {
        setSelected(null);
        setContent("");
        setDirty(false);
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
      {quickCaptureOpen && (
        <QuickCaptureDialog
          open={quickCaptureOpen}
          onClose={() => setQuickCaptureOpen(false)}
          onSaved={() => {
            setQuickCaptureNotice("빠른 캡처를 Inbox에 저장했습니다");
            void loadMeta();
          }}
          restoreFocusRef={quickCaptureButtonRef}
        />
      )}
      {templateManagerOpen && (
        <TemplateManager
          onClose={() => setTemplateManagerOpen(false)}
          onSaved={(result) => {
            setTemplateManagerOpen(false);
            setNotice(result.saved
              ? `템플릿으로 새 노트를 만들었습니다: ${result.path}`
              : "브라우저 미리보기에서는 파일을 만들지 않았습니다. 데스크톱 앱에서 적용하세요.");
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
                ? "Life Log draft 미리보기"
                : "Developer Toolbox draft 미리보기"}
            </h2>
            <p className="rename-note" id="knowledge-draft-description">
              저장하기 전 본문과 태그를 확인하세요. 취소하면 파일을 만들지 않고
              handoff를 다시 대기 상태로 돌립니다.
            </p>
            <div className="handoff-meta">
              <div><span className="dim">Title</span><strong>{draftPreview.title}</strong></div>
              <div><span className="dim">Tags</span><span>{draftPreview.tags.join(", ")}</span></div>
              {draftPreview.summary ? (
                <div><span className="dim">Range</span><span>{draftPreview.summary.startDate} ~ {draftPreview.summary.endDate} · {draftPreview.summary.timezone}</span></div>
              ) : (
                <div><span className="dim">Source</span><span>Developer Toolbox · explicit transform result</span></div>
              )}
            </div>
            <pre className="handoff-body" aria-label="Knowledge draft body">{draftPreview.body}</pre>
            <div className="handoff-size" aria-label="Knowledge draft size">
              Title {utf8Bytes(draftPreview.title).toLocaleString()} / {MAX_DRAFT_TITLE_BYTES.toLocaleString()} bytes · Body {utf8Bytes(draftPreview.body).toLocaleString()} / {MAX_DRAFT_BODY_BYTES.toLocaleString()} bytes
            </div>
            <div className="handoff-actions">
              <button type="button" className="btn" onClick={() => void cancelDraftPreview()} disabled={draftBusy}>
                취소
              </button>
              <button type="button" className="btn active" onClick={() => void commitDraftPreview()} disabled={draftBusy}>
                {draftBusy ? "처리 중…" : "Save draft"}
              </button>
            </div>
          </section>
        </div>
      )}
      {renamePreview && (
        <div className="modal-backdrop" role="presentation">
          <section
            className="rename-dialog"
            role="dialog"
            aria-modal="true"
            aria-busy={renameBusy}
            aria-labelledby="rename-dialog-title"
          >
            <h2 id="rename-dialog-title">이름 변경 미리보기</h2>
            <p className="rename-note">
              경로 이동과 연결된 위키링크 변경을 한 번에 적용합니다. 적용 직전에
              파일이 달라졌거나 충돌이 생기면 전체 작업을 중단합니다.
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
      {error && <div className="error" role="alert">{error}</div>}
      {quickCaptureNotice && <div className="quick-capture-notice" role="status">{quickCaptureNotice}</div>}
      {quickCaptureShortcut && ["conflict", "unavailable"].includes(quickCaptureShortcut.state) && (
        <div className="quick-capture-shortcut-warning" role="status">
          전역 단축키 {quickCaptureShortcut.shortcut}를 등록하지 못했습니다. 다른 앱이 사용 중일 수 있습니다.
          해당 앱의 단축키 설정을 변경한 뒤 Knowledge를 다시 시작하거나, 아래 버튼으로 계속 빠르게 기록할 수 있습니다.
        </div>
      )}
      {notice && <div className="notice" role="status">{notice}</div>}
      <aside className="sidebar">
        <h1 className="app-title">Knowledge</h1>
        <div className="sidebar-row">
          <button
            ref={quickCaptureButtonRef}
            className="btn small quick-capture-trigger"
            type="button"
            aria-keyshortcuts="Control+Alt+K"
            onClick={() => setQuickCaptureOpen(true)}
          >
            빠른 캡처 <span className="dim">Ctrl+Alt+K</span>
          </button>
          <button className="btn small" onClick={() => void openDaily()}>
            Daily note
          </button>
          <button className="btn small" onClick={() => setTemplateManagerOpen(true)}>
            Templates
          </button>
          <button className="btn small" onClick={() => void newFile()}>
            + File
          </button>
        </div>
        <input
          className="search"
          placeholder="Search docs..."
          value={query}
          onChange={(e) => setQuery(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void runSearch();
          }}
        />
        <div className="tree">
          {query.trim()
            ? results.map((r) => (
                <button key={r.path} className={`tree-node ${selected === r.path ? "active" : ""}`} onClick={() => void openFile(r.path)}>
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
                    aria-selected={selectedTreePath === t.path}
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
                      <button className="mini" title="Rename" onClick={() => void rename(t.path)}>
                        ✎
                      </button>
                      <button className="mini" title="Delete" onClick={() => void remove(t.path)}>
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
          <div className="dim">Tags</div>
          {tags.map((t) => (
            <span key={t} className="tag">
              {t}
            </span>
          ))}
        </div>
      </aside>

      <main className="content">
        {selected ? (
          <>
            <div className="editor-head">
              <span className="path">{selected}</span>
              <div className="mode-toggle">
                <button
                  className={`btn small ${mode === "edit" ? "active" : ""}`}
                  onClick={() => setMode("edit")}
                >
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
                  <span className={`link-health ${wikilinks.some((link) => link.status !== "resolved") ? "has-unresolved" : ""}`}>
                    {wikilinks.filter((link) => link.status !== "resolved").length} unresolved
                  </span>
                  <button
                    className={`btn small ${showBacklinks ? "active" : ""}`}
                    aria-pressed={showBacklinks}
                    onClick={() => setShowBacklinks((visible) => !visible)}
                  >
                    Backlinks ({backlinks.length})
                  </button>
                </>
              )}
              {dirty && <span className="dirty">● unsaved</span>}
              <button className="btn" onClick={() => void save()}>
                Save
              </button>
            </div>
            <div className="note-workspace">
              <div className={`editor-body mode-${mode}`}>
                {mode !== "preview" && (
                  <MarkdownEditor
                    value={content}
                    onChange={(text) => {
                      setContent(text);
                      setDirty(true);
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
                    doc={rendered}
                    baseRel={selected}
                    onNavigate={(rel) => void openFile(rel)}
                    onNavigateWikilink={(rel) => void openIndexedNoteAt(rel)}
                  />
                )}
              </div>
              {showBacklinks && isMarkdown(selected) && (
                <aside className="backlink-panel" aria-label="Backlinks">
                  <div className="backlink-head">Backlinks</div>
                  {backlinks.length > 0 ? backlinks.map((link, index) => (
                    <button
                      key={`${link.source_path}-${link.line}-${link.column}-${index}`}
                      className="backlink-item"
                      onClick={() => void openIndexedNoteAt(
                        link.source_path,
                        link.line,
                        link.column,
                      )}
                    >
                      <span>{link.source_path}</span>
                      <span className="dim">line {link.line}:{link.column}</span>
                    </button>
                  )) : (
                    <div className="backlink-empty">No backlinks.</div>
                  )}
                </aside>
              )}
            </div>
          </>
        ) : (
          <div className="empty">Select a note or create a daily note</div>
        )}
      </main>
    </div>
  );
}
