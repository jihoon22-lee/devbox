import { useCallback, useEffect, useMemo, useState } from "react";
import { listRuns, tailLog } from "../api";
import type { Job, LogStream, Run, RunStatus } from "../types";

const DISPLAY_BYTE_LIMIT = 1024 * 1024;

const STATUS_LABEL: Record<RunStatus, string> = {
  queued: "대기",
  starting: "시작 중",
  running: "실행 중",
  stopping: "종료 중",
  succeeded: "성공",
  failed: "실패",
  cancelled: "취소",
  skipped: "건너뜀",
};

interface RunHistoryProps {
  jobs: Job[];
}

function dateBoundary(value: string, end: boolean): number | null {
  if (!value) return null;
  const time = new Date(`${value}T${end ? "23:59:59.999" : "00:00:00.000"}`).getTime();
  return Number.isFinite(time) ? time : null;
}

function formatTime(value: number | null): string {
  return value === null ? "—" : new Date(value).toLocaleString();
}

function duration(run: Run): string {
  if (run.startedAt === null) return "—";
  const end = run.endedAt ?? Date.now();
  const elapsed = Math.max(0, end - run.startedAt);
  if (elapsed < 1_000) return `${elapsed}ms`;
  return `${(elapsed / 1_000).toFixed(elapsed < 10_000 ? 1 : 0)}초`;
}

export default function RunHistory({ jobs }: RunHistoryProps) {
  const [jobId, setJobId] = useState(jobs[0]?.id ?? "");
  const [startDate, setStartDate] = useState("");
  const [endDate, setEndDate] = useState("");
  const [runs, setRuns] = useState<Run[]>([]);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [stream, setStream] = useState<LogStream>("stdout");
  const [logBytes, setLogBytes] = useState<Uint8Array>(new Uint8Array());
  const [logTrimmed, setLogTrimmed] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!jobs.some((job) => job.id === jobId)) setJobId(jobs[0]?.id ?? "");
  }, [jobId, jobs]);

  const refresh = useCallback(async () => {
    if (!jobId) {
      setRuns([]);
      return;
    }
    setLoading(true);
    try {
      const next = await listRuns(jobId, {
        limit: 50,
        startAt: dateBoundary(startDate, false),
        endAt: dateBoundary(endDate, true),
      });
      setRuns(next);
      setSelectedRunId((current) =>
        current && next.some((run) => run.id === current) ? current : next[0]?.id ?? null,
      );
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoading(false);
    }
  }, [endDate, jobId, startDate]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const selectedRun = useMemo(
    () => runs.find((run) => run.id === selectedRunId) ?? null,
    [runs, selectedRunId],
  );

  useEffect(() => {
    setLogBytes(new Uint8Array());
    setLogTrimmed(false);
  }, [selectedRunId, stream]);

  useEffect(() => {
    if (!selectedRun?.logDir) return;
    let active = true;
    let nextCursor: string | null = null;
    let timer = 0;

    const poll = async () => {
      try {
        const response = await tailLog(selectedRun.id, stream, nextCursor);
        if (!active) return;
        nextCursor = response.nextCursor;
        setLogBytes((current) => {
          const incoming = Uint8Array.from(response.data);
          const base = response.truncated ? new Uint8Array() : current;
          const combined = new Uint8Array(base.length + incoming.length);
          combined.set(base);
          combined.set(incoming, base.length);
          if (combined.length <= DISPLAY_BYTE_LIMIT) return combined;
          setLogTrimmed(true);
          return combined.slice(combined.length - DISPLAY_BYTE_LIMIT);
        });
        if (response.truncated) setLogTrimmed(true);
        setError(null);
      } catch (cause) {
        if (active) setError(cause instanceof Error ? cause.message : String(cause));
      }
      if (active) timer = window.setTimeout(() => void poll(), 1_000);
    };
    void poll();
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [selectedRun?.id, selectedRun?.logDir, stream]);

  const logText = useMemo(() => new TextDecoder().decode(logBytes), [logBytes]);

  return (
    <section className="history-section" aria-labelledby="history-title">
      <div className="section-toolbar">
        <div>
          <p className="subtitle">최근 50회 또는 지정한 기간의 실행 상태와 로그를 확인합니다.</p>
          <h3 id="history-title" className="visually-hidden">실행 기록</h3>
        </div>
        <button type="button" className="button-secondary" disabled={loading} onClick={() => void refresh()}>
          새로고침
        </button>
      </div>

      <div className="history-filters">
        <label className="field">
          <span>작업</span>
          <select aria-label="기록 작업" value={jobId} onChange={(event) => setJobId(event.target.value)}>
            {jobs.map((job) => <option key={job.id} value={job.id}>{job.name}</option>)}
          </select>
        </label>
        <label className="field">
          <span>시작일</span>
          <input aria-label="기록 시작일" type="date" value={startDate} onChange={(event) => setStartDate(event.target.value)} />
        </label>
        <label className="field">
          <span>종료일</span>
          <input aria-label="기록 종료일" type="date" value={endDate} onChange={(event) => setEndDate(event.target.value)} />
        </label>
      </div>

      {error ? <div className="error-banner" role="alert">오류: {error}</div> : null}
      {!jobId ? <div className="empty-card compact"><p>먼저 작업을 만들어 주세요.</p></div> : null}
      {jobId && !loading && runs.length === 0 ? <div className="empty-card compact"><p>조건에 맞는 실행 기록이 없습니다.</p></div> : null}

      {runs.length > 0 ? (
        <div className="history-layout">
          <div className="run-list" role="list" aria-label="실행 목록">
            {runs.map((run) => (
              <button
                key={run.id}
                type="button"
                role="listitem"
                className={`run-row ${selectedRunId === run.id ? "selected" : ""}`}
                onClick={() => setSelectedRunId(run.id)}
              >
                <span className={`run-status ${run.status}`}>{STATUS_LABEL[run.status]}</span>
                <span><strong>{formatTime(run.startedAt ?? run.createdAt)}</strong><small>{duration(run)}</small></span>
                <span><strong>{run.exitCode === null ? "종료 코드 —" : `종료 코드 ${run.exitCode}`}</strong><small>{run.scheduledAt === null ? "수동 실행" : "예약 실행"}</small></span>
              </button>
            ))}
          </div>

          <section className="run-detail" aria-label="선택한 실행 상세">
            {selectedRun ? (
              <>
                <div className="run-detail-heading">
                  <div>
                    <span className={`run-status ${selectedRun.status}`}>{STATUS_LABEL[selectedRun.status]}</span>
                    <p>{formatTime(selectedRun.startedAt ?? selectedRun.createdAt)} · {duration(selectedRun)}</p>
                  </div>
                  {selectedRun.logDir ? (
                    <div className="stream-tabs" aria-label="로그 스트림">
                      {(["stdout", "stderr"] as const).map((value) => (
                        <button key={value} type="button" className={stream === value ? "active" : ""} onClick={() => setStream(value)}>{value}</button>
                      ))}
                    </div>
                  ) : null}
                </div>
                {selectedRun.errorMessage ? <p className="run-error">{selectedRun.errorMessage}</p> : null}
                {selectedRun.logDir ? (
                  <div className="log-panel">
                    {logTrimmed ? <div className="log-notice">보존 범위 또는 화면 한도를 벗어난 이전 로그는 생략했습니다.</div> : null}
                    <pre aria-label={`${stream} 로그`}>{logText || "로그를 기다리는 중…"}</pre>
                  </div>
                ) : <p className="muted run-no-log">보존된 로그가 없습니다.</p>}
              </>
            ) : null}
          </section>
        </div>
      ) : null}
    </section>
  );
}
