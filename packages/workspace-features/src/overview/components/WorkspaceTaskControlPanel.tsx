import { useCallback, useEffect, useRef, useState } from "react";
import {
  dispatchWorkspaceTaskControl,
  getWorkspaceTaskControlReceipt,
  listWorkspaceTaskControls,
  type WorkspaceTaskControl,
  type WorkspaceTaskControlAction,
  type WorkspaceTaskControlReceipt,
} from "../api";
import {
  canStartWorkspaceTask,
  canStopWorkspaceTask,
  isTerminalTaskControlReceipt,
  taskControlErrorMessage,
  taskControlReceiptMessage,
} from "../lib/taskControl";

const POLL_INTERVAL_MS = 500;
const MAX_POLL_MS = 10 * 60 * 1000;

interface PendingRequest {
  requestId: string;
  taskId: string;
  action: WorkspaceTaskControlAction;
  startedAt: number;
}

type RequestGuard = "dispatching" | { requestId: string };

interface Props {
  disabled?: boolean;
}

function taskAvailabilityLabel(task: WorkspaceTaskControl): string {
  if (task.operationActive) return "실행 중";
  if (!task.available) return "사용할 수 없음";
  if (!task.trusted) return "소스 승인 필요";
  if (task.taskKind === "shell" && !task.shellTrusted) return "셸 실행 승인 필요";
  return "실행 가능";
}

function taskKindLabel(task: WorkspaceTaskControl): string {
  return task.taskKind === "shell" ? "shell" : "process";
}

export default function WorkspaceTaskControlPanel({ disabled = false }: Props) {
  const [tasks, setTasks] = useState<WorkspaceTaskControl[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pendingByTask, setPendingByTask] = useState<Record<string, PendingRequest>>({});
  const [receipts, setReceipts] = useState<Record<string, WorkspaceTaskControlReceipt>>({});
  const [latestRequestByTask, setLatestRequestByTask] = useState<Record<string, string>>({});
  const [dispatchingRequest, setDispatchingRequest] = useState<{
    taskId: string;
    action: WorkspaceTaskControlAction;
  } | null>(null);
  const [requestGuardActive, setRequestGuardActive] = useState(false);
  const mountedRef = useRef(false);
  const refreshSequence = useRef(0);
  const timersRef = useRef<Map<string, number>>(new Map());
  // Run Manager exposes one confirmation slot. This ref is the synchronous
  // gate that prevents a second click before React commits the disabled state.
  const requestGuardRef = useRef<RequestGuard | null>(null);

  const clearPolling = useCallback((requestId: string) => {
    const timer = timersRef.current.get(requestId);
    if (timer !== undefined) window.clearTimeout(timer);
    timersRef.current.delete(requestId);
  }, []);

  const finishPending = useCallback((requestId: string) => {
    clearPolling(requestId);
    if (requestGuardRef.current && requestGuardRef.current !== "dispatching"
      && requestGuardRef.current.requestId === requestId) {
      requestGuardRef.current = null;
      if (mountedRef.current) setRequestGuardActive(false);
    }
    if (!mountedRef.current) return;
    setPendingByTask((previous) => {
      const entry = Object.entries(previous).find(([, value]) => value.requestId === requestId);
      if (!entry) return previous;
      const next = { ...previous };
      delete next[entry[0]];
      return next;
    });
  }, [clearPolling]);

  const refresh = useCallback(async () => {
    const request = ++refreshSequence.current;
    setLoading(true);
    setError(null);
    try {
      const nextTasks = await listWorkspaceTaskControls();
      if (!mountedRef.current || request !== refreshSequence.current) return;
      setTasks(nextTasks);
    } catch (cause) {
      if (mountedRef.current && request === refreshSequence.current) {
        setTasks([]);
        setError(taskControlErrorMessage(cause));
      }
    } finally {
      if (mountedRef.current && request === refreshSequence.current) setLoading(false);
    }
  }, []);

  const pollReceipt = useCallback((pending: PendingRequest) => {
    const pollStartedAt = pending.startedAt;
    const poll = async (): Promise<void> => {
      if (!mountedRef.current) return;
      if (Date.now() - pollStartedAt >= MAX_POLL_MS) {
        finishPending(pending.requestId);
        setError("Run Manager task 확인 결과를 제한 시간 안에 받지 못했습니다.");
        return;
      }
      try {
        const receipt = await getWorkspaceTaskControlReceipt(pending.requestId);
        if (!mountedRef.current) return;
        // A receipt is provenance for this request only. Do not render a
        // response with a mismatched correlator/task/action.
        if (receipt
          && receipt.requestId === pending.requestId
          && receipt.taskId === pending.taskId
          && receipt.action === pending.action) {
          setReceipts((previous) => ({ ...previous, [receipt.requestId]: receipt }));
          if (isTerminalTaskControlReceipt(receipt.status)) {
            if (receipt.status === "started" || receipt.status === "stopped") {
              setTasks((previous) => previous.map((task) => task.id === pending.taskId
                ? { ...task, operationActive: receipt.status === "started" }
                : task));
            }
            finishPending(pending.requestId);
            return;
          }
        }
      } catch (cause) {
        if (Date.now() - pollStartedAt >= MAX_POLL_MS) {
          finishPending(pending.requestId);
          setError(taskControlErrorMessage(cause));
          return;
        }
      }
      if (!mountedRef.current) return;
      if (Date.now() - pollStartedAt >= MAX_POLL_MS) {
        finishPending(pending.requestId);
        setError("Run Manager task 확인 결과를 제한 시간 안에 받지 못했습니다.");
        return;
      }
      const timer = window.setTimeout(() => {
        timersRef.current.delete(pending.requestId);
        void poll();
      }, POLL_INTERVAL_MS);
      timersRef.current.set(pending.requestId, timer);
    };
    void poll();
  }, [finishPending]);

  const dispatch = useCallback(async (task: WorkspaceTaskControl, action: WorkspaceTaskControlAction) => {
    // Check the ref before any await so two rapid clicks cannot enqueue two
    // handoffs even while the first dispatch is still in flight.
    if (disabled || requestGuardRef.current !== null || Object.keys(pendingByTask).length > 0) return;
    if (action === "start" && !canStartWorkspaceTask(task)) return;
    if (action === "stop" && !canStopWorkspaceTask(task)) return;
    requestGuardRef.current = "dispatching";
    setRequestGuardActive(true);
    setDispatchingRequest({ taskId: task.id, action });
    setError(null);
    try {
      const result = await dispatchWorkspaceTaskControl({
        taskId: task.id,
        action,
        expectedRevision: task.revision,
      });
      if (!mountedRef.current || !result.requestId || !result.handoffId) {
        if (mountedRef.current) {
          requestGuardRef.current = null;
          setRequestGuardActive(false);
          setDispatchingRequest(null);
          setError(taskControlErrorMessage("task-control-dispatch-invalid"));
        }
        return;
      }
      const pending: PendingRequest = {
        requestId: result.requestId,
        taskId: task.id,
        action,
        startedAt: Date.now(),
      };
      requestGuardRef.current = { requestId: result.requestId };
      setDispatchingRequest(null);
      setPendingByTask((previous) => ({ ...previous, [task.id]: pending }));
      setLatestRequestByTask((previous) => ({ ...previous, [task.id]: result.requestId }));
      pollReceipt(pending);
    } catch (cause) {
      if (mountedRef.current) {
        requestGuardRef.current = null;
        setRequestGuardActive(false);
        setDispatchingRequest(null);
        setError(taskControlErrorMessage(cause));
      }
    }
  }, [disabled, pendingByTask, pollReceipt]);

  useEffect(() => {
    mountedRef.current = true;
    void refresh();
    return () => {
      mountedRef.current = false;
      requestGuardRef.current = null;
      refreshSequence.current += 1;
      for (const timer of timersRef.current.values()) window.clearTimeout(timer);
      timersRef.current.clear();
    };
  }, [refresh]);

  return (
    <section
      className="panel workspace-task-panel"
      aria-labelledby="workspace-task-control-title"
      aria-busy={loading || requestGuardActive}
    >
      <div className="workspace-task-heading">
        <div>
          <h2 id="workspace-task-control-title">Run Manager 작업</h2>
          <p className="field-help">
            Run Manager의 승인된 task snapshot만 읽습니다. 시작/중지 요청은 Run Manager 창에서 확인한 뒤 실행됩니다.
          </p>
        </div>
        <button
          type="button"
          className="btn"
          disabled={disabled || loading}
          onClick={() => void refresh()}
          aria-label="Run Manager task snapshot 새로고침"
        >
          {loading ? "새로고침 중…" : "새로고침"}
        </button>
      </div>

      {error && <div className="field-error form-error" role="alert">{error}</div>}
      {dispatchingRequest && (
        <div className="workspace-task-pending" role="status" aria-live="polite">
          Run Manager task 요청을 전달하는 중입니다. 다른 시작/중지 요청은 잠시 막혀 있습니다.
        </div>
      )}
      {loading && tasks.length === 0 && <div className="dim" role="status">Run Manager task snapshot을 읽는 중…</div>}
      {!loading && !error && tasks.length === 0 && <div className="dim">동기화된 workspace task가 없습니다.</div>}
      {tasks.length > 0 && (
        <ul className="workspace-task-list" aria-label="Run Manager workspace task 목록">
          {tasks.map((task) => {
            const pending = pendingByTask[task.id];
            const latestRequestId = latestRequestByTask[task.id];
            const receipt = latestRequestId ? receipts[latestRequestId] : undefined;
            const startAllowed = canStartWorkspaceTask(task);
            const stopAllowed = canStopWorkspaceTask(task);
            const controlsLocked = disabled || loading || requestGuardActive;
            const statusText = pending
              ? receipt
                ? taskControlReceiptMessage(receipt)
                : "Run Manager 창의 확인을 기다리는 중…"
              : receipt
                ? taskControlReceiptMessage(receipt)
                : taskAvailabilityLabel(task);
            const terminalError = !pending && receipt
              && (receipt.status === "rejected" || receipt.status === "failed")
              ? taskControlReceiptMessage(receipt)
              : null;
            // Native snapshot validation limits IDs to ASCII alphanumeric and
            // `-_.:`. Keeping that already-safe opaque value preserves a
            // one-to-one ARIA relationship; replacing punctuation could make
            // distinct IDs such as `task:a` and `task.a` collide.
            const statusId = `workspace-task-status-${task.id}`;
            return (
              <li className="workspace-task-row" key={task.id}>
                <div className="workspace-task-row-heading">
                  <strong>{task.label}</strong>
                  <span className="workspace-task-kind">{taskKindLabel(task)}</span>
                </div>
                <div className="workspace-task-row-meta">
                  <span
                    id={statusId}
                    className={`workspace-task-state ${!task.available && !task.operationActive ? "bad" : ""}`}
                  >
                    {statusText}
                  </span>
                  {task.hasDependencies && <span className="workspace-task-dependency">dependency 포함</span>}
                </div>
                {terminalError && <div className="field-error" role="alert">{terminalError}</div>}
                {pending && (
                  <div className="workspace-task-pending" role="status" aria-live="polite">
                    요청을 전달했습니다. Run Manager 창의 확인 전에는 실행되지 않습니다.
                  </div>
                )}
                <div className="workspace-task-actions">
                  <button
                    type="button"
                    className="btn primary"
                    disabled={controlsLocked || Boolean(pending) || !startAllowed}
                    onClick={() => void dispatch(task, "start")}
                    aria-label={`${task.label} 시작`}
                    aria-describedby={statusId}
                    title={!startAllowed
                      ? task.operationActive
                        ? "이미 실행 중인 operation이 있습니다"
                        : "소스·셸 승인과 사용 가능 상태를 확인하세요"
                      : undefined}
                  >
                    시작
                  </button>
                  <button
                    type="button"
                    className="btn danger"
                    disabled={controlsLocked || Boolean(pending) || !stopAllowed}
                    onClick={() => void dispatch(task, "stop")}
                    aria-label={`${task.label} 중지`}
                    aria-describedby={statusId}
                    title={!stopAllowed ? "현재 실행 중인 operation이 없습니다" : undefined}
                  >
                    중지
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
