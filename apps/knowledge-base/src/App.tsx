import { useCallback, useEffect, useRef, useState } from "react";
import {
  createFile,
  dailyNote,
  deleteFile,
  listTags,
  listTree,
  onDocsChanged,
  readFile,
  renameFile,
  renderMarkdown,
  searchDocs,
  writeFile,
} from "./api";
import MarkdownEditor from "./components/MarkdownEditor";
import MarkdownPreview from "./components/MarkdownPreview";
import type { RenderedDoc, SearchResult, TreeEntry } from "./types";
import "./App.css";

type ViewMode = "edit" | "split" | "preview";

const RENDER_DEBOUNCE_MS = 300;

function indent(path: string): number {
  return path.split("/").length - 1;
}

function isMarkdown(path: string | null): boolean {
  return !!path && path.endsWith(".md");
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
  const [mode, setMode] = useState<ViewMode>("edit");
  const [rendered, setRendered] = useState<RenderedDoc | null>(null);

  // 인플라이트 렌더 응답이 도착했을 때 "그사이 다른 문서로 전환했는지"를 판단하기 위해
  // 최신 선택값을 ref로도 들고 있는다(설계 결정 3 — 응답 시점에 최신값과 비교).
  const selectedRef = useRef<string | null>(selected);
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
          if (selectedRef.current !== rel) return;
          setRendered(doc);
        })
        .catch((e) => {
          if (selectedRef.current !== rel) return;
          setError(e instanceof Error ? e.message : String(e));
        });
    }, RENDER_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [content, selected, mode]);

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

  // 외부 편집 watcher가 docs-changed를 보내면 트리·태그를 새로고침한다
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void onDocsChanged(() => void loadMeta()).then((u) => {
      unlisten = u;
    });
    return () => unlisten?.();
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
              {dirty && <span className="dirty">● unsaved</span>}
              <button className="btn" onClick={() => void save()}>
                Save
              </button>
            </div>
            <div className={`editor-body mode-${mode}`}>
              {mode !== "preview" && (
                <MarkdownEditor
                  value={content}
                  onChange={(text) => {
                    setContent(text);
                    setDirty(true);
                  }}
                  onSave={() => void save()}
                />
              )}
              {mode !== "edit" && (
                <MarkdownPreview doc={rendered} baseRel={selected} onNavigate={(rel) => void openFile(rel)} />
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
