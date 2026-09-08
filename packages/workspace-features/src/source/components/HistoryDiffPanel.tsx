import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import {
  GIT_VIEW_ERROR,
  repoCommitDetail,
  repoDiff,
  repoHistory,
  type CommitDetail,
  type DiffResult,
  type HistoryResult,
  type RepoEntry,
} from "../api";

const DEFAULT_HISTORY_LIMIT = "50";
const MAX_HISTORY_LIMIT = 100;

interface Props {
  repo: RepoEntry | null;
}

type DiffSelection = "workingTree" | "commit";

function parseLimit(value: string): number | null {
  if (!/^\d+$/u.test(value)) return null;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed >= 1 && parsed <= MAX_HISTORY_LIMIT
    ? parsed
    : null;
}

/** Read-only Git history/detail/diff surface for the selected repository. */
export default function HistoryDiffPanel({ repo }: Props) {
  const [historyLimit, setHistoryLimit] = useState(DEFAULT_HISTORY_LIMIT);
  const [history, setHistory] = useState<HistoryResult | null>(null);
  const [selectedCommitId, setSelectedCommitId] = useState<string | null>(null);
  const [detail, setDetail] = useState<CommitDetail | null>(null);
  const [diff, setDiff] = useState<DiffResult | null>(null);
  const [diffSelection, setDiffSelection] = useState<DiffSelection>("workingTree");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const sequenceRef = useRef(0);
  const busyRef = useRef(false);
  const composingRef = useRef(0);

  useEffect(() => {
    sequenceRef.current += 1;
    busyRef.current = false;
    setBusy(false);
    setHistory(null);
    setSelectedCommitId(null);
    setDetail(null);
    setDiff(null);
    setDiffSelection("workingTree");
    setError(null);

    return () => {
      sequenceRef.current += 1;
      busyRef.current = false;
    };
  }, [repo?.canonicalKey, repo?.path]);

  if (!repo) return null;

  const runHistory = async () => {
    if (busyRef.current) return;
    const limit = parseLimit(historyLimit);
    if (limit === null) {
      setError(GIT_VIEW_ERROR);
      return;
    }

    const sequence = ++sequenceRef.current;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      const result = await repoHistory(repo.path, limit);
      if (sequence !== sequenceRef.current) return;
      setHistory(result);
      setSelectedCommitId(null);
      setDetail(null);
      setDiff(null);
      setDiffSelection("workingTree");
    } catch {
      if (sequence === sequenceRef.current) {
        setError(GIT_VIEW_ERROR);
        setHistory(null);
        setDetail(null);
        setDiff(null);
      }
    } finally {
      if (sequence === sequenceRef.current) {
        busyRef.current = false;
        setBusy(false);
      }
    }
  };

  const selectCommit = async (commitId: string) => {
    if (busyRef.current) return;
    const sequence = ++sequenceRef.current;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    setSelectedCommitId(commitId);
    setDetail(null);
    setDiff(null);
    setDiffSelection("commit");
    try {
      const [nextDetail, nextDiff] = await Promise.all([
        repoCommitDetail(repo.path, commitId),
        repoDiff(repo.path, commitId),
      ]);
      if (sequence !== sequenceRef.current) return;
      setDetail(nextDetail);
      setDiff(nextDiff);
    } catch {
      if (sequence === sequenceRef.current) {
        setError(GIT_VIEW_ERROR);
        setDetail(null);
        setDiff(null);
      }
    } finally {
      if (sequence === sequenceRef.current) {
        busyRef.current = false;
        setBusy(false);
      }
    }
  };

  const loadDiff = async (selection: DiffSelection) => {
    if (busyRef.current) return;
    if (selection === "commit" && !selectedCommitId) {
      setError(GIT_VIEW_ERROR);
      return;
    }
    const sequence = ++sequenceRef.current;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    setDiffSelection(selection);
    try {
      const nextDiff = await repoDiff(repo.path, selection === "commit" ? selectedCommitId : null);
      if (sequence !== sequenceRef.current) return;
      setDiff(nextDiff);
    } catch {
      if (sequence === sequenceRef.current) {
        setError(GIT_VIEW_ERROR);
        setDiff(null);
      }
    } finally {
      if (sequence === sequenceRef.current) {
        busyRef.current = false;
        setBusy(false);
      }
    }
  };

  const onLimitKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Enter" || event.nativeEvent.isComposing || composingRef.current > 0) return;
    event.preventDefault();
    void runHistory();
  };

  return (
    <section className="history-panel" aria-label="Git 히스토리 및 diff" aria-busy={busy}>
      <div className="history-panel-head">
        <div>
          <h2>Git 히스토리 · diff</h2>
          <div className="history-repository mono">{repo.path}</div>
        </div>
        <label className="history-limit-field">
          히스토리 표시 개수
          <input
            aria-label="히스토리 표시 개수"
            inputMode="numeric"
            value={historyLimit}
            maxLength={3}
            disabled={busy}
            onChange={(event) => {
              setHistoryLimit(event.currentTarget.value);
              setError(null);
            }}
            onCompositionStart={() => { composingRef.current += 1; }}
            onCompositionEnd={() => { composingRef.current = Math.max(0, composingRef.current - 1); }}
            onKeyDown={onLimitKeyDown}
          />
          <span className="history-limit-help">1–{MAX_HISTORY_LIMIT}</span>
        </label>
        <button className="btn primary" disabled={busy} onClick={() => void runHistory()}>
          {busy ? "불러오는 중…" : "히스토리 불러오기"}
        </button>
      </div>

      {error ? <div className="error history-error" role="alert">{error}</div> : null}
      <div className="history-status" role="status" aria-live="polite" aria-atomic="true">
        {busy
          ? "Git 정보를 읽는 중입니다."
          : history
            ? `${history.entries.length}개 commit${history.hasMore ? " 이상" : ""}`
            : "히스토리를 불러오면 커밋과 diff를 확인할 수 있습니다."}
      </div>

      {history ? (
        <div className="history-layout">
          <div className="history-list" aria-label="커밋 히스토리" role="list">
            {history.entries.map((entry) => (
              <div className="history-entry-row" key={entry.id} role="listitem">
                <span className="history-graph" aria-hidden="true">
                  <span className="history-graph-node" />
                </span>
                <button
                  type="button"
                  className={`history-entry ${selectedCommitId === entry.id ? "selected" : ""}`}
                  aria-pressed={selectedCommitId === entry.id}
                  disabled={busy}
                  onClick={() => void selectCommit(entry.id)}
                >
                  <span className="history-entry-head">
                    <span className="mono">{entry.shortId}</span>
                    <span>{entry.authoredAt}</span>
                  </span>
                  <strong>{entry.subject}</strong>
                  <span className="history-entry-author">{entry.author} · {entry.authorEmail}</span>
                  <span className="history-entry-parents">
                    {entry.parents.length > 0
                      ? `상위 커밋 ${entry.parents.map((parent) => parent.slice(0, 12)).join(" · ")}`
                      : "루트 커밋"}
                  </span>
                </button>
              </div>
            ))}
            {history.entries.length === 0 ? <div className="dim">commit history가 없습니다.</div> : null}
          </div>

          <div className="history-detail-column">
            {detail ? (
              <article className="commit-detail" aria-label="커밋 상세">
                <h3>{detail.subject}</h3>
                <div className="commit-meta mono">
                  {detail.id.slice(0, 12)} · {detail.authoredAt} · {detail.author} · {detail.authorEmail}
                </div>
                <pre>{detail.body || "(본문 없음)"}</pre>
              </article>
            ) : (
              <div className="history-empty dim">commit을 선택하면 detail을 표시합니다.</div>
            )}

              <div className="diff-toolbar" aria-label="diff 작업">
              <button
                type="button"
                className={`btn ${diffSelection === "workingTree" ? "primary" : ""}`}
                disabled={busy}
                onClick={() => void loadDiff("workingTree")}
              >
                작업 트리 diff
              </button>
              <button
                type="button"
                className={`btn ${diffSelection === "commit" ? "primary" : ""}`}
                disabled={busy || !selectedCommitId}
                onClick={() => void loadDiff("commit")}
              >
                선택한 커밋 diff
              </button>
            </div>

            {diff ? <DiffView result={diff} /> : <div className="history-empty dim">diff를 선택하세요.</div>}
          </div>
        </div>
      ) : null}
    </section>
  );
}

function DiffView({ result }: { result: DiffResult }) {
  return (
    <div className="repo-diff" aria-label={`${result.scope} diff`}>
      {result.truncated ? (
        <div className="note diff-truncated" role="note">
          diff가 안전한 출력 상한에 도달해 일부 파일 또는 줄을 생략했습니다.
        </div>
      ) : null}
      {result.files.map((file, index) => (
        <article className="diff-file" key={`${index}:${file.path}:${file.oldPath ?? ""}`}>
          <div className="diff-file-head">
            <strong>{file.status}</strong>
            <span className="mono">{file.path}</span>
            {file.oldPath ? <span className="mono">← {file.oldPath}</span> : null}
          </div>
          {file.binary ? (
            <div className="note diff-binary" role="note">바이너리 파일 — 내용은 표시하지 않습니다.</div>
          ) : (
            <pre className="diff-patch">{file.patch || "(변경 내용 없음)"}</pre>
          )}
          {file.truncated ? <div className="note diff-truncated">이 파일의 diff가 상한으로 잘렸습니다.</div> : null}
        </article>
      ))}
      {result.files.length === 0 ? <div className="history-empty dim">변경 사항이 없습니다.</div> : null}
    </div>
  );
}
