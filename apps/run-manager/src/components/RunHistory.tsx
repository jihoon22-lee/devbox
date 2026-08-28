import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listActiveRuns, listRuns, runJobNow, searchRunLogs, stopActiveRun, tailLog } from "../api";
import type {
  Job,
  LogLevel,
  LogSearchMatch,
  LogSearchMode,
  LogSearchResponse,
  LogStream,
  Run,
  RunDefinitionKind,
  RunStatus,
} from "../types";

const DISPLAY_BYTE_LIMIT = 1024 * 1024;
const LOG_EXPORT_CHUNK_BYTES = 256 * 1024;
const LOG_EXPORT_BYTE_LIMIT = 50 * 1024 * 1024;
export const LOG_SEARCH_QUERY_BYTE_LIMIT = 512;
export const LOG_SEARCH_MAX_RESULT_LINES = 500;
const MAX_WRAPPED_LOG_LINES = 20_000;

function utf8ByteLength(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

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
  const date = new Date(`${value}T00:00:00.000`);
  if (!Number.isFinite(date.getTime())) return null;
  if (end) date.setDate(date.getDate() + 1);
  return date.getTime();
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

function durationFilter(value: string): number | null {
  if (!value.trim()) return null;
  const seconds = Number(value);
  const millis = Math.floor(seconds * 1_000);
  // Preserve invalid non-empty input as an invalid native value instead of
  // silently broadening the query to "no duration filter". The backend owns
  // the authoritative 0..30-day validation and returns a fixed error.
  if (!Number.isFinite(seconds) || !Number.isSafeInteger(millis)) return -1;
  return millis;
}

function searchErrorMessage(cause: unknown): string {
  return String(cause) === "log-search-invalid-pattern"
    ? "정규식 패턴이 올바르지 않습니다."
    : "로그 검색을 완료하지 못했습니다.";
}

function levelLabel(level: LogLevel | null): string {
  return level === null ? "레벨 없음" : level;
}

function isSafeSearchResponse(response: LogSearchResponse, runId: string): boolean {
  if (
    !response ||
    !Array.isArray(response.matches) ||
    !Array.isArray(response.sources) ||
    !Number.isSafeInteger(response.scannedLines) ||
    response.scannedLines < 0 ||
    response.scannedLines > 50_000 ||
    !Number.isSafeInteger(response.scannedBytes) ||
    response.scannedBytes < 0 ||
    response.scannedBytes > 8 * 1024 * 1024 ||
    typeof response.truncated !== "boolean" ||
    response.matches.length > LOG_SEARCH_MAX_RESULT_LINES ||
    response.sources.length > 2
  ) return false;
  const sourceIds = new Set<string>();
  const sourceStreams = new Set<LogStream>();
  for (const source of response.sources) {
    if (source.kind !== "log-source/v1" || source.runId !== runId || !["stdout", "stderr"].includes(source.stream) || sourceStreams.has(source.stream)) {
      return false;
    }
    const expected = `run-manager:${runId}:${source.stream}`;
    if (source.sourceId !== expected || source.sourceId.length > 192) return false;
    sourceIds.add(source.sourceId);
    sourceStreams.add(source.stream);
  }
  return response.matches.every((match) => {
    if (!sourceIds.has(match.sourceId) || !["stdout", "stderr"].includes(match.stream)) return false;
    if (match.sourceId !== `run-manager:${runId}:${match.stream}`) return false;
    if (!Number.isSafeInteger(match.lineNumber) || match.lineNumber < 1 || match.lineNumber > 50_000) return false;
    if (match.level !== null && !["trace", "debug", "info", "warn", "error"].includes(match.level)) return false;
    return match.timestampMillis === null || Number.isSafeInteger(match.timestampMillis);
  });
}

export default function RunHistory({ jobs, requestedJobId = null }: RunHistoryProps) {
  const initialJobId = requestedJobId && jobs.some((job) => job.id === requestedJobId)
    ? requestedJobId
    : jobs[0]?.id ?? "";
  const [jobId, setJobId] = useState(initialJobId);
  const [kind, setKind] = useState<RunDefinitionKind | "">("");
  const [status, setStatus] = useState<RunStatus | "">("");
  const [startDate, setStartDate] = useState("");
  const [endDate, setEndDate] = useState("");
  const [minDuration, setMinDuration] = useState("");
  const [maxDuration, setMaxDuration] = useState("");
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
  const [searchQuery, setSearchQuery] = useState("");
  const [searchMode, setSearchMode] = useState<LogSearchMode>("literal");
  const [searchSource, setSearchSource] = useState<LogStream | "">("");
  const [searchLevel, setSearchLevel] = useState<LogLevel | "">("");
  const [searchResponse, setSearchResponse] = useState<LogSearchResponse | null>(null);
  const [searchIndex, setSearchIndex] = useState(-1);
  const [searchBusy, setSearchBusy] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [contextRun, setContextRun] = useState<Run | null>(null);
  const visibleJobs = kind ? jobs.filter((job) => job.kind === kind) : jobs;
  const viewGeneration = useRef(0);
  const refreshInFlight = useRef<Promise<void> | null>(null);
  const refreshPending = useRef(false);
  const refreshRef = useRef<() => Promise<void>>(() => Promise.resolve());
  const selectedStatusRef = useRef<RunStatus | null>(null);
  const searchGeneration = useRef(0);
  const searchBusyRef = useRef(false);
  const mountedRef = useRef(true);
  const logLineRefs = useRef(new Map<number, HTMLSpanElement>());
  const queryRef = useRef({ jobId, kind, status, startDate, endDate, minDuration, maxDuration });
  queryRef.current = { jobId, kind, status, startDate, endDate, minDuration, maxDuration };

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      searchGeneration.current += 1;
    };
  }, []);

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
    if (!(kind ? jobs.some((job) => job.id === jobId && job.kind === kind) : jobs.some((job) => job.id === jobId))) {
      setJobId("");
    }
  }, [jobId, kind, jobs]);

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
        if (jobs.length === 0) {
          setRuns([]);
          setActiveRuns([]);
          setActiveSnapshotFresh(true);
          setLoading(false);
        } else {
          try {
            const [historyResult, activeResult] = await Promise.allSettled([
              listRuns(query.jobId || null, {
                limit: 50,
                startAt: dateBoundary(query.startDate, false),
                endAt: dateBoundary(query.endDate, true),
                kind: query.kind || null,
                status: query.status || null,
                minDurationMs: durationFilter(query.minDuration),
                maxDurationMs: durationFilter(query.maxDuration),
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
  }, [endDate, jobId, jobs, kind, maxDuration, minDuration, startDate, status]);

  refreshRef.current = refresh;

  useEffect(() => {
    viewGeneration.current += 1;
    void refresh();
  }, [refresh]);

  useEffect(() => {
    const timer = window.setInterval(() => void refresh(), 1_000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const selectedDefinition = jobs.find((job) => job.id === jobId) ?? null;
  const activeRun = activeSnapshotFresh && selectedDefinition?.kind === "job"
    ? activeRuns.find((run) => run.jobId === jobId) ?? null
    : null;

  const handleRunNow = async () => {
    if (!jobId || selectedDefinition?.kind !== "job") return;
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
    if (!jobId || selectedDefinition?.kind !== "job") return;
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
    if (jobs.find((job) => job.id === run.jobId)?.kind !== "job") return;
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

  const handleSearch = async () => {
    const run = selectedRun;
    if (!run?.logsAvailable || !searchQuery || searchBusyRef.current) return;
    if (utf8ByteLength(searchQuery) > LOG_SEARCH_QUERY_BYTE_LIMIT) {
      setSearchError("검색어가 허용된 크기를 초과했습니다.");
      return;
    }
    const generation = ++searchGeneration.current;
    searchBusyRef.current = true;
    setSearchBusy(true);
    setSearchError(null);
    setSearchResponse(null);
    setSearchIndex(-1);
    try {
      const response = await searchRunLogs(run.id, {
        query: searchQuery,
        mode: searchMode,
        source: searchSource || null,
        level: searchLevel || null,
        startAt: dateBoundary(startDate, false),
        endAt: dateBoundary(endDate, true),
      });
      if (!isSafeSearchResponse(response, run.id)) throw new Error("log-search-invalid-source");
      if (!mountedRef.current || generation !== searchGeneration.current) return;
      setSearchResponse(response);
      setSearchIndex(response.matches.length > 0 ? 0 : -1);
    } catch (cause) {
      if (mountedRef.current && generation === searchGeneration.current) {
        setSearchError(searchErrorMessage(cause));
      }
    } finally {
      searchBusyRef.current = false;
      if (mountedRef.current) setSearchBusy(false);
    }
  };

  const clearSearch = () => {
    searchGeneration.current += 1;
    setSearchQuery("");
    setSearchResponse(null);
    setSearchIndex(-1);
    setSearchError(null);
  };

  const moveToSearchMatch = (index: number) => {
    const matches = searchResponse?.matches ?? [];
    if (matches.length === 0) return;
    const next = (index + matches.length) % matches.length;
    const match = matches[next];
    setSearchIndex(next);
    if (match.stream !== stream) setStream(match.stream);
  };

  const runContextItems = useMemo<readonly ContextMenuEntry[]>(() => {
    if (!contextRun) return [];
    const definition = jobs.find((job) => job.id === contextRun.jobId);
    return [
      {
        type: "item",
        id: "view-log",
        label: "로그 보기",
        disabled: !contextRun.logsAvailable,
      },
      { type: "item", id: "rerun", label: "재실행", disabled: actionBusy || definition?.kind !== "job" },
      {
        type: "item",
        id: "save-log",
        label: "로그 저장",
        disabled: actionBusy || !contextRun.logsAvailable,
      },
    ];
  }, [actionBusy, contextRun, jobs]);

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

  // A result is tied to the selected run and exact search controls. Clear it
  // as soon as any of those controls changes so an old async response cannot
  // be mistaken for the new query.
  useEffect(() => {
    searchGeneration.current += 1;
    setSearchResponse(null);
    setSearchIndex(-1);
    setSearchError(null);
  }, [endDate, searchLevel, searchMode, searchQuery, searchSource, selectedRunId, startDate]);

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
  const logLines = useMemo(() => logText.split("\n"), [logText]);
  const activeSearchMatch: LogSearchMatch | null =
    searchResponse && searchIndex >= 0 ? searchResponse.matches[searchIndex] ?? null : null;
  const activeSearchLineUnavailable = Boolean(
    activeSearchMatch &&
    (activeSearchMatch.stream !== stream ||
      logTrimmed ||
      !logText ||
      logLines.length > MAX_WRAPPED_LOG_LINES ||
      activeSearchMatch.lineNumber > logLines.length),
  );

  useEffect(() => {
    if (!activeSearchMatch || activeSearchMatch.stream !== stream) return;
    const line = logLineRefs.current.get(activeSearchMatch.lineNumber);
    if (line && typeof line.scrollIntoView === "function") {
      line.scrollIntoView({ block: "center" });
    }
  }, [activeSearchMatch, logLines, stream]);

  const renderLogText = () => {
    if (!logText) return "로그를 기다리는 중…";
    if (logLines.length > MAX_WRAPPED_LOG_LINES) return logText;
    return logLines.map((line, index) => (
      <span
        key={index}
        ref={(element) => {
          if (element) logLineRefs.current.set(index + 1, element);
          else logLineRefs.current.delete(index + 1);
        }}
        className={activeSearchMatch?.stream === stream && !logTrimmed && activeSearchMatch.lineNumber === index + 1 ? "log-line match-active" : "log-line"}
        data-line-number={index + 1}
      >
        {line}{index < logLines.length - 1 ? "\n" : null}
      </span>
    ));
  };

  return (
    <section className="history-section" aria-labelledby="history-title">
      <div className="section-toolbar">
        <div>
          <p className="subtitle">최근 50회 또는 지정한 기간의 실행 상태와 로그를 확인합니다.</p>
          <h3 id="history-title" className="visually-hidden">실행 기록</h3>
        </div>
        <div className="history-actions">
          <button type="button" className="button-primary" disabled={actionBusy || loading || selectedDefinition?.kind !== "job"} onClick={() => void handleRunNow()}>지금 실행</button>
          <button type="button" className="button-secondary" disabled={actionBusy || loading || !activeRun} onClick={() => void handleStop()}>활성 실행 중지</button>
          <button type="button" className="button-secondary" disabled={loading} onClick={() => void refresh()}>새로고침</button>
        </div>
      </div>

      <div className="history-filters">
        <label className="field">
          <span>대상 종류</span>
          <select aria-label="기록 대상 종류" value={kind} onChange={(event) => setKind(event.target.value as RunDefinitionKind | "")}>
            <option value="">작업과 서비스</option>
            <option value="job">작업만</option>
            <option value="service">서비스만</option>
          </select>
        </label>
        <label className="field">
          <span>작업 또는 서비스</span>
          <select aria-label="기록 작업" value={jobId} onChange={(event) => setJobId(event.target.value)}>
            <option value="">모든 대상</option>
            {visibleJobs.map((job) => <option key={job.id} value={job.id}>{job.name}</option>)}
          </select>
        </label>
        <label className="field">
          <span>상태</span>
          <select aria-label="기록 상태" value={status} onChange={(event) => setStatus(event.target.value as RunStatus | "")}>
            <option value="">모든 상태</option>
            <option value="succeeded">성공</option>
            <option value="failed">실패</option>
            <option value="cancelled">취소</option>
            <option value="skipped">건너뜀</option>
            <option value="queued">대기</option>
            <option value="starting">시작 중</option>
            <option value="running">실행 중</option>
            <option value="stopping">종료 중</option>
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
        <label className="field">
          <span>최소 실행 시간 <em>(초)</em></span>
          <input aria-label="최소 실행 시간(초)" type="number" min="0" max="2592000" step="1" value={minDuration} onChange={(event) => setMinDuration(event.target.value)} />
        </label>
        <label className="field">
          <span>최대 실행 시간 <em>(초)</em></span>
          <input aria-label="최대 실행 시간(초)" type="number" min="0" max="2592000" step="1" value={maxDuration} onChange={(event) => setMaxDuration(event.target.value)} />
        </label>
      </div>

      {(error ?? historyError ?? activeSnapshotError ?? logError ?? searchError) ? <div className="error-banner" role="alert">오류: {error ?? historyError ?? activeSnapshotError ?? logError ?? searchError}</div> : null}
      {jobs.length === 0 ? <div className="empty-card compact"><p>먼저 작업 또는 서비스를 만들어 주세요.</p></div> : null}
      {jobs.length > 0 && !loading && runs.length === 0 ? <div className="empty-card compact"><p>조건에 맞는 실행 기록이 없습니다.</p></div> : null}

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
                    <form
                      className="log-search"
                      aria-label="로그 검색"
                      aria-busy={searchBusy}
                      onSubmit={(event) => {
                        event.preventDefault();
                        if ((event.nativeEvent as SubmitEvent & { isComposing?: boolean }).isComposing) return;
                        void handleSearch();
                      }}
                    >
                      <label className="field log-search-query">
                        <span>검색어</span>
                        <input
                          aria-label="로그 검색어"
                          type="search"
                          value={searchQuery}
                          maxLength={LOG_SEARCH_QUERY_BYTE_LIMIT}
                          onChange={(event) => {
                            if (utf8ByteLength(event.target.value) <= LOG_SEARCH_QUERY_BYTE_LIMIT) {
                              setSearchQuery(event.target.value);
                            }
                          }}
                          onKeyDown={(event) => {
                            if (event.key !== "Enter") return;
                            if (event.nativeEvent.isComposing || event.keyCode === 229) {
                              event.preventDefault();
                              return;
                            }
                            event.preventDefault();
                            void handleSearch();
                          }}
                          placeholder="로그에서 찾기"
                        />
                      </label>
                      <label className="field">
                        <span>방식</span>
                        <select aria-label="로그 검색 방식" value={searchMode} onChange={(event) => setSearchMode(event.target.value as LogSearchMode)}>
                          <option value="literal">일반 텍스트</option>
                          <option value="regex">정규식(명시적)</option>
                        </select>
                      </label>
                      <label className="field">
                        <span>소스</span>
                        <select aria-label="로그 검색 소스" value={searchSource} onChange={(event) => setSearchSource(event.target.value as LogStream | "")}>
                          <option value="">모든 스트림</option>
                          <option value="stdout">stdout</option>
                          <option value="stderr">stderr</option>
                        </select>
                      </label>
                      <label className="field">
                        <span>레벨</span>
                        <select aria-label="로그 검색 레벨" value={searchLevel} onChange={(event) => setSearchLevel(event.target.value as LogLevel | "")}>
                          <option value="">모든 레벨</option>
                          <option value="trace">trace</option>
                          <option value="debug">debug</option>
                          <option value="info">info</option>
                          <option value="warn">warn</option>
                          <option value="error">error</option>
                        </select>
                      </label>
                      <button type="submit" className="button-secondary" disabled={searchBusy || !searchQuery}>검색</button>
                      <button type="button" className="button-secondary" disabled={searchBusy && !searchResponse} onClick={clearSearch}>지우기</button>
                    </form>
                    {searchResponse ? (
                      <div className="log-search-results" aria-label="로그 검색 결과">
                        <div className="log-search-summary" role="status" aria-live="polite">
                          {searchResponse.matches.length === 0
                            ? "검색 결과가 없습니다."
                            : `${searchIndex + 1} / ${searchResponse.matches.length}개 결과`}
                          {activeSearchLineUnavailable ? " (현재 화면 범위 밖)" : ""}
                          {searchResponse.truncated ? " (검색 범위가 상한으로 제한됨)" : ""}
                        </div>
                        {searchResponse.matches.length > 0 ? (
                          <div className="log-search-navigation">
                            <button type="button" className="button-secondary" aria-label="이전 검색 결과" onClick={() => moveToSearchMatch(searchIndex - 1)}>이전</button>
                            <button type="button" className="button-secondary" aria-label="다음 검색 결과" onClick={() => moveToSearchMatch(searchIndex + 1)}>다음</button>
                            <ol className="log-search-match-list">
                              {searchResponse.matches.slice(0, LOG_SEARCH_MAX_RESULT_LINES).map((match, index) => (
                                <li key={`${match.sourceId}:${match.lineNumber}:${index}`}>
                                  <button
                                    type="button"
                                    className={index === searchIndex ? "active" : ""}
                                    aria-current={index === searchIndex ? "true" : undefined}
                                    onClick={() => moveToSearchMatch(index)}
                                  >
                                    {match.stream} · {match.lineNumber}번째 줄 · {levelLabel(match.level)}
                                  </button>
                                </li>
                              ))}
                            </ol>
                          </div>
                        ) : null}
                      </div>
                    ) : null}
                    <pre aria-label={`${stream} 로그`}>{renderLogText()}</pre>
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
