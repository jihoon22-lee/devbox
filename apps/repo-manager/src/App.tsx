import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  createWorktree,
  onOpenRequest,
  openIn,
  openRepositoryFolder,
  openTargets,
  prepareInboundRepository,
  repositoryCopyPath,
  repoStatus,
  scanRoot,
  takePendingOpen,
  worktrees,
  type OpenRequest,
  type RepoEntry,
  type RepoOpenTarget,
  type RepoSnapshot,
} from "./api";
import { routeOpenRequest, sameRepositoryKey } from "./lib/applink";
import { buildRepositoryContextMenu } from "./lib/contextMenu";
import GitSafetyPanel from "./components/GitSafetyPanel";
import HistoryDiffPanel from "./components/HistoryDiffPanel";
import RemoteSyncPanel from "./components/RemoteSyncPanel";
import StageCommitPanel from "./components/StageCommitPanel";
import CleanupPanel from "./components/CleanupPanel";
import "./App.css";

function usesNativeTextContext(target: EventTarget | null): boolean {
  return target instanceof HTMLElement
    && target.closest("button, a, input, select, textarea, [contenteditable='true']") !== null;
}

const SAFE_APP_ERRORS = new Set([
  "지원하지 않는 열기 요청입니다",
  "요청한 repository 경로를 사용할 수 없습니다",
  "repository 경로를 확인할 수 없습니다",
]);

/** Keep legacy/native details out of the top-level App error banner. */
export function safeRepoManagerError(cause: unknown, fallback: string): string {
  const raw = cause instanceof Error ? cause.message : typeof cause === "string" ? cause : "";
  const message = raw.replace(/^Error:\s*/u, "");
  return SAFE_APP_ERRORS.has(message) ? message : fallback;
}

export default function App() {
  const [root, setRoot] = useState("C:\\projects");
  const [repos, setRepos] = useState<RepoEntry[]>([]);
  const [truncated, setTruncated] = useState(false);
  const [status, setStatus] = useState<Record<string, RepoSnapshot>>({});
  const [wt, setWt] = useState<Record<string, string[]>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [newBranch, setNewBranch] = useState("");
  const [newDir, setNewDir] = useState("");
  const [targets, setTargets] = useState<RepoOpenTarget[] | null>(null);
  const [reposLoaded, setReposLoaded] = useState(false);
  const [selectedRepoKey, setSelectedRepoKey] = useState<string | null>(null);
  const [registrationDraft, setRegistrationDraft] = useState<RepoEntry | null>(null);
  const [contextRepo, setContextRepo] = useState<RepoEntry | null>(null);
  const reposRef = useRef<RepoEntry[]>(repos);
  const pendingSelectionKeyRef = useRef<string | null>(null);
  const selectedCardRef = useRef<HTMLDivElement | null>(null);
  const openSequenceRef = useRef(0);
  const scanSequenceRef = useRef(0);
  const mountedRef = useRef(false);
  const branchInputRefs = useRef(new Map<string, HTMLInputElement>());
  reposRef.current = repos;

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      scanSequenceRef.current += 1;
    };
  }, []);

  const prepareRepositoryContext = useCallback((target: HTMLElement) => {
    const key = target.dataset.repoKey;
    const repo = repos.find((candidate) => sameRepositoryKey(candidate.canonicalKey, key ?? ""));
    if (!repo) return;
    setSelectedRepoKey(repo.canonicalKey);
    setRegistrationDraft(null);
    setContextRepo(repo);
  }, [repos]);
  const repositoryContextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareRepositoryContext(target),
  });

  const scan = useCallback(async () => {
    const sequence = ++scanSequenceRef.current;
    const requestedRoot = root;
    const isCurrentScan = () => mountedRef.current && sequence === scanSequenceRef.current;
    setError(null);
    try {
      const { repos: list, truncated: wasTruncated } = await scanRoot(requestedRoot);
      if (!isCurrentScan()) return;
      reposRef.current = list;
      setRepos(list);
      setTruncated(wasTruncated);
      const pendingKey = pendingSelectionKeyRef.current;
      if (pendingKey) {
        const match = list.find((repo) => sameRepositoryKey(repo.canonicalKey, pendingKey));
        if (match) {
          setSelectedRepoKey(match.canonicalKey);
          setRegistrationDraft(null);
        }
        pendingSelectionKeyRef.current = null;
      } else {
        setSelectedRepoKey((current) =>
          current && list.some((repo) => sameRepositoryKey(repo.canonicalKey, current))
            ? current
            : null,
        );
      }
      setRegistrationDraft((current) =>
        current && list.some((repo) => sameRepositoryKey(repo.canonicalKey, current.canonicalKey))
          ? null
          : current,
      );
      // 목록이 확보되면 inbound listener/선택을 먼저 활성화한다. 각 repository의
      // Git status와 worktree 조회는 그 뒤에 이어져도 card 선택을 막지 않는다.
      setReposLoaded(true);

      const st: Record<string, RepoSnapshot> = {};
      const ws: Record<string, string[]> = {};
      for (const r of list) {
        st[r.path] = await repoStatus(r.path).catch(() => st[r.path] ?? { path: r.path, branch: { current: "?", ahead: 0, behind: 0, dirty: false, detached: false }, changes: 0 });
        if (!isCurrentScan()) return;
        ws[r.path] = await worktrees(r.path).catch(() => []);
        if (!isCurrentScan()) return;
      }
      setStatus(st);
      setWt(ws);
    } catch (e) {
      if (!isCurrentScan()) return;
      pendingSelectionKeyRef.current = null;
      setError(safeRepoManagerError(e, "repository 목록을 불러오지 못했습니다."));
    } finally {
      if (isCurrentScan()) setReposLoaded(true);
    }
  }, [root]);

  useEffect(() => {
    void scan();
  }, [scan]);

  useEffect(() => {
    void openTargets()
      .then(setTargets)
      .catch(() => {
        setTargets([]);
        setError("다른 앱으로 열기 대상을 확인하지 못했습니다");
      });
  }, []);

  const handleOpenRequest = async (request: OpenRequest) => {
    const sequence = ++openSequenceRef.current;
    pendingSelectionKeyRef.current = null;
    const action = routeOpenRequest(request);
    if (action.kind === "error") {
      setError(safeRepoManagerError(action.message, "repository 열기 요청을 처리하지 못했습니다"));
      return;
    }

    try {
      const inbound = await prepareInboundRepository(action.path);
      if (sequence !== openSequenceRef.current) return;
      const match = reposRef.current.find((repo) =>
        sameRepositoryKey(repo.canonicalKey, inbound.canonicalKey),
      );
      setError(null);
      if (match) {
        setRegistrationDraft(null);
        setSelectedRepoKey(match.canonicalKey);
      } else {
        setSelectedRepoKey(null);
        setRegistrationDraft(inbound);
      }
    } catch (cause) {
      if (sequence === openSequenceRef.current) {
        setError(safeRepoManagerError(cause, "repository 경로를 확인할 수 없습니다"));
      }
    }
  };
  const handleOpenRequestRef = useRef(handleOpenRequest);
  handleOpenRequestRef.current = handleOpenRequest;

  // 초기 목록을 준비한 뒤 listener를 먼저 등록하고 cold request를 pull한다.
  // Hot event payload는 trigger로만 쓰고 authoritative request는 pending slot에서 take한다.
  useEffect(() => {
    if (!reposLoaded) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;

    const consumePendingOpen = () => {
      void takePendingOpen()
        .then((request) => {
          if (!disposed && request) void handleOpenRequestRef.current(request);
        })
        .catch(() => {
          if (!disposed) setError("repository 열기 요청을 처리하지 못했습니다");
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
  }, [reposLoaded]);

  useEffect(() => {
    if (!selectedRepoKey || repositoryContextMenu.open) return;
    selectedCardRef.current?.focus();
    selectedCardRef.current?.scrollIntoView?.({ block: "nearest" });
  }, [repositoryContextMenu.open, selectedRepoKey]);

  useEffect(() => {
    const key = contextRepo?.canonicalKey;
    if (!key) return;
    const current = repos.find((repo) => sameRepositoryKey(repo.canonicalKey, key)) ?? null;
    if (current) setContextRepo(current);
    else {
      repositoryContextMenu.close();
      setContextRepo(null);
      setSelectedRepoKey((selected) => selected && sameRepositoryKey(selected, key) ? null : selected);
    }
  }, [contextRepo?.canonicalKey, repos, repositoryContextMenu.close]);

  const exploreDraft = () => {
    if (!registrationDraft) return;
    pendingSelectionKeyRef.current = registrationDraft.canonicalKey;
    if (registrationDraft.path === root) void scan();
    else setRoot(registrationDraft.path);
    setRegistrationDraft(null);
  };

  const onOpen = async (target: RepoOpenTarget, path: string) => {
    setError(null);
    try {
      await openIn(target.id, path);
    } catch (e) {
      setError(safeRepoManagerError(e, "다른 앱으로 repository를 열 수 없습니다"));
    }
  };

  const onCopyRepositoryPath = async (repo: RepoEntry) => {
    setError(null);
    try {
      const path = await repositoryCopyPath(repo.path);
      await navigator.clipboard.writeText(path);
    } catch {
      setError("repository 경로를 확인하거나 복사하지 못했습니다");
    }
  };

  const onOpenRepositoryFolder = async (repo: RepoEntry) => {
    setError(null);
    try {
      await openRepositoryFolder(repo.path);
    } catch {
      setError("repository 폴더를 열 수 없습니다");
    }
  };

  const focusWorktreeCreate = (repo: RepoEntry) => {
    setSelectedRepoKey(repo.canonicalKey);
    window.requestAnimationFrame(() => {
      branchInputRefs.current.get(repo.canonicalKey)?.focus({ preventScroll: true });
    });
  };

  const onCreate = async (repo: string) => {
    if (!newBranch.trim() || !newDir.trim()) return;
    setBusy(true);
    setError(null);
    try {
      await createWorktree(repo, newBranch.trim(), newDir.trim());
      setNewBranch("");
      setNewDir("");
      await scan();
    } catch (e) {
      setError(safeRepoManagerError(e, "worktree를 생성하지 못했습니다"));
    } finally {
      setBusy(false);
    }
  };

  const repositoryContextItems = useMemo<readonly ContextMenuEntry[]>(
    () => buildRepositoryContextMenu(targets, busy),
    [busy, targets],
  );

  const onRepositoryContextSelect = (id: string) => {
    const repo = contextRepo;
    if (!repo) return;
    if (id === "create-worktree") focusWorktreeCreate(repo);
    else if (id === "copy-path") void onCopyRepositoryPath(repo);
    else if (id === "open-folder") void onOpenRepositoryFolder(repo);
    else {
      const target = targets?.find((candidate) => `open-in:${candidate.id}` === id);
      if (target) void onOpen(target, repo.path);
    }
  };

  const selectedRepo = repos.find((repo) =>
    sameRepositoryKey(repo.canonicalKey, selectedRepoKey ?? ""),
  ) ?? null;

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">Repo Manager</h1>
        <label className="sr-only" htmlFor="repo-scan-root">탐색 root</label>
        <input
          id="repo-scan-root"
          className="root-input"
          value={root}
          onChange={(e) => setRoot(e.currentTarget.value)}
          placeholder="탐색 root"
        />
        <button className="btn primary" onClick={() => void scan()}>탐색</button>
      </header>
      {error && <div className="error" role="alert" aria-live="assertive">{error}</div>}
      {truncated && (
        <div className="note dim">
          탐색 범위가 커서 일부 디렉터리를 건너뛰었습니다 (깊이·개수 상한). root를 더 좁혀서 다시 탐색하세요.
        </div>
      )}

      {registrationDraft && (
        <section className="registration-draft" aria-label="Repository 등록 초안">
          <div>
            <strong>Repository 등록 초안</strong>
            <span className="draft-path">{registrationDraft.path}</span>
          </div>
          <span className="draft-help">현재 목록에는 없습니다. 아직 저장하거나 Git 명령을 실행하지 않았습니다.</span>
          <button className="btn primary" onClick={exploreDraft}>이 경로 탐색</button>
          <button className="btn" onClick={() => setRegistrationDraft(null)}>취소</button>
        </section>
      )}

      <div className="repos">
        {repos.map((r, index) => {
          const s = status[r.path];
          const branchInputId = `worktree-branch-${index}`;
          const directoryInputId = `worktree-directory-${index}`;
          const isSelected = sameRepositoryKey(r.canonicalKey, selectedRepoKey ?? "");
          return (
            <div
              key={r.path}
              ref={isSelected ? selectedCardRef : undefined}
              className={`repo-card ${isSelected ? "selected" : ""}`}
              role="group"
              tabIndex={0}
              aria-current={isSelected ? "true" : undefined}
              aria-label={`${r.path} repository`}
              data-repo-key={r.canonicalKey}
              onClick={(event) => {
                if (!usesNativeTextContext(event.target)) {
                  setSelectedRepoKey(r.canonicalKey);
                  setRegistrationDraft(null);
                }
              }}
              {...repositoryContextMenu.triggerProps}
              onContextMenu={(event) => {
                if (!usesNativeTextContext(event.target)) {
                  repositoryContextMenu.triggerProps.onContextMenu?.(event);
                }
              }}
              onKeyDown={(event) => {
                if (!usesNativeTextContext(event.target)) {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    setSelectedRepoKey(r.canonicalKey);
                    setRegistrationDraft(null);
                    return;
                  }
                  repositoryContextMenu.triggerProps.onKeyDown?.(event);
                }
              }}
            >
              <div className="repo-head">
                <span className="repo-path">{r.path}</span>
                <span className={`branch ${s?.branch.dirty ? "dirty" : ""}`}>
                  {s?.branch.detached ? "(detached)" : s?.branch.current}
                  {s?.branch.ahead ? ` ↑${s.branch.ahead}` : ""}
                  {s?.branch.behind ? ` ↓${s.branch.behind}` : ""}
                  {s?.changes ? ` +${s.changes}` : ""}
                </span>
                <div className="open-targets" aria-label="다른 앱으로 열기">
                  {targets?.map((target) => (
                    <button
                      key={target.id}
                      className="mini"
                      title={`${target.displayName}에서 ${target.payloadKind === "workspace" ? "workspace" : "path"}로 열기`}
                      onClick={() => void onOpen(target, r.path)}
                    >
                      {target.displayName}
                    </button>
                  ))}
                  {targets === null && <span className="open-targets-empty">대상 확인 중…</span>}
                  {targets?.length === 0 && <span className="open-targets-empty">설치된 대상 앱 없음</span>}
                </div>
              </div>
              {wt[r.path] && wt[r.path].length > 1 && (
                <div className="worktrees">
                  {wt[r.path].map((w) => (
                    <div key={w} className="wt-row">
                      <span className="mono">{w}</span>
                    </div>
                  ))}
                </div>
              )}
              <div className="wt-create">
                <label className="sr-only" htmlFor={branchInputId}>새 브랜치</label>
                <input
                  id={branchInputId}
                  ref={(node) => {
                    if (node) branchInputRefs.current.set(r.canonicalKey, node);
                    else branchInputRefs.current.delete(r.canonicalKey);
                  }}
                  placeholder="새 브랜치"
                  value={newBranch}
                  onChange={(e) => setNewBranch(e.currentTarget.value)}
                />
                <label className="sr-only" htmlFor={directoryInputId}>대상 디렉터리</label>
                <input
                  id={directoryInputId}
                  placeholder="대상 디렉터리"
                  value={newDir}
                  onChange={(e) => setNewDir(e.currentTarget.value)}
                />
                <button className="btn" disabled={busy} onClick={() => void onCreate(r.path)}>worktree 생성</button>
              </div>
            </div>
          );
        })}
        {repos.length === 0 && <div className="dim">repository가 없습니다.</div>}
      </div>
      <HistoryDiffPanel repo={selectedRepo} />
      <StageCommitPanel repo={selectedRepo} />
      <GitSafetyPanel repo={selectedRepo} />
      <RemoteSyncPanel repo={selectedRepo} />
      <CleanupPanel repo={selectedRepo} />
      <div className="note dim">force delete·reset·clean은 제공하지 않습니다. 정리는 preview와 안전 차단을 통과한 명시적 선택만 실행합니다.</div>
      <ContextMenu
        open={repositoryContextMenu.open}
        anchor={repositoryContextMenu.anchor}
        restoreFocusTo={repositoryContextMenu.restoreFocusTo}
        items={repositoryContextItems}
        onSelect={onRepositoryContextSelect}
        onClose={repositoryContextMenu.close}
        ariaLabel="Repository 메뉴"
      />
    </div>
  );
}
