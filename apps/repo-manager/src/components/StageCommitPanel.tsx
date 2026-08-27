import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import {
  GIT_MUTATION_ERROR,
  repoChanges,
  repoCommit,
  repoStage,
  repoUnstage,
  type ChangeEntry,
  type RepoEntry,
} from "../api";

const MAX_COMMIT_MESSAGE_BYTES = 16 * 1024;

interface Props {
  repo: RepoEntry | null;
}

type Selection = "stage" | "unstage";

function isCommitMessageValid(value: string): boolean {
  return value.trim().length > 0
    && new TextEncoder().encode(value).byteLength <= MAX_COMMIT_MESSAGE_BYTES
    && ![...value].some((character) => {
      const code = character.charCodeAt(0);
      return code < 0x20 && code !== 0x09 && code !== 0x0a && code !== 0x0d;
    });
}

function pathsFor(changes: ChangeEntry[] | null, selection: Selection): ChangeEntry[] {
  return (changes ?? []).filter((change) => selection === "stage" ? change.unstaged : change.staged);
}

/** Explicit local working-tree stage/unstage and index-only commit surface. */
export default function StageCommitPanel({ repo }: Props) {
  const [changes, setChanges] = useState<ChangeEntry[] | null>(null);
  const [stageSelection, setStageSelection] = useState<Set<string>>(new Set());
  const [unstageSelection, setUnstageSelection] = useState<Set<string>>(new Set());
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const sequenceRef = useRef(0);
  const busyRef = useRef(false);
  const mountedRef = useRef(false);

  useEffect(() => {
    mountedRef.current = true;
    sequenceRef.current += 1;
    busyRef.current = false;
    setChanges(null);
    setStageSelection(new Set());
    setUnstageSelection(new Set());
    setMessage("");
    setBusy(false);
    setError(null);

    return () => {
      mountedRef.current = false;
      sequenceRef.current += 1;
      busyRef.current = false;
    };
  }, [repo?.canonicalKey, repo?.path]);

  if (!repo) return null;

  const isCurrent = (sequence: number) => mountedRef.current && sequence === sequenceRef.current;

  const setSelectionsForChanges = (next: ChangeEntry[]) => {
    const stagePaths = new Set(pathsFor(next, "stage").map((change) => change.path));
    const unstagePaths = new Set(pathsFor(next, "unstage").map((change) => change.path));
    setStageSelection((current) => new Set([...current].filter((path) => stagePaths.has(path))));
    setUnstageSelection((current) => new Set([...current].filter((path) => unstagePaths.has(path))));
  };

  const finishBusy = (sequence: number) => {
    if (!isCurrent(sequence)) return;
    busyRef.current = false;
    setBusy(false);
  };

  const loadChanges = async () => {
    if (busyRef.current) return;
    const sequence = ++sequenceRef.current;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      const next = await repoChanges(repo.path);
      if (!isCurrent(sequence)) return;
      setChanges(next);
      setSelectionsForChanges(next);
    } catch {
      if (isCurrent(sequence)) {
        setError(GIT_MUTATION_ERROR);
        setChanges(null);
        setStageSelection(new Set());
        setUnstageSelection(new Set());
      }
    } finally {
      finishBusy(sequence);
    }
  };

  const runPathMutation = async (selection: Selection) => {
    if (busyRef.current) return;
    const selected = [...(selection === "stage" ? stageSelection : unstageSelection)];
    if (selected.length === 0) {
      setError(GIT_MUTATION_ERROR);
      return;
    }
    const sequence = ++sequenceRef.current;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      if (selection === "stage") await repoStage(repo.path, selected);
      else await repoUnstage(repo.path, selected);
      if (!isCurrent(sequence)) return;

      const next = await repoChanges(repo.path);
      if (!isCurrent(sequence)) return;
      setChanges(next);
      setStageSelection(new Set());
      setUnstageSelection(new Set());
      setSelectionsForChanges(next);
    } catch {
      // Keep the previous selection and status visible so a failed action is
      // isolated and can be retried without silently changing the intent.
      if (isCurrent(sequence)) setError(GIT_MUTATION_ERROR);
    } finally {
      finishBusy(sequence);
    }
  };

  const runCommit = async () => {
    if (busyRef.current) return;
    if (!isCommitMessageValid(message) || !pathsFor(changes, "unstage").length) {
      setError(GIT_MUTATION_ERROR);
      return;
    }
    const sequence = ++sequenceRef.current;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      await repoCommit(repo.path, message);
      if (!isCurrent(sequence)) return;
      setMessage("");

      const next = await repoChanges(repo.path);
      if (!isCurrent(sequence)) return;
      setChanges(next);
      setStageSelection(new Set());
      setUnstageSelection(new Set());
      setSelectionsForChanges(next);
    } catch {
      // Commit message and selections remain intact after a failed commit.
      if (isCurrent(sequence)) setError(GIT_MUTATION_ERROR);
    } finally {
      finishBusy(sequence);
    }
  };

  const onMessageKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
      event.preventDefault();
      void runCommit();
    }
  };

  const toggleSelection = (selection: Selection, path: string) => {
    if (busyRef.current) return;
    const setter = selection === "stage" ? setStageSelection : setUnstageSelection;
    setter((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
    setError(null);
  };

  const unstaged = pathsFor(changes, "stage");
  const staged = pathsFor(changes, "unstage");

  return (
    <section className="stage-commit-panel" aria-label="Git stage and commit" aria-busy={busy}>
      <div className="stage-commit-head">
        <div>
          <h2>Stage · commit</h2>
          <div className="history-repository mono">{repo.path}</div>
        </div>
        <button
          type="button"
          className="btn"
          disabled={busy}
          onClick={() => void loadChanges()}
        >
          {busy ? "처리 중…" : "변경 파일 불러오기"}
        </button>
      </div>

      {error ? <div className="error stage-commit-error" role="alert">{error}</div> : null}
      <div className="stage-commit-status" role="status" aria-live="polite" aria-atomic="true">
        {busy
          ? "Git 변경 사항을 처리하는 중입니다."
          : changes === null
            ? "변경 파일을 불러오면 선택한 파일만 stage·unstage할 수 있습니다."
            : `${unstaged.length}개 unstaged · ${staged.length}개 staged`}
      </div>

      {changes !== null ? (
        <div className="change-columns">
          <fieldset className="change-group" aria-label="Unstaged changes">
            <legend>Unstaged changes</legend>
            {unstaged.map((change) => (
              <label className="change-row" key={`unstaged:${change.path}`}>
                <input
                  type="checkbox"
                  aria-label={`stage ${change.path}`}
                  checked={stageSelection.has(change.path)}
                  disabled={busy}
                  onChange={() => toggleSelection("stage", change.path)}
                />
                <span className="change-kind">{change.kind}</span>
                <span className="mono change-path">{change.path}</span>
                <span className="change-status mono">{change.indexStatus}{change.worktreeStatus}</span>
              </label>
            ))}
            {unstaged.length === 0 ? <div className="change-empty dim">unstaged 변경이 없습니다.</div> : null}
            <button
              type="button"
              className="btn"
              disabled={busy || stageSelection.size === 0}
              onClick={() => void runPathMutation("stage")}
            >
              선택 항목 stage ({stageSelection.size})
            </button>
          </fieldset>

          <fieldset className="change-group" aria-label="Staged changes">
            <legend>Staged changes</legend>
            {staged.map((change) => (
              <label className="change-row" key={`staged:${change.path}`}>
                <input
                  type="checkbox"
                  aria-label={`unstage ${change.path}`}
                  checked={unstageSelection.has(change.path)}
                  disabled={busy}
                  onChange={() => toggleSelection("unstage", change.path)}
                />
                <span className="change-kind">{change.kind}</span>
                <span className="mono change-path">{change.path}</span>
                <span className="change-status mono">{change.indexStatus}{change.worktreeStatus}</span>
              </label>
            ))}
            {staged.length === 0 ? <div className="change-empty dim">staged 변경이 없습니다.</div> : null}
            <button
              type="button"
              className="btn"
              disabled={busy || unstageSelection.size === 0}
              onClick={() => void runPathMutation("unstage")}
            >
              선택 항목 unstage ({unstageSelection.size})
            </button>
          </fieldset>
        </div>
      ) : null}

      <div className="commit-form">
        <label htmlFor="repo-commit-message">Commit message</label>
        <textarea
          id="repo-commit-message"
          aria-label="Commit message"
          value={message}
          maxLength={MAX_COMMIT_MESSAGE_BYTES}
          disabled={busy}
          placeholder="변경 내용을 설명하세요"
          onChange={(event) => {
            setMessage(event.currentTarget.value);
            setError(null);
          }}
          onKeyDown={onMessageKeyDown}
        />
        <div className="commit-form-foot">
          <span className="dim">현재 staged 파일만 commit합니다. Ctrl+Enter로 실행</span>
          <button
            type="button"
            className="btn primary"
            disabled={busy || staged.length === 0 || !isCommitMessageValid(message)}
            onClick={() => void runCommit()}
          >
            Commit ({staged.length})
          </button>
        </div>
      </div>
    </section>
  );
}
