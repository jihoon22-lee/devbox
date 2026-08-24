import { useCallback, useEffect, useRef, useState } from "react";
import {
  createWorktree,
  onOpenRequest,
  openIn,
  openTargets,
  prepareInboundRepository,
  repoStatus,
  scanRoot,
  takePendingOpen,
  worktreeClean,
  worktrees,
  type OpenRequest,
  type RepoEntry,
  type RepoOpenTarget,
  type RepoSnapshot,
} from "./api";
import { routeOpenRequest, sameRepositoryKey } from "./lib/applink";
import "./App.css";

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
  const reposRef = useRef<RepoEntry[]>(repos);
  const pendingSelectionKeyRef = useRef<string | null>(null);
  const selectedCardRef = useRef<HTMLDivElement | null>(null);
  const openSequenceRef = useRef(0);
  reposRef.current = repos;

  const scan = useCallback(async () => {
    setError(null);
    try {
      const { repos: list, truncated: wasTruncated } = await scanRoot(root);
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
        ws[r.path] = await worktrees(r.path).catch(() => []);
      }
      setStatus(st);
      setWt(ws);
    } catch (e) {
      pendingSelectionKeyRef.current = null;
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setReposLoaded(true);
    }
  }, [root]);

  useEffect(() => {
    void scan();
  }, [scan]);

  useEffect(() => {
    void openTargets()
      .then(setTargets)
      .catch((e: unknown) => {
        setTargets([]);
        setError(e instanceof Error ? e.message : String(e));
      });
  }, []);

  const handleOpenRequest = async (request: OpenRequest) => {
    const sequence = ++openSequenceRef.current;
    pendingSelectionKeyRef.current = null;
    const action = routeOpenRequest(request);
    if (action.kind === "error") {
      setError(action.message);
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
    } catch {
      if (sequence === openSequenceRef.current) {
        setError("repository 경로를 확인할 수 없습니다");
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
    if (!selectedRepoKey) return;
    selectedCardRef.current?.focus();
    selectedCardRef.current?.scrollIntoView?.({ block: "nearest" });
  }, [selectedRepoKey]);

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
      setError(e instanceof Error ? e.message : String(e));
    }
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
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const onRemoveCheck = async (_repo: string, wtPath: string) => {
    setError(null);
    try {
      const clean = await worktreeClean(wtPath);
      if (clean) {
        setError(`worktree ${wtPath}는 clean — 제거 가능 (동작 미구현: remove는 신중히).`);
      } else {
        setError(`worktree ${wtPath}에 uncommitted/untracked 변경이 있습니다. 제거 전 정리하세요.`);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">Repo Manager</h1>
        <input className="root-input" value={root} onChange={(e) => setRoot(e.currentTarget.value)} placeholder="탐색 root" />
        <button className="btn primary" onClick={() => void scan()}>탐색</button>
      </header>
      {error && <div className="error">{error}</div>}
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
        {repos.map((r) => {
          const s = status[r.path];
          return (
            <div
              key={r.path}
              ref={sameRepositoryKey(r.canonicalKey, selectedRepoKey ?? "") ? selectedCardRef : undefined}
              className={`repo-card ${sameRepositoryKey(r.canonicalKey, selectedRepoKey ?? "") ? "selected" : ""}`}
              tabIndex={-1}
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
                      <button className="mini" onClick={() => void onRemoveCheck(r.path, w)}>remove 확인</button>
                    </div>
                  ))}
                </div>
              )}
              <div className="wt-create">
                <input placeholder="새 브랜치" value={newBranch} onChange={(e) => setNewBranch(e.currentTarget.value)} />
                <input placeholder="대상 디렉터리" value={newDir} onChange={(e) => setNewDir(e.currentTarget.value)} />
                <button className="btn" disabled={busy} onClick={() => void onCreate(r.path)}>worktree 생성</button>
              </div>
            </div>
          );
        })}
        {repos.length === 0 && <div className="dim">repository가 없습니다.</div>}
      </div>
      <div className="note dim">force delete·reset·clean은 기본 동작으로 제공하지 않습니다. remove 전 검사만 지원합니다.</div>
    </div>
  );
}
