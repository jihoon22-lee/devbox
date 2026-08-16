import { useCallback, useEffect, useState } from "react";
import {
  createWorktree,
  openIn,
  repoStatus,
  scanRoot,
  worktreeClean,
  worktrees,
  type RepoEntry,
  type RepoSnapshot,
} from "./api";
import "./App.css";

export default function App() {
  const [root, setRoot] = useState("C:\\projects");
  const [repos, setRepos] = useState<RepoEntry[]>([]);
  const [status, setStatus] = useState<Record<string, RepoSnapshot>>({});
  const [wt, setWt] = useState<Record<string, string[]>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [newBranch, setNewBranch] = useState("");
  const [newDir, setNewDir] = useState("");

  const scan = useCallback(async () => {
    setError(null);
    try {
      const list = await scanRoot(root);
      setRepos(list);
      const st: Record<string, RepoSnapshot> = {};
      const ws: Record<string, string[]> = {};
      for (const r of list) {
        st[r.path] = await repoStatus(r.path).catch(() => st[r.path] ?? { path: r.path, branch: { current: "?", ahead: 0, behind: 0, dirty: false, detached: false }, changes: 0 });
        ws[r.path] = await worktrees(r.path).catch(() => []);
      }
      setStatus(st);
      setWt(ws);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [root]);

  useEffect(() => {
    void scan();
  }, [scan]);

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

      <div className="repos">
        {repos.map((r) => {
          const s = status[r.path];
          return (
            <div key={r.path} className="repo-card">
              <div className="repo-head">
                <span className="repo-path">{r.path}</span>
                <span className={`branch ${s?.branch.dirty ? "dirty" : ""}`}>
                  {s?.branch.detached ? "(detached)" : s?.branch.current}
                  {s?.branch.ahead ? ` ↑${s.branch.ahead}` : ""}
                  {s?.branch.behind ? ` ↓${s.branch.behind}` : ""}
                  {s?.changes ? ` +${s.changes}` : ""}
                </span>
                <button className="mini" onClick={() => void openIn("code-pad", r.path)}>CodePad</button>
                <button className="mini" onClick={() => void openIn("wsl-desktop", r.path)}>WSLDesktop</button>
                <button className="mini" onClick={() => void openIn("workbench", r.path)}>Workbench</button>
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
