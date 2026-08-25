import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listActiveRuns, listRuns, runJobNow, stopActiveRun, tailLog } from "../api";
import type { Job, LogStream, Run, RunStatus } from "../types";

const DISPLAY_BYTE_LIMIT = 1024 * 1024;
const LOG_EXPORT_CHUNK_BYTES = 256 * 1024;
const LOG_EXPORT_BYTE_LIMIT = 50 * 1024 * 1024;

export interface CollectedRunLog {
  bytes: Uint8Array;
  truncated: boolean;
}

export async function collectRunLog(
  runId: string,
  stream: LogStream,
  reader: typeof tailLog = tailLog,
): Promise<CollectedRunLog> {
  const chunks: Uint8Array[] = [];
  let total = 0;
  let cursor: string | null = null;
  let truncated = false;

  while (total < LOG_EXPORT_BYTE_LIMIT) {
    const remaining = LOG_EXPORT_BYTE_LIMIT - total;
    const response = await reader(
      runId,
      stream,
      cursor,
      Math.min(LOG_EXPORT_CHUNK_BYTES, remaining),
    );
    truncated ||= response.truncated;
    const incoming = Uint8Array.from(response.data);
    if (incoming.length === 0) break;
    truncated ||= incoming.length > remaining;
    const accepted = incoming.slice(0, remaining);
    chunks.push(accepted);
    total += accepted.length;

    const previousCursor: string | null = cursor;
    cursor = response.nextCursor;
    if (cursor === previousCursor) {
      truncated = true;
      break;
    }
    if (total === LOG_EXPORT_BYTE_LIMIT) {
      const probe = await reader(runId, stream, cursor, 1);
      truncated ||= probe.data.length > 0;
      break;
    }
  }

  const bytes = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.length;
  }
  return { bytes, truncated };
}

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
  requestedJobId?: string | null;
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

export default function RunHistory({ jobs, requestedJobId = null }: RunHistoryProps) {
  const initialJobId = requestedJobId && jobs.some((job) => job.id === requestedJobId)
    ? requestedJobId
    : jobs[0]?.id ?? "";
  const [jobId, setJobId] = useState(initialJobId);
  const [startDate, setStartDate] = useState("");
  const [endDate, setEndDate] = useState("");
  const [runs, setRuns] = useState<Run[]>([]);
  const [activeRuns, setActiveRuns] = useState<Run[]>([]);
  const [activeSnapshotFresh, setActiveSnapshotFresh] = useState(false);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [stream, setStream] = useState<LogStream>("stdout");
  const [logBytes, setLogBytes] = useState<Uint8Array>(new Uint8Array());
  const [logTrimmed, setLogTrimmed] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [logError, setLogError] = useState<string | null>(null);
  const [activeSnapshotError, setActiveSnapshotError] = useState<string | null>(null);
  const [actionBusy, setActionBusy] = useState(false);
  const [contextRun, setContextRun] = useState<Run | null>(null);
  const viewGeneration = useRef(0);
  const refreshInFlight = useRef<Promise<void> | null>(null);
  const refreshPending = useRef(false);
  const refreshRef = useRef<() => Promise<void>>(() => Promise.resolve());
  const selectedStatusRef = useRef<RunStatus | null>(null);
  const queryRef = useRef({ jobId, startDate, endDate });
  queryRef.current = { jobId, startDate, endDate };

  const prepareRunContext = useCallback((target: HTMLElement) => {
    const id = target.dataset.runId;
    const run = runs.find((candidate) => candidate.id === id);
    if (!run) return;
    setSelectedRunId(run.id);
    setContextRun(run);
  }, [runs]);
  const runContextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareRunContext(target),
  });

  useEffect(() => {
    if (!jobs.some((job) => job.id === jobId)) setJobId(jobs[0]?.id ?? "");
  }, [jobId, jobs]);

  useEffect(() => {
    const id = contextRun?.id;
    if (!id) return;
    const current = runs.find((run) => run.id === id) ?? null;
    if (current) setContextRun(current);
    else {
      runContextMenu.close();
      setContextRun(null);
    }
  }, [contextRun?.id, runContextMenu.close, runs]);

  const refresh = useCallback(async () => {
    const existing = refreshInFlight.current;
    if (existing) {
      refreshPending.current = true;
      await existing;
      return;
    }
    const operation = (async () => {
      do {
        refreshPending.current = false;
        const generation = viewGeneration.current;
        const query = queryRef.current;
        setLoading(true);
        if (!query.jobId) {
          setRuns([]);
          setActiveRuns([]);
          setActiveSnapshotFresh(true);
          setLoading(false);
        } else {
          try {
            const [historyResult, activeResult] = await Promise.allSettled([
              listRuns(query.jobId, {
                limit: 50,
                startAt: dateBoundary(query.startDate, false),
                endAt: dateBoundary(query.endDate, true),
              }),
              listActiveRuns(),
            ]);
            if (generation !== viewGeneration.current) continue;
            if (historyResult.status === "fulfilled") {
              const next = historyResult.value;
              setRuns(next);
              setSelectedRunId((current) =>
                current && next.some((run) => run.id === current) ? current : next[0]?.id ?? null,
              );
              setHistoryError(null);
            } else {
              setHistoryError(historyResult.reason instanceof Error ? historyResult.reason.message : String(historyResult.reason));
            }
            if (activeResult.status === "fulfilled") {
              setActiveRuns(activeResult.value);
              setActiveSnapshotFresh(true);
              setActiveSnapshotError(null);
            } else {
              setActiveRuns([]);
              setActiveSnapshotFresh(false);
              setActiveSnapshotError(activeResult.reason instanceof Error ? activeResult.reason.message : String(activeResult.reason));
            }
          } finally {
            if (generation === viewGeneration.current) setLoading(false);
          }
        }
      } while (refreshPending.current);
    })();
    refreshInFlight.current = operation;
    try {
      await operation;
    } finally {
      if (refreshInFlight.current === operation) {
        refreshInFlight.current = null;
        if (refreshPending.current) {
          refreshPending.current = false;
          await refreshRef.current();
        }
      }
    }
  }, [endDate, jobId, startDate]);

  refreshRef.current = refresh;

  useEffect(() => {
    viewGeneration.current += 1;
    void refresh();
  }, [refresh]);

  useEffect(() => {
    const timer = window.setInterval(() => void refresh(), 1_000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const activeRun = activeSnapshotFresh ? activeRuns.find((run) => run.jobId === jobId) ?? null : null;

  const handleRunNow = async () => {
    if (!jobId) return;
    setActionBusy(true);
    try {
      await runJobNow(jobId);
      await refresh();
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setActionBusy(false);
    }
  };

  const handleStop = async () => {
    if (!jobId) return;
    const name = jobs.find((job) => job.id === jobId)?.name ?? "선택한 작업";
    if (!window.confirm(`'${name}' 작업의 활성 실행을 중지할까요?`)) return;
    setActionBusy(true);
    try {
      await stopActiveRun(jobId);
      await refresh();
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setActionBusy(false);
    }
  };

  const selectedRun = useMemo(
    () => runs.find((run) => run.id === selectedRunId) ?? null,
    [runs, selectedRunId],
  );

  const handleRerun = async (run: Run) => {
    setActionBusy(true);
    try {
      await runJobNow(run.jobId);
      await refresh();
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setActionBusy(false);
    }
  };

  const handleSaveLog = async (run: Run) => {
    setActionBusy(true);
    try {
      const collected = await collectRunLog(run.id, stream);
      const blob = new Blob([collected.bytes], { type: "text/plain;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      try {
        const anchor = document.createElement("a");
        const safeId = run.id.replace(/[^a-zA-Z0-9_-]/g, "_").slice(0, 64) || "run";
        anchor.href = url;
        anchor.download = `run-${safeId}-${stream}.log`;
        anchor.click();
      } finally {
        URL.revokeObjectURL(url);
      }
      setError(
        collected.truncated
          ? "로그 보존 범위가 바뀌었거나 저장 상한을 넘어 현재 확인 가능한 부분만 저장했습니다."
          : null,
      );
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setActionBusy(false);
    }
  };

  const runContextItems = useMemo<readonly ContextMenuEntry[]>(() => {
    if (!contextRun) return [];
    return [
      {
        type: "item",
        id: "view-log",
        label: "로그 보기",
        disabled: !contextRun.logsAvailable,
      },
      { type: "item", id: "rerun", label: "재실행", disabled: actionBusy },
      {
        type: "item",
        id: "save-log",
        label: "로그 저장",
        disabled: actionBusy || !contextRun.logsAvailable,
      },
    ];
  }, [actionBusy, contextRun]);

  const onRunContextSelect = (id: string) => {
    const run = contextRun;
    if (!run) return;
    if (id === "view-log") setSelectedRunId(run.id);
    else if (id === "rerun") void handleRerun(run);
    else if (id === "save-log") void handleSaveLog(run);
  };

  useEffect(() => {
    setLogBytes(new Uint8Array());
    setLogTrimmed(false);
    setLogError(null);
  }, [selectedRunId, stream]);

  useEffect(() => {
    selectedStatusRef.current = selectedRun?.status ?? null;
  }, [selectedRun?.status]);

  useEffect(() => {
    if (!selectedRun?.logsAvailable) return;
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
      } catch (cause) {
        if (active) setLogError(cause instanceof Error ? cause.message : String(cause));
      }
      if (
        active &&
        selectedStatusRef.current !== null &&
        ["queued", "starting", "running", "stopping"].includes(selectedStatusRef.current)
      ) {
        timer = window.setTimeout(() => void poll(), 1_000);
      }
    };
    void poll();
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [selectedRun?.id, selectedRun?.logsAvailable, stream]);

  const logText = useMemo(() => new TextDecoder().decode(logBytes), [logBytes]);

  return (
    <section className="history-section" aria-labelledby="history-title">
      <div className="section-toolbar">
        <div>
          <p className="subtitle">최근 50회 또는 지정한 기간의 실행 상태와 로그를 확인합니다.</p>
          <h3 id="history-title" className="visually-hidden">실행 기록</h3>
        </div>
        <div className="history-actions">
          <button type="button" className="button-primary" disabled={actionBusy || loading} onClick={() => void handleRunNow()}>지금 실행</button>
          <button type="button" className="button-secondary" disabled={actionBusy || loading || !activeRun} onClick={() => void handleStop()}>활성 실행 중지</button>
          <button type="button" className="button-secondary" disabled={loading} onClick={() => void refresh()}>새로고침</button>
        </div>
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

      {(error ?? historyError ?? activeSnapshotError ?? logError) ? <div className="error-banner" role="alert">오류: {error ?? historyError ?? activeSnapshotError ?? logError}</div> : null}
      {!jobId ? <div className="empty-card compact"><p>먼저 작업을 만들어 주세요.</p></div> : null}
      {jobId && !loading && runs.length === 0 ? <div className="empty-card compact"><p>조건에 맞는 실행 기록이 없습니다.</p></div> : null}

      {runs.length > 0 ? (
        <div className="history-layout">
          <div className="run-list" role="list" aria-label="실행 목록">
            {runs.map((run) => (
              <div key={run.id} role="listitem">
                <button
                  type="button"
                  className={`run-row ${selectedRunId === run.id ? "selected" : ""}`}
                  aria-current={selectedRunId === run.id ? "true" : undefined}
                  aria-pressed={selectedRunId === run.id}
                  data-run-id={run.id}
                  onClick={() => setSelectedRunId(run.id)}
                  {...runContextMenu.triggerProps}
                >
                  <span className={`run-status ${run.status}`}>{STATUS_LABEL[run.status]}</span>
                  <span><strong>{formatTime(run.startedAt ?? run.createdAt)}</strong><small>{duration(run)}</small></span>
                  <span><strong>{run.exitCode === null ? "종료 코드 —" : `종료 코드 ${run.exitCode}`}</strong><small>{run.scheduledAt === null ? "수동 실행" : "예약 실행"}</small></span>
                </button>
              </div>
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
                  {selectedRun.logsAvailable ? (
                    <div className="stream-tabs" aria-label="로그 스트림">
                      {(["stdout", "stderr"] as const).map((value) => (
                        <button key={value} type="button" className={stream === value ? "active" : ""} onClick={() => setStream(value)}>{value}</button>
                      ))}
                    </div>
                  ) : null}
                </div>
                {selectedRun.failureCode ? <p className="run-error">실행 오류 코드: {selectedRun.failureCode}</p> : null}
                {selectedRun.logsAvailable ? (
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
      <ContextMenu
        open={runContextMenu.open}
        anchor={runContextMenu.anchor}
        restoreFocusTo={runContextMenu.restoreFocusTo}
        items={runContextItems}
        onSelect={onRunContextSelect}
        onClose={runContextMenu.close}
        ariaLabel="실행 이력 메뉴"
      />
    </section>
  );
}
