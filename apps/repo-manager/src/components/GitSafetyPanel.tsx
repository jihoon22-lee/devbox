import { useEffect, useRef, useState } from "react";
import {
  GIT_SAFETY_ERROR,
  repoPreflight,
  type GitSafetyIssue,
  type GitSafetySnapshot,
  type RepoEntry,
} from "../api";

interface Props {
  repo: RepoEntry | null;
}

const ISSUE_LABELS: Record<GitSafetyIssue, string> = {
  dirty: "커밋되지 않은 변경이 있습니다.",
  detached: "HEAD가 detached 상태입니다.",
  noUpstream: "현재 브랜치에 upstream이 없습니다.",
  diverged: "현재 브랜치와 upstream이 서로 갈라졌습니다.",
  rebaseInProgress: "rebase가 진행 중입니다.",
  mergeInProgress: "merge가 진행 중입니다.",
};

/** Read-only Git state preflight. It intentionally owns no recovery action. */
export default function GitSafetyPanel({ repo }: Props) {
  const [snapshot, setSnapshot] = useState<GitSafetySnapshot | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const sequenceRef = useRef(0);
  const busyRef = useRef(false);
  const mountedRef = useRef(false);

  useEffect(() => {
    mountedRef.current = true;
    sequenceRef.current += 1;
    busyRef.current = false;
    setSnapshot(null);
    setBusy(false);
    setError(null);

    return () => {
      mountedRef.current = false;
      sequenceRef.current += 1;
      busyRef.current = false;
    };
  }, [repo?.canonicalKey, repo?.path]);

  if (!repo) return null;

  const isCurrent = (sequence: number) =>
    mountedRef.current && sequence === sequenceRef.current;

  const finishBusy = (sequence: number) => {
    if (!isCurrent(sequence)) return;
    busyRef.current = false;
    setBusy(false);
  };

  const runPreflight = async () => {
    if (busyRef.current) return;
    const sequence = ++sequenceRef.current;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      const next = await repoPreflight(repo.path);
      if (isCurrent(sequence)) setSnapshot(next);
    } catch {
      if (isCurrent(sequence)) {
        setSnapshot(null);
        setError(GIT_SAFETY_ERROR);
      }
    } finally {
      finishBusy(sequence);
    }
  };

  return (
    <section
      className="git-safety-panel"
      aria-label="Git 상태 사전 검사"
      aria-busy={busy}
    >
      <div className="git-safety-head">
        <div>
          <h2>Git 상태 사전 검사</h2>
          <div className="history-repository mono">{repo.path}</div>
        </div>
        <button
          type="button"
          className="btn"
          disabled={busy}
          onClick={() => void runPreflight()}
        >
          {busy ? "검사 중…" : "상태 검사"}
        </button>
      </div>

      {error ? <div className="error git-safety-error" role="alert">{error}</div> : null}
      <div className="git-safety-status" role="status" aria-live="polite" aria-atomic="true">
        {busy
          ? "Git 상태를 확인하는 중입니다."
          : snapshot === null
            ? "상태 검사를 실행하면 remote 작업 전 확인할 항목을 보여줍니다."
            : snapshot.safe
              ? "알려진 Git 안전 차단 항목이 없습니다."
              : `${snapshot.issues.length}개 확인이 필요합니다.`}
      </div>

      {snapshot ? (
        <>
          <dl className="git-safety-facts">
            <div>
              <dt>branch</dt>
              <dd className="mono">{snapshot.detached ? "(detached)" : snapshot.branch}</dd>
            </div>
            <div>
              <dt>upstream</dt>
              <dd className="mono">{snapshot.upstream ?? "없음"}</dd>
            </div>
            <div>
              <dt>ahead / behind</dt>
              <dd className="mono">↑{snapshot.ahead} / ↓{snapshot.behind}</dd>
            </div>
          </dl>
          {snapshot.issues.length > 0 ? (
            <ul className="git-safety-issues" aria-label="Git 확인 항목">
              {snapshot.issues.map((issue) => (
                <li key={issue}>{ISSUE_LABELS[issue]}</li>
              ))}
            </ul>
          ) : null}
        </>
      ) : null}

      <div className="git-safety-note">
        force push·reset·clean과 자동 복구는 제공하지 않습니다. 이 패널은 상태만 읽습니다.
      </div>
    </section>
  );
}
