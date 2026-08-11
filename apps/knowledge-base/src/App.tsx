import { useCallback, useEffect, useState } from "react";
import {
  createFile,
  dailyNote,
  deleteFile,
  listTags,
  listTree,
  readFile,
  renameFile,
  searchDocs,
  writeFile,
} from "./api";
import type { SearchResult, TreeEntry } from "./types";
import "./App.css";

function indent(path: string): number {
  return path.split("/").length - 1;
}

export default function App() {
  const [tree, setTree] = useState<TreeEntry[]>([]);
  const [tags, setTags] = useState<string[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [content, setContent] = useState("");
  const [dirty, setDirty] = useState(false);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [error, setError] = useState<string | null>(null);

  const loadMeta = useCallback(async () => {
    try {
      const [t, ts] = await Promise.all([listTree(), listTags()]);
      setTree(t);
      setTags(ts);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void loadMeta();
  }, [loadMeta]);

  const openFile = async (path: string) => {
    if (dirty && !confirm("저장하지 않은 변경사항이 있습니다. 계속할까요?")) return;
    setError(null);
    try {
      const text = await readFile(path);
      setSelected(path);
      setContent(text);
      setDirty(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
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

  const runSearch = async () => {
    if (!query.trim()) {
      setResults([]);
      return;
    }
    setError(null);
    try {
      setResults(await searchDocs(query.trim()));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const openDaily = async () => {
    setError(null);
    try {
      const [rel, text] = await dailyNote();
      setSelected(rel);
      setContent(text);
      setDirty(false);
      await loadMeta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const newFile = async () => {
    const name = prompt("새 파일 이름 (예: Notes/idea.md)");
    if (!name) return;
    setError(null);
    try {
      await createFile(name, "---\ntitle: \n---\n\n");
      await loadMeta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const rename = async (path: string) => {
    const name = prompt("새 이름", path);
    if (!name || name === path) return;
    setError(null);
    try {
      await renameFile(path, name);
      if (selected === path) setSelected(name);
      await loadMeta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const remove = async (path: string) => {
    if (!confirm(`'${path}' 삭제?`)) return;
    setError(null);
    try {
      await deleteFile(path);
      if (selected === path) {
        setSelected(null);
        setContent("");
      }
      await loadMeta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="app">
      {error && <div className="error">{error}</div>}
      <aside className="sidebar">
        <h1 className="app-title">Knowledge</h1>
        <div className="sidebar-row">
          <button className="btn small" onClick={() => void openDaily()}>
            Daily note
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
                    className={`tree-node ${selected === t.path ? "active" : ""}`}
                    style={{ paddingLeft: `${8 + indent(t.path) * 14}px` }}
                    onClick={() => (t.is_dir ? undefined : void openFile(t.path))}
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
              <span className="spacer" />
              {dirty && <span className="dirty">● unsaved</span>}
              <button className="btn" onClick={() => void save()}>
                Save
              </button>
            </div>
            <textarea
              className="editor"
              value={content}
              onChange={(e) => {
                setContent(e.currentTarget.value);
                setDirty(true);
              }}
              onKeyDown={(e) => {
                if ((e.ctrlKey || e.metaKey) && e.key === "s") {
                  e.preventDefault();
                  void save();
                }
              }}
              spellCheck={false}
            />
          </>
        ) : (
          <div className="empty">Select a note or create a daily note</div>
        )}
      </main>
    </div>
  );
}
