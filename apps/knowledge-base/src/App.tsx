import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import {
  createFile,
  createDirectory,
  dailyNote,
  deleteFile,
  entryPath,
  listTags,
  listTree,
  onDocsChanged,
  onOpenRequest,
  openInboundNote,
  openIn,
  openTargets,
  readFile,
  revealEntry,
  renameFile,
  renderMarkdown,
  searchDocs,
  takePendingOpen,
  writeFile,
  type OpenRequest,
  type KnowledgeOpenTarget,
} from "./api";
import MarkdownEditor from "./components/MarkdownEditor";
import MarkdownPreview from "./components/MarkdownPreview";
import type { RenderedDoc, SearchResult, TreeEntry } from "./types";
import { routeOpenRequest } from "./lib/applink";
import "./App.css";

type ViewMode = "edit" | "split" | "preview";
type TreeContextTarget = { path: string; isDir: boolean };

const RENDER_DEBOUNCE_MS = 300;

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
  const [mode, setMode] = useState<ViewMode>("edit");
  const [rendered, setRendered] = useState<RenderedDoc | null>(null);
  const [contextTarget, setContextTarget] = useState<TreeContextTarget | null>(null);
  const [availableTargets, setAvailableTargets] = useState<KnowledgeOpenTarget[] | null>(null);

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

  const openFile = async (path: string) => {
    if (dirty && !confirm("저장하지 않은 변경사항이 있습니다. 계속할까요?")) return;
    setError(null);
    try {
      const text = await readFile(path);
      setSelected(path);
      setSelectedTreePath(path);
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
        } catch {
          setError("요청한 노트를 열 수 없습니다");
        }
        break;
      case "search":
        setQuery(action.query);
        await runSearch(action.query);
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
    const name = prompt("새 이름", path);
    const normalized = name ? normalizeRelativePath(name) : "";
    if (!normalized || normalized === path) return;
    setError(null);
    try {
      await renameFile(path, normalized);
      setSelected((current) => remapPath(current, path, normalized));
      setSelectedTreePath((current) => remapPath(current, path, normalized));
      await loadMeta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
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
                  onError={setError}
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
