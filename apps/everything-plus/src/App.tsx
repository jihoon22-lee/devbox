import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  addRoot,
  cancelIndex,
  copyPath,
  indexNow,
  indexStatus,
  listSavedQueries,
  listRoots,
  onOpenRequest,
  openFile,
  openIn,
  openTargets,
  removeRoot,
  revealFile,
  deleteSavedQuery,
  saveSavedQuery,
  searchContent,
  searchFiles,
  takePendingOpen,
  watcherStatuses,
  type EverythingOpenTarget,
  type OpenRequest,
} from "./api";
import type {
  ContentResult,
  FileEntry,
  IndexStatus,
  RootInfo,
  RootStatus,
  SavedQuery,
  SearchFilter,
} from "./types";
import { normalizeFilter, routeOpenRequest } from "./lib/applink";
import "./App.css";

const MAX_SEARCH_QUERY_BYTES = 4 * 1024;
const MAX_ROOT_BYTES = 4 * 1024;
const MAX_SAVED_NAME_BYTES = 128;
const MAX_SAVED_QUERY_BYTES = 512;
const SEARCH_INPUT_ERROR = "검색어가 너무 길거나 사용할 수 없는 문자를 포함합니다.";
const SEARCH_ERROR = "검색을 처리하지 못했습니다.";
const INDEX_ERROR = "인덱싱 작업을 처리하지 못했습니다.";
const SAVED_QUERY_ERROR = "저장된 검색을 처리하지 못했습니다.";
const FILTER_ERROR = "검색 필터를 사용할 수 없습니다.";

const EMPTY_FILTER: SearchFilter = {};
const CONTENT_STATUS_OPTIONS = [
  ["", "Any content status"],
  ["indexed", "Indexed"],
  ["truncated", "Truncated / partial"],
  ["failed", "Failed extraction"],
  ["not_indexed", "Not indexed"],
  ["too_large", "Too large"],
  ["unsupported_encoding", "Unsupported encoding"],
  ["read_error", "Read error"],
  ["timeout", "Timed out"],
  ["changed_during_read", "Changed during read"],
  ["skipped_sensitive", "Skipped sensitive"],
  ["no_text", "No text"],
  ["unsupported_encrypted", "Unsupported encrypted"],
  ["extract_error", "Extraction error"],
] as const;

function utf8ByteLength(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function isSearchQueryAllowed(value: string): boolean {
  return (
    utf8ByteLength(value) <= MAX_SEARCH_QUERY_BYTES &&
    !Array.from(value).some((character) => {
      const code = character.codePointAt(0) ?? 0;
      return code < 0x20 || code === 0x7f;
    })
  );
}

function isSavedDefinitionAllowed(name: string, value: string): boolean {
  return (
    utf8ByteLength(name.trim()) <= MAX_SAVED_NAME_BYTES &&
    utf8ByteLength(value.trim()) <= MAX_SAVED_QUERY_BYTES
  );
}

function isFilterEmpty(filter: SearchFilter): boolean {
  return (
    !(filter.extensions?.length) &&
    filter.modifiedAfter == null &&
    filter.modifiedBefore == null &&
    filter.minSize == null &&
    filter.maxSize == null &&
    filter.sourceRootId == null &&
    !filter.contentStatus
  );
}

function normalizeUiFilter(filter: SearchFilter): SearchFilter | null {
  const normalized = normalizeFilter(filter);
  if (normalized) return normalized;
  return isFilterEmpty(filter) ? EMPTY_FILTER : null;
}

function filterCount(filter: SearchFilter): number {
  return [
    Boolean(filter.extensions?.length),
    filter.modifiedAfter != null,
    filter.modifiedBefore != null,
    filter.minSize != null,
    filter.maxSize != null,
    filter.sourceRootId != null,
    Boolean(filter.contentStatus),
  ].filter(Boolean).length;
}

function parseExtensions(value: string): string[] {
  return [...new Set(value.split(",").map((extension) => extension.trim().replace(/^\.+/, "").toLowerCase()).filter(Boolean))];
}

function optionalSize(value: string): number | null | undefined {
  if (!value.trim()) return undefined;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed >= 0 ? parsed : null;
}

function dateInputValue(timestamp: number | undefined): string {
  if (timestamp === undefined) return "";
  const date = new Date(timestamp);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function contentStatusLabel(status: string | null | undefined, truncated = false): string {
  if (truncated || status === "truncated" || status === "partial") return "Truncated / partial";
  if (!status) return "Not indexed";
  return status === "indexed" ? "Indexed" : status.replace(/_/g, " ");
}

function fmtSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function watcherLabel(status: RootStatus): string {
  if (status.error === "root_unavailable") return "연결 끊김";
  if (status.error === "root_scan_limit") return "범위 상한";
  if (status.error === "root_scan_incomplete") return "부분 스캔";
  if (status.error) return "확인 필요";
  if (status.pending > 0) return `${status.pending}개 반영 대기`;
  return status.watchMode === "polling" ? "WSL 주기 확인" : "실시간";
}

function watcherTitle(status: RootStatus): string {
  if (status.error === "root_unavailable") {
    return status.sourceKind === "wsl"
      ? "WSL 배포판 또는 검색 루트에 연결할 수 없어 기존 인덱스를 보존했습니다. 연결되면 자동으로 다시 확인합니다."
      : "검색 루트에 연결할 수 없어 기존 인덱스를 보존했습니다.";
  }
  if (status.error === "root_scan_limit") {
    return "파일 수 상한을 넘어 기존 인덱스를 보존했습니다. 검색 루트를 더 작게 나누세요.";
  }
  if (status.error === "root_scan_incomplete") {
    return "읽을 수 없는 하위 경로가 있어 삭제를 추정하지 않고 기존 인덱스를 보존했습니다.";
  }
  if (status.error) return "증분 인덱스를 확인해야 합니다.";
  return status.watchMode === "polling"
    ? "WSL UNC 루트는 Linux 경로 대소문자를 보존하며 bounded metadata polling으로 반영합니다."
    : "네이티브 파일 시스템 watcher";
}

interface ResultContext {
  path: string;
  name: string;
}

export default function App() {
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<"name" | "content">("name");
  const [regexMode, setRegexMode] = useState(false);
  const [results, setResults] = useState<FileEntry[]>([]);
  const [contentResults, setContentResults] = useState<ContentResult[]>([]);
  const [status, setStatus] = useState<IndexStatus>({
    indexing: false,
    cancel_requested: false,
    total_files: 0,
    indexed_files: 0,
    content_indexed_files: 0,
    content_truncated_files: 0,
    content_failed_files: 0,
    roots: 0,
    last_indexed_at: null,
    last_error: null,
  });
  const [roots, setRoots] = useState<RootInfo[]>([]);
  const [watchStatus, setWatchStatus] = useState<RootStatus[]>([]);
  const [newRoot, setNewRoot] = useState("");
  const [newRootContent, setNewRootContent] = useState(false);
  const [filter, setFilter] = useState<SearchFilter>(EMPTY_FILTER);
  const [extensionInput, setExtensionInput] = useState("");
  const [filterOpen, setFilterOpen] = useState(false);
  const [savedQueries, setSavedQueries] = useState<SavedQuery[]>([]);
  const [savedName, setSavedName] = useState("");
  const [editingSavedId, setEditingSavedId] = useState<number | undefined>(undefined);
  const [savedSelection, setSavedSelection] = useState("");
  const [savedQueryBusy, setSavedQueryBusy] = useState(false);
  const [regexError, setRegexError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [activeIdx, setActiveIdx] = useState(-1);
  const [contextResult, setContextResult] = useState<ResultContext | null>(null);
  const [availableTargets, setAvailableTargets] = useState<EverythingOpenTarget[] | null>(null);
  const [indexActionBusy, setIndexActionBusy] = useState(false);
  const mounted = useRef(true);
  const seq = useRef(0);
  const savedQueriesSeq = useRef(0);
  const openRequestSeq = useRef(0);
  const metaSeq = useRef(0);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const loadMeta = useCallback(async () => {
    const requestSeq = ++metaSeq.current;
    try {
      const [st, rs, ws] = await Promise.all([indexStatus(), listRoots(), watcherStatuses()]);
      if (!mounted.current || requestSeq !== metaSeq.current) return;
      setStatus(st);
      setRoots(rs);
      setWatchStatus(ws);
    } catch {
      if (!mounted.current || requestSeq !== metaSeq.current) return;
      setError(INDEX_ERROR);
    }
  }, []);

  // 인덱싱 중에는 진행률을 주기적으로 갱신
  useEffect(() => {
    if (!status.indexing) return;
    const id = setInterval(() => void loadMeta(), 500);
    return () => clearInterval(id);
  }, [status.indexing, loadMeta]);

  useEffect(() => {
    void loadMeta();
  }, [loadMeta]);

  useEffect(() => {
    let disposed = false;
    const requestSeq = ++savedQueriesSeq.current;
    void listSavedQueries()
      .then((saved) => {
        if (!disposed && savedQueriesSeq.current === requestSeq) setSavedQueries(saved);
      })
      .catch(() => {
        if (!disposed && savedQueriesSeq.current === requestSeq) setError(SAVED_QUERY_ERROR);
      });
    return () => {
      disposed = true;
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    void openTargets()
      .then((targets) => {
        if (!disposed) setAvailableTargets(targets);
      })
      .catch(() => {
        if (!disposed) {
          setAvailableTargets([]);
          setError("다른 앱으로 열기 대상을 확인하지 못했습니다");
        }
      });
    return () => {
      disposed = true;
    };
  }, []);

  const handleOpenRequest = (request: OpenRequest) => {
    const action = routeOpenRequest(request);
    if (action.kind === "error") {
      setError(action.message);
      return;
    }

    setError(null);
    setRegexError(null);
    setMode("name");
    setRegexMode(false);
    setQuery(action.query);
    setFilter(action.filter ?? EMPTY_FILTER);
    setExtensionInput(action.filter?.extensions?.join(", ") ?? "");
    setFilterOpen(Boolean(action.filter));
  };
  const handleOpenRequestRef = useRef(handleOpenRequest);
  handleOpenRequestRef.current = handleOpenRequest;

  // Event listener를 먼저 준비한 다음 cold request를 pull한다. Hot event도
  // payload를 직접 적용하지 않고 같은 one-shot pending slot을 소비한다.
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    const consumePendingOpen = () => {
      const requestSeq = ++openRequestSeq.current;
      void takePendingOpen()
        .then((request) => {
          if (!disposed && requestSeq === openRequestSeq.current && request) {
            handleOpenRequestRef.current(request);
          }
        })
        .catch(() => {
          if (!disposed && requestSeq === openRequestSeq.current) {
            setError("검색 요청을 처리하지 못했습니다");
          }
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

  useEffect(() => {
    const current = ++seq.current;
    const q = query.trim();
    setActiveIdx(-1);
    if (!isSearchQueryAllowed(query)) {
      setResults([]);
      setContentResults([]);
      setRegexError(null);
      setError(SEARCH_INPUT_ERROR);
      return;
    }
    if (!q) {
      setResults([]);
      setContentResults([]);
      setRegexError(null);
      return;
    }
    if (mode !== "name" || !regexMode) setRegexError(null);
    let cancelled = false;
    const t = setTimeout(async () => {
      try {
        if (mode === "content") {
          const next = isFilterEmpty(filter) ? await searchContent(q) : await searchContent(q, undefined, filter);
          if (!cancelled && seq.current === current) setContentResults(next);
        } else if (regexMode) {
          let re: RegExp;
          try {
            re = new RegExp(q, "i");
          } catch {
            if (!cancelled && seq.current === current) {
              setRegexError("정규식을 해석할 수 없습니다.");
            }
            return;
          }
          if (!cancelled && seq.current === current) setRegexError(null);
          const all = isFilterEmpty(filter)
            ? await searchFiles(q.replace(/[^a-zA-Z0-9\s]/g, ""), 2000)
            : await searchFiles(q.replace(/[^a-zA-Z0-9\s]/g, ""), 2000, filter);
          if (!cancelled && seq.current === current) setResults(all.filter((f) => re.test(f.name)));
        } else {
          if (!cancelled && seq.current === current) setRegexError(null);
          const next = isFilterEmpty(filter) ? await searchFiles(q) : await searchFiles(q, undefined, filter);
          if (!cancelled && seq.current === current) setResults(next);
        }
      } catch {
        if (!cancelled && seq.current === current) {
          setError(SEARCH_ERROR);
        }
      }
    }, 150);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [query, mode, regexMode, filter]);

  useEffect(() => {
    if (status.last_error === "indexing_failed") {
      setError("인덱싱을 완료하지 못했습니다. 다시 시도해 주세요.");
    }
  }, [status.last_error]);

  const onAddRoot = async () => {
    if (!newRoot.trim()) return;
    setError(null);
    try {
      await addRoot(newRoot.trim(), newRootContent);
      setNewRoot("");
      setNewRootContent(false);
      await loadMeta();
    } catch {
      setError(INDEX_ERROR);
    }
  };

  const onReindex = async () => {
    if (indexActionBusy) return;
    setIndexActionBusy(true);
    setError(null);
    try {
      if (status.indexing) await cancelIndex();
      else await indexNow();
      await loadMeta();
    } catch {
      if (mounted.current) setError(INDEX_ERROR);
    } finally {
      if (mounted.current) setIndexActionBusy(false);
    }
  };

  const onSaveQuery = async () => {
    if (savedQueryBusy) return;
    if (!isSavedDefinitionAllowed(savedName, query)) {
      setError(SAVED_QUERY_ERROR);
      return;
    }
    const normalizedFilter = normalizeFilter(filter);
    if (!normalizedFilter && !isFilterEmpty(filter)) {
      setError(FILTER_ERROR);
      return;
    }
    setSavedQueryBusy(true);
    ++savedQueriesSeq.current;
    setError(null);
    try {
      const saved = await saveSavedQuery({
        ...(editingSavedId === undefined ? {} : { id: editingSavedId }),
        name: savedName,
        query: query.trim(),
        filter: normalizedFilter ?? EMPTY_FILTER,
      });
      if (mounted.current) {
        setSavedQueries((previous) => [saved, ...previous.filter((item) => item.id !== saved.id)]);
        setSavedName("");
        setEditingSavedId(undefined);
        setSavedSelection("");
      }
    } catch {
      if (mounted.current) setError(SAVED_QUERY_ERROR);
    } finally {
      if (mounted.current) setSavedQueryBusy(false);
    }
  };

  const onLoadSavedQuery = (id: string) => {
    setSavedSelection("");
    const saved = savedQueries.find((item) => String(item.id) === id);
    if (!saved) return;
    const normalizedFilter = normalizeFilter(saved.filter);
    if (saved.filter && !normalizedFilter && Object.keys(saved.filter).length > 0) {
      setError(SAVED_QUERY_ERROR);
      return;
    }
    setQuery(saved.query);
    setFilter(normalizedFilter ?? EMPTY_FILTER);
    setExtensionInput(normalizedFilter?.extensions?.join(", ") ?? "");
    setSavedName(saved.name);
    setEditingSavedId(saved.id);
    setFilterOpen(true);
    setError(null);
  };

  const onDeleteSavedQuery = async (saved: SavedQuery) => {
    if (savedQueryBusy) return;
    setSavedQueryBusy(true);
    ++savedQueriesSeq.current;
    setError(null);
    try {
      await deleteSavedQuery(saved.id);
      if (mounted.current) {
        setSavedQueries((previous) => previous.filter((item) => item.id !== saved.id));
        if (editingSavedId === saved.id) {
          setEditingSavedId(undefined);
          setSavedName("");
        }
      }
    } catch {
      if (mounted.current) setError(SAVED_QUERY_ERROR);
    } finally {
      if (mounted.current) setSavedQueryBusy(false);
    }
  };

  const clearFilters = () => {
    setFilter(EMPTY_FILTER);
    setExtensionInput("");
    setError(null);
  };

  const pct = status.total_files > 0 ? Math.round((status.indexed_files / status.total_files) * 100) : 0;

  const activeList = mode === "content" ? contentResults : results;
  const activePath = (i: number) => (mode === "content" ? contentResults[i]?.path : results[i]?.path) ?? null;

  const prepareContextResult = useCallback((target: HTMLElement) => {
    const indexText = target.dataset.resultIndex;
    if (indexText === undefined) return;
    const index = Number.parseInt(indexText, 10);
    const result = activeList[index];
    if (!Number.isInteger(index) || !result) return;
    setActiveIdx(index);
    setContextResult({ path: result.path, name: result.name });
  }, [activeList]);

  const contextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareContextResult(target),
  });

  // Result replacement invalidates the exact menu target. Run this before
  // paint: a passive effect could otherwise close a keyboard menu that the
  // user opened on the freshly rendered row in the same frame.
  useLayoutEffect(() => {
    contextMenu.close();
    setContextResult(null);
  }, [activeList, contextMenu.close]);

  const moveActive = (dir: 1 | -1) => {
    if (activeList.length === 0) return;
    setActiveIdx((prev) => {
      const base = prev === -1 ? (dir === 1 ? -1 : 0) : prev;
      return Math.min(activeList.length - 1, Math.max(0, base + dir));
    });
  };

  const onOpenActive = async () => {
    const path = activeIdx >= 0 ? activePath(activeIdx) : null;
    if (!path) return;
    setError(null);
    try {
      await openFile(path);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      moveActive(1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      moveActive(-1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      void onOpenActive();
    } else if (e.key === "Escape") {
      setQuery("");
      setActiveIdx(-1);
    }
  };

  const onRowAction = async (path: string, action: "open" | "folder" | "copy") => {
    setError(null);
    try {
      if (action === "open") await openFile(path);
      else if (action === "folder") await revealFile(path);
      else await copyPath(path);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const contextMenuItems = useMemo<readonly ContextMenuEntry[]>(() => {
    if (!contextResult) return [];
    const targetItems: ContextMenuEntry[] = (availableTargets ?? []).map((target) => ({
      type: "item",
      id: `open-in:${target.id}`,
      label: target.displayName,
    }));
    return [
      { type: "item", id: "open", label: "Open" },
      { type: "item", id: "reveal", label: "Show in folder" },
      { type: "separator", id: "copy-separator" },
      { type: "item", id: "copy-path", label: "Copy path" },
      { type: "item", id: "copy-name", label: "Copy file name" },
      { type: "separator", id: "open-in-separator" },
      {
        type: "submenu",
        id: "open-in",
        label: "Open in another app",
        disabled: availableTargets === null || targetItems.length === 0,
        items: targetItems,
      },
    ];
  }, [availableTargets, contextResult]);

  const onContextMenuSelect = (id: string) => {
    const result = contextResult;
    if (!result) return;
    if (id === "open") {
      void onRowAction(result.path, "open");
      return;
    }
    if (id === "reveal") {
      void onRowAction(result.path, "folder");
      return;
    }
    if (id === "copy-path") {
      void onRowAction(result.path, "copy");
      return;
    }
    if (id === "copy-name") {
      void navigator.clipboard.writeText(result.name).catch((cause: unknown) => {
        setError(cause instanceof Error ? cause.message : String(cause));
      });
      return;
    }
    const target = availableTargets?.find((candidate) => `open-in:${candidate.id}` === id);
    if (!target) return;
    setError(null);
    void openIn(target.id, result.path).catch((cause: unknown) => {
      setError(cause instanceof Error ? cause.message : String(cause));
    });
  };

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">Everything+</h1>
        <div className="mode-tabs">
          <button
            className={`mode-tab ${mode === "name" ? "active" : ""}`}
            aria-pressed={mode === "name"}
            onClick={() => setMode("name")}
          >
            Name
          </button>
          <button
            className={`mode-tab ${mode === "content" ? "active" : ""}`}
            aria-pressed={mode === "content"}
            onClick={() => setMode("content")}
          >
            Content
          </button>
        </div>
        <input
          className="search"
          aria-label={mode === "content" ? "Search file contents" : "Search file names"}
          placeholder={mode === "content" ? "Search file contents..." : "Search file names..."}
          value={query}
          maxLength={MAX_SEARCH_QUERY_BYTES}
          onChange={(e) => {
            const value = e.currentTarget.value;
            if (isSearchQueryAllowed(value)) {
              setError(null);
              setQuery(value);
            } else {
              setError(SEARCH_INPUT_ERROR);
            }
          }}
          autoFocus
        />
        {mode === "name" && (
          <label className="regex-toggle">
            <input type="checkbox" checked={regexMode} onChange={(e) => setRegexMode(e.currentTarget.checked)} />
            regex
          </label>
        )}
        <span className="status" aria-live="polite">
          {status.indexing
            ? `${status.cancel_requested ? "Cancelling..." : "Indexing..."} ${status.indexed_files.toLocaleString()} files`
            : `${status.total_files.toLocaleString()} files`}
        </span>
        <span className="content-status" aria-live="polite">
          Content {status.content_indexed_files.toLocaleString()} indexed · {status.content_failed_files.toLocaleString()} skipped
          {status.content_truncated_files > 0 && ` · ${status.content_truncated_files.toLocaleString()} truncated`}
        </span>
        <button
          className="btn"
          onClick={() => void onReindex()}
          disabled={status.cancel_requested || indexActionBusy}
          aria-busy={indexActionBusy}
        >
          {status.indexing ? "Cancel" : "Re-index"}
        </button>
      </header>

      {status.indexing && (
        <div className="progress">
          <div className="progress-bar" style={{ width: `${Math.max(4, pct)}%` }} />
          <span className="progress-text">{pct}% ({status.indexed_files.toLocaleString()})</span>
        </div>
      )}

      {error && <div className="error">{error}</div>}
      {regexError && <div className="error">{regexError}</div>}

      <div className="query-tools" aria-label="Saved searches and filters">
        <button
          className="btn"
          type="button"
          aria-expanded={filterOpen}
          aria-controls="search-filters"
          onClick={() => setFilterOpen((open) => !open)}
        >
          Filters{filterCount(filter) > 0 ? ` (${filterCount(filter)})` : ""}
        </button>
        <label className="saved-name-label">
          Save current query as
          <input
            className="saved-name"
            aria-label="Saved query name"
            placeholder="Saved query name"
            value={savedName}
            maxLength={MAX_SAVED_NAME_BYTES}
            onChange={(event) => setSavedName(event.currentTarget.value)}
          />
        </label>
        <button
          className="btn"
          type="button"
          onClick={() => void onSaveQuery()}
          disabled={
            savedQueryBusy
              || !query.trim()
              || !savedName.trim()
              || !isSavedDefinitionAllowed(savedName, query)
          }
          aria-busy={savedQueryBusy}
        >
          Save query
        </button>
        <select
          className="saved-select"
          aria-label="Load saved query"
          value={savedSelection}
          onChange={(event) => onLoadSavedQuery(event.currentTarget.value)}
        >
          <option value="">Load saved query…</option>
          {savedQueries.map((saved) => (
            <option key={saved.id} value={saved.id}>
              {saved.name}
            </option>
          ))}
        </select>
        {savedQueries.length > 0 && (
          <div className="saved-list" aria-label="Saved query actions">
            {savedQueries.map((saved) => (
              <span className="saved-chip" key={saved.id}>
                <button type="button" className="saved-load" onClick={() => onLoadSavedQuery(String(saved.id))}>
                  {saved.name}
                </button>
                <button
                  type="button"
                  className="saved-delete"
                  aria-label={`Delete saved query ${saved.name}`}
                  title={`Delete ${saved.name}`}
                  onClick={() => void onDeleteSavedQuery(saved)}
                  disabled={savedQueryBusy}
                >
                  ×
                </button>
              </span>
            ))}
          </div>
        )}
      </div>

      {filterOpen && (
        <section id="search-filters" className="filter-panel" aria-label="Search filters">
          <label>
            Extensions
            <input
              aria-label="File extensions"
              placeholder="rs, md, pdf"
              value={extensionInput}
              onChange={(event) => {
                const value = event.currentTarget.value;
                setExtensionInput(value);
                const next = normalizeUiFilter({ ...filter, extensions: parseExtensions(value) });
                if (!next) {
                  setError(FILTER_ERROR);
                  return;
                }
                setError(null);
                setFilter(next);
              }}
            />
          </label>
          <label>
            Minimum size (bytes)
            <input
              aria-label="Minimum size in bytes"
              type="number"
              min={0}
              value={filter.minSize ?? ""}
              onChange={(event) => {
                const value = optionalSize(event.currentTarget.value);
                if (value === null) {
                  setError(FILTER_ERROR);
                  return;
                }
                const next = normalizeUiFilter({ ...filter, minSize: value });
                if (!next) {
                  setError(FILTER_ERROR);
                  return;
                }
                setError(null);
                setFilter(next);
              }}
            />
          </label>
          <label>
            Maximum size (bytes)
            <input
              aria-label="Maximum size in bytes"
              type="number"
              min={0}
              value={filter.maxSize ?? ""}
              onChange={(event) => {
                const value = optionalSize(event.currentTarget.value);
                if (value === null) {
                  setError(FILTER_ERROR);
                  return;
                }
                const next = normalizeUiFilter({ ...filter, maxSize: value });
                if (!next) {
                  setError(FILTER_ERROR);
                  return;
                }
                setError(null);
                setFilter(next);
              }}
            />
          </label>
          <label>
            Modified after
            <input
              aria-label="Modified after"
              type="datetime-local"
              value={dateInputValue(filter.modifiedAfter)}
              onChange={(event) => {
                const value = event.currentTarget.value;
                const timestamp = value ? Date.parse(value) : Number.NaN;
                if (value && !Number.isSafeInteger(timestamp)) {
                  setError(FILTER_ERROR);
                  return;
                }
                const next = normalizeUiFilter({ ...filter, modifiedAfter: value ? timestamp : undefined });
                if (!next) {
                  setError(FILTER_ERROR);
                  return;
                }
                setError(null);
                setFilter(next);
              }}
            />
          </label>
          <label>
            Modified before
            <input
              aria-label="Modified before"
              type="datetime-local"
              value={dateInputValue(filter.modifiedBefore)}
              onChange={(event) => {
                const value = event.currentTarget.value;
                const timestamp = value ? Date.parse(value) : Number.NaN;
                if (value && !Number.isSafeInteger(timestamp)) {
                  setError(FILTER_ERROR);
                  return;
                }
                const next = normalizeUiFilter({ ...filter, modifiedBefore: value ? timestamp : undefined });
                if (!next) {
                  setError(FILTER_ERROR);
                  return;
                }
                setError(null);
                setFilter(next);
              }}
            />
          </label>
          <label>
            Source root
            <select
              aria-label="Source root"
              value={filter.sourceRootId ?? ""}
              onChange={(event) => {
                const value = event.currentTarget.value;
                const next = normalizeUiFilter({
                  ...filter,
                  sourceRootId: value ? Number(value) : undefined,
                });
                if (!next) {
                  setError(FILTER_ERROR);
                  return;
                }
                setError(null);
                setFilter(next);
              }}
            >
              <option value="">All roots</option>
              {roots.map((root) => <option key={root.id} value={root.id}>{root.path}</option>)}
            </select>
          </label>
          <label>
            Content status
            <select
              aria-label="Content status filter"
              value={filter.contentStatus ?? ""}
              onChange={(event) => {
                const value = event.currentTarget.value;
                const next = normalizeUiFilter({ ...filter, contentStatus: value || undefined });
                if (!next) {
                  setError(FILTER_ERROR);
                  return;
                }
                setError(null);
                setFilter(next);
              }}
            >
              {CONTENT_STATUS_OPTIONS.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
            </select>
          </label>
          <button className="btn" type="button" onClick={clearFilters} disabled={isFilterEmpty(filter)}>
            Clear filters
          </button>
          <p className="filter-note">Filters are applied in the native bounded query; saved searches store only query and filter definitions.</p>
        </section>
      )}

      <div className="roots">
        <span className="dim">Roots:</span>
        {roots.map((r) => {
          const ws = watchStatus.find((w) => w.root === r.path);
          return (
            <span key={r.path} className="root-chip" title={r.content ? "content index on" : "name only"}>
              {r.path}
              {ws?.sourceKind === "wsl" && <span className="root-tag">WSL</span>}
              {r.content && <span className="root-tag">content</span>}
              {ws && (
                <span className={`watch-state ${ws.error ? "watch-error" : ""}`} title={watcherTitle(ws)}>
                  {watcherLabel(ws)}
                </span>
              )}
              <button className="root-del" title="Remove root" onClick={() => void removeRoot(r.path).then(loadMeta)}>
                ✕
              </button>
            </span>
          );
        })}
        <input
          className="root-input"
          placeholder="검색 루트 (C:\projects 또는 \\wsl$\Ubuntu\home\...)"
          value={newRoot}
          maxLength={MAX_ROOT_BYTES}
          onChange={(e) => setNewRoot(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void onAddRoot();
          }}
        />
        <label className="regex-toggle">
          <input type="checkbox" checked={newRootContent} onChange={(e) => setNewRootContent(e.currentTarget.checked)} />
          index content
        </label>
        <button className="btn" onClick={() => void onAddRoot()}>
          Add
        </button>
      </div>

      <div className="table-wrap" onKeyDown={onKeyDown}>
        {mode === "content" ? (
          <table>
            <thead>
              <tr>
                <th>NAME</th>
                <th>SNIPPET</th>
                <th>PATH</th>
                <th>CONTENT</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {contentResults.map((f, i) => (
                <tr
                  key={`${f.path}-${i}`}
                  className={i === activeIdx ? "active-row" : ""}
                  data-result-index={i}
                  tabIndex={0}
                  aria-selected={i === activeIdx}
                  onMouseEnter={() => setActiveIdx(i)}
                  onClick={() => {
                    setActiveIdx(i);
                    void onRowAction(f.path, "open");
                  }}
                  {...contextMenu.triggerProps}
                >
                  <td>
                    <span className="name">{f.name}</span>
                  </td>
                  <td className="snippet">{f.snippet}</td>
                  <td className="mono dim">{f.path}</td>
                  <td className="status-cell">{contentStatusLabel(f.content_status, f.truncated)}</td>
                  <td className="row-actions">
                    <button className="mini" title="Open" onClick={(e) => { e.stopPropagation(); void onRowAction(f.path, "open"); }}>Open</button>
                    <button className="mini" title="Show in folder" onClick={(e) => { e.stopPropagation(); void onRowAction(f.path, "folder"); }}>Folder</button>
                    <button className="mini" title="Copy path" onClick={(e) => { e.stopPropagation(); void onRowAction(f.path, "copy"); }}>Copy</button>
                  </td>
                </tr>
              ))}
              {query.trim() && contentResults.length === 0 && (
                <tr>
                  <td colSpan={5} className="empty">
                    No content matches
                  </td>
                </tr>
              )}
              {!query.trim() && (
                <tr>
                  <td colSpan={5} className="empty">
                    Type to search file contents
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        ) : (
          <table>
            <thead>
              <tr>
                <th>NAME</th>
                <th>PATH</th>
                <th>SIZE</th>
                <th>CONTENT</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {results.map((f, i) => (
                <tr
                  key={f.id}
                  className={i === activeIdx ? "active-row" : ""}
                  data-result-index={i}
                  tabIndex={0}
                  aria-selected={i === activeIdx}
                  onMouseEnter={() => setActiveIdx(i)}
                  onClick={() => {
                    setActiveIdx(i);
                    void onRowAction(f.path, "open");
                  }}
                  {...contextMenu.triggerProps}
                >
                  <td>
                    <span className="name">{f.name}</span>
                  </td>
                  <td className="mono dim">{f.path}</td>
                  <td className="mono">{fmtSize(f.size)}</td>
                  <td className="status-cell">{contentStatusLabel(f.content_status, f.content_truncated)}</td>
                  <td className="row-actions">
                    <button className="mini" title="Open" onClick={(e) => { e.stopPropagation(); void onRowAction(f.path, "open"); }}>Open</button>
                    <button className="mini" title="Show in folder" onClick={(e) => { e.stopPropagation(); void onRowAction(f.path, "folder"); }}>Folder</button>
                    <button className="mini" title="Copy path" onClick={(e) => { e.stopPropagation(); void onRowAction(f.path, "copy"); }}>Copy</button>
                  </td>
                </tr>
              ))}
              {query.trim() && results.length === 0 && (
                <tr>
                  <td colSpan={5} className="empty">
                    No results
                  </td>
                </tr>
              )}
              {!query.trim() && (
                <tr>
                  <td colSpan={5} className="empty">
                    Type to search
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        )}
      </div>
      <ContextMenu
        open={contextMenu.open}
        anchor={contextMenu.anchor}
        restoreFocusTo={contextMenu.restoreFocusTo}
        items={contextMenuItems}
        onSelect={onContextMenuSelect}
        onClose={contextMenu.close}
        ariaLabel="Search result actions"
      />
    </div>
  );
}
