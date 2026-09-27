import { useCallback, useEffect, useMemo, useState } from "react";
import { repoStatus, repoConflicts, worktrees, type RepoEntry, type RepoSnapshot, type ConflictState } from "./api";
import BlamePanel, { type BlameTarget } from "./components/BlamePanel";
import ConflictPanel from "./components/ConflictPanel";
import PullRequestPanel from "./components/PullRequestPanel";
import BranchPanel from "./components/BranchPanel";
import StashPanel from "./components/StashPanel";
import GitSafetyPanel from "./components/GitSafetyPanel";
import HistoryDiffPanel from "./components/HistoryDiffPanel";
import StageCommitPanel from "./components/StageCommitPanel";
import RemoteSyncPanel from "./components/RemoteSyncPanel";
import CleanupPanel from "./components/CleanupPanel";
import { WorkspaceOperationError } from "../transport";
import "./App.css";

interface Props {
  repo: RepoEntry;
  cleanupRevision?: number;
  onBusyChange: (busy: boolean) => void;
  onDirtyChange: (dirty: boolean) => void;
  onOpenFile?: (path: string, line: number | null) => void;
  onProposeWorktree?: (path: string) => void;
}
/** Product composition consumes the selected native Registry projection. */
export default function SourcePanel({
  repo,
  cleanupRevision = 0,
  onBusyChange,
  onDirtyChange,
  onOpenFile,
  onProposeWorktree,
}: Props) {
  const repository = JSON.stringify([repo.canonicalKey, repo.path]);
  const [blame, setBlame] = useState<(BlameTarget & { repository: string }) | null>(null);
  const [focusCommit, setFocusCommit] = useState<{ id: string; sequence: number; repository: string } | null>(null);
  const showBlame = (file: string, commitId: string | null = null) =>
    setBlame((previous) => ({ file, commitId, repository, sequence: (previous?.sequence ?? 0) + 1 }));
  const [busy, setBusy] = useState(false);
  const [conflictSnapshot, setConflictSnapshot] = useState<{ repository: string; value: ConflictState } | null>(null);
  const conflicts = conflictSnapshot?.repository === repository ? conflictSnapshot.value : null;
  const [dirtyPanels, setDirtyPanels] = useState<Record<string, boolean>>({});
  const dirtyCallbacks = useMemo(
    () =>
      Object.fromEntries(
        ["stage", "conflicts", "pr"].map((name) => [
          name,
          (dirty: boolean) =>
            setDirtyPanels((previous) => (previous[name] === dirty ? previous : { ...previous, [name]: dirty })),
        ]),
      ),
    [],
  );
  const dirty = Object.values(dirtyPanels).some(Boolean);
  useEffect(() => {
    onDirtyChange(dirty);
  }, [dirty, onDirtyChange]);
  useEffect(() => () => onDirtyChange(false), [onDirtyChange]);
  useEffect(() => {
    let active = true;
    void repoConflicts(repo.path)
      .then((value) => {
        if (active) setConflictSnapshot({ repository, value });
      })
      .catch((cause) => {
        if (active) setError(cause instanceof WorkspaceOperationError ? cause.message : "충돌 상태를 읽지 못했습니다.");
      });
    return () => {
      active = false;
    };
  }, [repo.path, repository]);
  const [panels, setPanels] = useState<Record<string, boolean>>({});
  const [snapshot, setSnapshot] = useState<RepoSnapshot | null>(null);
  const [trees, setTrees] = useState<string[]>([]);
  const [error, setError] = useState("");
  const callbacks = useMemo(
    () =>
      Object.fromEntries(
        ["safety", "history", "stage", "remote", "cleanup", "branches", "stash", "blame", "conflicts", "pr"].map(
          (name) => [
            name,
            (value: boolean) =>
              setPanels((previous) => (previous[name] === value ? previous : { ...previous, [name]: value })),
          ],
        ),
      ),
    [],
  );
  useEffect(() => {
    onBusyChange(busy || Object.values(panels).some(Boolean));
  }, [busy, panels, onBusyChange]);
  useEffect(() => () => onBusyChange(false), [onBusyChange]);
  const refresh = useCallback(async () => {
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      const [next, paths, conflicts] = await Promise.all([
        repoStatus(repo.path),
        worktrees(repo.path),
        repoConflicts(repo.path),
      ]);
      setConflictSnapshot({ repository, value: conflicts });
      setSnapshot(next);
      setTrees(paths);
    } catch (cause) {
      setError(cause instanceof WorkspaceOperationError ? cause.message : "저장소 상태를 불러오지 못했습니다.");
    } finally {
      setBusy(false);
    }
  }, [busy, repo.path, repository]);
  async function refreshAfterResolution() {
    const value = await repoConflicts(repo.path);
    setConflictSnapshot({ repository, value });
  }
  return (
    <div className="workspace-native-source-panels">
      {conflicts && (conflicts.files.length > 0 || conflicts.operation) && (
        <ConflictPanel
          key={`conflicts:${repository}`}
          repo={repo}
          state={conflicts}
          onBusyChange={callbacks.conflicts}
          onDirtyChange={dirtyCallbacks.conflicts}
          onChanged={() => void refreshAfterResolution().catch(() => setError("충돌 상태를 다시 읽지 못했습니다."))}
        />
      )}
      <section aria-label="현재 저장소">
        <h2>현재 저장소</h2>
        <p>{repo.path}</p>
        <button type="button" disabled={busy} onClick={() => void refresh()}>
          저장소 상태 새로 고침
        </button>
        {error && <p role="alert">{error}</p>}
        {snapshot && (
          <p>
            {snapshot.branch.current} · 변경 {snapshot.changes}개 · 앞섬 {snapshot.branch.ahead} / 뒤처짐{" "}
            {snapshot.branch.behind}
          </p>
        )}
        {trees.length > 0 && (
          <details>
            <summary>연결된 작업 폴더 {trees.length}개</summary>
            <ul>
              {trees.map((path) => (
                <li key={path}>
                  {path}
                  {onProposeWorktree && (
                    <button type="button" disabled={busy} onClick={() => onProposeWorktree(path)}>
                      등록 검토
                    </button>
                  )}
                </li>
              ))}
            </ul>
          </details>
        )}
      </section>
      <GitSafetyPanel repo={repo} onBusyChange={callbacks.safety} />
      <BranchPanel repo={repo} onBusyChange={callbacks.branches} onChanged={() => void refresh()} />
      <StashPanel repo={repo} onBusyChange={callbacks.stash} onChanged={() => void refresh()} />
      <HistoryDiffPanel
        repo={repo}
        onBusyChange={callbacks.history}
        onOpenFile={onOpenFile}
        focusCommit={focusCommit?.repository === repository ? focusCommit : null}
        onBlame={showBlame}
      />
      <BlamePanel
        key={`blame:${repository}`}
        repo={repo}
        target={blame?.repository === repository ? blame : null}
        onBusyChange={callbacks.blame}
        onShowCommit={(id) =>
          setFocusCommit((previous) => ({ id, repository, sequence: (previous?.sequence ?? 0) + 1 }))
        }
      />
      <StageCommitPanel
        repo={repo}
        onBusyChange={callbacks.stage}
        onBlame={showBlame}
        onDirtyChange={dirtyCallbacks.stage}
        onOpenFile={onOpenFile}
      />
      <RemoteSyncPanel repo={repo} onBusyChange={callbacks.remote} />
      <PullRequestPanel
        key={`pr:${repository}`}
        repo={repo}
        onBusyChange={callbacks.pr}
        onDirtyChange={dirtyCallbacks.pr}
      />
      <CleanupPanel key={cleanupRevision} repo={repo} onBusyChange={callbacks.cleanup} />
    </div>
  );
}
