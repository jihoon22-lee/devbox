import { useCallback, useEffect, useRef, useState } from "react";
import { addRoot, copyPath, indexNow, indexStatus, listRoots, onOpenRequest, openFile, removeRoot, revealFile, searchContent, searchFiles, takePendingOpen, watcherStatuses, type OpenRequest } from "./api";
import type { ContentResult, FileEntry, IndexStatus, RootInfo, RootStatus } from "./types";
import { routeOpenRequest } from "./lib/applink";
import "./App.css";

function fmtSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export default function App() {
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<"name" | "content">("name");
  const [regexMode, setRegexMode] = useState(false);
  const [results, setResults] = useState<FileEntry[]>([]);
  const [contentResults, setContentResults] = useState<ContentResult[]>([]);
  const [status, setStatus] = useState<IndexStatus>({ indexing: false, total_files: 0, indexed_files: 0, roots: 0, last_indexed_at: null });
  const [roots, setRoots] = useState<RootInfo[]>([]);
  const [watchStatus, setWatchStatus] = useState<RootStatus[]>([]);
  const [newRoot, setNewRoot] = useState("");
  const [newRootContent, setNewRootContent] = useState(false);
  const [regexError, setRegexError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [activeIdx, setActiveIdx] = useState(-1);
  const seq = useRef(0);

  const loadMeta = useCallback(async () => {
    try {
      const [st, rs, ws] = await Promise.all([indexStatus(), listRoots(), watcherStatuses()]);
      setStatus(st);
      setRoots(rs);
      setWatchStatus(ws);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
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
  };
  const handleOpenRequestRef = useRef(handleOpenRequest);
  handleOpenRequestRef.current = handleOpenRequest;

  // Event listener를 먼저 준비한 다음 cold request를 pull한다. Hot event도
  // payload를 직접 적용하지 않고 같은 one-shot pending slot을 소비한다.
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    const consumePendingOpen = () => {
      void takePendingOpen()
        .then((request) => {
          if (!disposed && request) handleOpenRequestRef.current(request);
        })
        .catch(() => {
          if (!disposed) setError("검색 요청을 처리하지 못했습니다");
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
    if (!q) {
      setResults([]);
      setContentResults([]);
      setRegexError(null);
      return;
    }
    let cancelled = false;
    const t = setTimeout(async () => {
      try {
        if (mode === "content") {
          const next = await searchContent(q);
          if (!cancelled && seq.current === current) setContentResults(next);
        } else if (regexMode) {
          let re: RegExp;
          try {
            re = new RegExp(q, "i");
          } catch (e) {
            setRegexError(e instanceof Error ? e.message : String(e));
            return;
          }
          setRegexError(null);
          const all = await searchFiles(q.replace(/[^a-zA-Z0-9\s]/g, ""), 2000);
          if (!cancelled && seq.current === current) setResults(all.filter((f) => re.test(f.name)));
        } else {
          setRegexError(null);
          const next = await searchFiles(q);
          if (!cancelled && seq.current === current) setResults(next);
        }
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      }
    }, 150);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [query, mode, regexMode]);

  const onAddRoot = async () => {
    if (!newRoot.trim()) return;
    setError(null);
    try {
      await addRoot(newRoot.trim(), newRootContent);
      setNewRoot("");
      setNewRootContent(false);
      await loadMeta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onReindex = async () => {
    setError(null);
    try {
      await indexNow();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const pct = status.total_files > 0 ? Math.round((status.indexed_files / status.total_files) * 100) : 0;

  const activeList = mode === "content" ? contentResults : results;
  const activePath = (i: number) => (mode === "content" ? contentResults[i]?.path : results[i]?.path) ?? null;

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

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">Everything+</h1>
        <div className="mode-tabs">
          <button className={`mode-tab ${mode === "name" ? "active" : ""}`} onClick={() => setMode("name")}>
            Name
          </button>
          <button className={`mode-tab ${mode === "content" ? "active" : ""}`} onClick={() => setMode("content")}>
            Content
          </button>
        </div>
        <input
          className="search"
          placeholder={mode === "content" ? "Search file contents..." : "Search file names..."}
          value={query}
          onChange={(e) => setQuery(e.currentTarget.value)}
          autoFocus
        />
        {mode === "name" && (
          <label className="regex-toggle">
            <input type="checkbox" checked={regexMode} onChange={(e) => setRegexMode(e.currentTarget.checked)} />
            regex
          </label>
        )}
        <span className="status">
          {status.indexing
            ? `Indexing... ${status.indexed_files.toLocaleString()} files`
            : `${status.total_files.toLocaleString()} files`}
        </span>
        <button className="btn" onClick={() => void onReindex()}>
          Re-index
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

      <div className="roots">
        <span className="dim">Roots:</span>
        {roots.map((r) => {
          const ws = watchStatus.find((w) => w.root === r.path);
          return (
            <span key={r.path} className="root-chip" title={r.content ? "content index on" : "name only"}>
              {r.path}
              {r.content && <span className="root-tag">content</span>}
              {ws && (
                <span className={`watch-state ${ws.error ? "watch-error" : ""}`} title={ws.error ?? "watcher"}>
                  {ws.error ? "!" : ws.pending > 0 ? `${ws.pending} pending` : "live"}
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
          placeholder="Add root path (e.g. C:\projects)"
          value={newRoot}
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
                <th />
              </tr>
            </thead>
            <tbody>
              {contentResults.map((f, i) => (
                <tr
                  key={`${f.path}-${i}`}
                  className={i === activeIdx ? "active-row" : ""}
                  onMouseEnter={() => setActiveIdx(i)}
                  onClick={() => void onOpenActive()}
                >
                  <td>
                    <span className="name">{f.name}</span>
                  </td>
                  <td className="snippet">{f.snippet}</td>
                  <td className="mono dim">{f.path}</td>
                  <td className="row-actions">
                    <button className="mini" title="Open" onClick={(e) => { e.stopPropagation(); void onRowAction(f.path, "open"); }}>Open</button>
                    <button className="mini" title="Show in folder" onClick={(e) => { e.stopPropagation(); void onRowAction(f.path, "folder"); }}>Folder</button>
                    <button className="mini" title="Copy path" onClick={(e) => { e.stopPropagation(); void onRowAction(f.path, "copy"); }}>Copy</button>
                  </td>
                </tr>
              ))}
              {query.trim() && contentResults.length === 0 && (
                <tr>
                  <td colSpan={4} className="empty">
                    No content matches for "{query}"
                  </td>
                </tr>
              )}
              {!query.trim() && (
                <tr>
                  <td colSpan={4} className="empty">
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
                <th />
              </tr>
            </thead>
            <tbody>
              {results.map((f, i) => (
                <tr
                  key={f.id}
                  className={i === activeIdx ? "active-row" : ""}
                  onMouseEnter={() => setActiveIdx(i)}
                  onClick={() => void onOpenActive()}
                >
                  <td>
                    <span className="name">{f.name}</span>
                  </td>
                  <td className="mono dim">{f.path}</td>
                  <td className="mono">{fmtSize(f.size)}</td>
                  <td className="row-actions">
                    <button className="mini" title="Open" onClick={(e) => { e.stopPropagation(); void onRowAction(f.path, "open"); }}>Open</button>
                    <button className="mini" title="Show in folder" onClick={(e) => { e.stopPropagation(); void onRowAction(f.path, "folder"); }}>Folder</button>
                    <button className="mini" title="Copy path" onClick={(e) => { e.stopPropagation(); void onRowAction(f.path, "copy"); }}>Copy</button>
                  </td>
                </tr>
              ))}
              {query.trim() && results.length === 0 && (
                <tr>
                  <td colSpan={4} className="empty">
                    No results for "{query}"
                  </td>
                </tr>
              )}
              {!query.trim() && (
                <tr>
                  <td colSpan={4} className="empty">
                    Type to search
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
