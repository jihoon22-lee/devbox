import { useCallback, useEffect, useRef, useState } from "react";
import { addRoot, indexNow, indexStatus, listRoots, removeRoot, searchFiles } from "./api";
import type { FileEntry, IndexStatus } from "./types";
import "./App.css";

function fmtSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export default function App() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<FileEntry[]>([]);
  const [status, setStatus] = useState<IndexStatus>({ indexing: false, total_files: 0, roots: 0, last_indexed_at: null });
  const [roots, setRoots] = useState<string[]>([]);
  const [newRoot, setNewRoot] = useState("");
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);

  const loadMeta = useCallback(async () => {
    try {
      const [st, rs] = await Promise.all([indexStatus(), listRoots()]);
      setStatus(st);
      setRoots(rs);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void loadMeta();
  }, [loadMeta]);

  useEffect(() => {
    const current = ++seq.current;
    const q = query.trim();
    if (!q) {
      setResults([]);
      return;
    }
    let cancelled = false;
    const t = setTimeout(async () => {
      try {
        const r = await searchFiles(q);
        if (!cancelled && seq.current === current) setResults(r);
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      }
    }, 150);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [query]);

  const onAddRoot = async () => {
    if (!newRoot.trim()) return;
    setError(null);
    try {
      await addRoot(newRoot.trim());
      setNewRoot("");
      await loadMeta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onReindex = async () => {
    setError(null);
    try {
      await indexNow();
      setTimeout(() => void loadMeta(), 800);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">Everything+</h1>
        <input
          className="search"
          placeholder="Search file names..."
          value={query}
          onChange={(e) => setQuery(e.currentTarget.value)}
          autoFocus
        />
        <span className="status">
          {status.indexing ? "Indexing..." : `${status.total_files.toLocaleString()} files`}
        </span>
        <button className="btn" onClick={() => void onReindex()}>
          Re-index
        </button>
      </header>

      {error && <div className="error">{error}</div>}

      <div className="roots">
        <span className="dim">Roots:</span>
        {roots.map((r) => (
          <span key={r} className="root-chip">
            {r}
            <button className="root-del" title="Remove root" onClick={() => void removeRoot(r).then(loadMeta)}>
              ✕
            </button>
          </span>
        ))}
        <input
          className="root-input"
          placeholder="Add root path (e.g. C:\projects)"
          value={newRoot}
          onChange={(e) => setNewRoot(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void onAddRoot();
          }}
        />
        <button className="btn" onClick={() => void onAddRoot()}>
          Add
        </button>
      </div>

      <div className="table-wrap">
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
      </div>
    </div>
  );
}
