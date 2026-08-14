import { useCallback, useEffect, useRef, useState } from "react";
import { addRoot, indexNow, indexStatus, listRoots, removeRoot, searchContent, searchFiles, watcherStatuses } from "./api";
import type { ContentResult, FileEntry, IndexStatus, RootInfo, RootStatus } from "./types";
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

  useEffect(() => {
    const current = ++seq.current;
    const q = query.trim();
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
          setContentResults(await searchContent(q));
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
          setResults(await searchFiles(q));
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

      <div className="table-wrap">
        {mode === "content" ? (
          <table>
            <thead>
              <tr>
                <th>NAME</th>
                <th>SNIPPET</th>
                <th>PATH</th>
              </tr>
            </thead>
            <tbody>
              {contentResults.map((f, i) => (
                <tr key={`${f.path}-${i}`}>
                  <td>
                    <span className="name">{f.name}</span>
                  </td>
                  <td className="snippet">{f.snippet}</td>
                  <td className="mono dim">{f.path}</td>
                </tr>
              ))}
              {query.trim() && contentResults.length === 0 && (
                <tr>
                  <td colSpan={3} className="empty">
                    No content matches for "{query}"
                  </td>
                </tr>
              )}
              {!query.trim() && (
                <tr>
                  <td colSpan={3} className="empty">
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
              </tr>
            </thead>
            <tbody>
              {results.map((f) => (
                <tr key={f.id}>
                  <td>
                    <span className="name">{f.name}</span>
                  </td>
                  <td className="mono dim">{f.path}</td>
                  <td className="mono">{fmtSize(f.size)}</td>
                </tr>
              ))}
              {query.trim() && results.length === 0 && (
                <tr>
                  <td colSpan={3} className="empty">
                    No results for "{query}"
                  </td>
                </tr>
              )}
              {!query.trim() && (
                <tr>
                  <td colSpan={3} className="empty">
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
