import { useEffect, useRef, useState } from "react";
import {
  GIT_REMOTE_BUSY,
  GIT_REMOTE_CANCELLED,
  GIT_REMOTE_ERROR,
  GIT_REMOTE_STATE_CHANGED,
  repoFetch,
  repoPull,
  repoPush,
  repoRemoteCancel,
  repoRemoteStatus,
  type RemoteState,
  type RepoEntry,
} from "../api";

interface Props {
  repo: RepoEntry | null;
}

type RemoteAction = "fetch" | "pull" | "push";

const ACTION_LABELS: Record<RemoteAction, string> = {
  fetch: "Fetch",
  pull: "Pull (FF only)",
  push: "Push",
};

const FIXED_REMOTE_ERRORS = new Set([
  GIT_REMOTE_ERROR,
  GIT_REMOTE_CANCELLED,
  GIT_REMOTE_BUSY,
  GIT_REMOTE_STATE_CHANGED,
  "현재 HEAD가 detached 상태라 pull/push를 실행할 수 없습니다.",
  "현재 branch에 upstream이 없어 pull/push를 실행할 수 없습니다.",
  "working tree에 변경 사항이 있어 pull/push를 실행할 수 없습니다.",
  "branch가 diverged 상태라 fast-forward pull/push를 실행할 수 없습니다.",
  "다른 Git 작업 또는 merge/rebase가 진행 중이라 원격 작업을 실행할 수 없습니다.",
]);

function safeRemoteError(error: unknown): string {
  const message = typeof error === "string"
    ? error
    : error instanceof Error
      ? error.message
      : "";
  return FIXED_REMOTE_ERRORS.has(message) ? message : GIT_REMOTE_ERROR;
}

function blockedReason(state: RemoteState, action: RemoteAction): string | null {
  if (state.operationInProgress) {
    return "다른 Git 작업 또는 merge/rebase가 진행 중이라 원격 작업을 실행할 수 없습니다.";
  }
  if (action === "fetch") return null;
  if (state.detached || !state.currentBranch) {
    return "현재 HEAD가 detached 상태라 pull/push를 실행할 수 없습니다.";
  }
  if (!state.upstream) {
    return "현재 branch에 upstream이 없어 pull/push를 실행할 수 없습니다.";
  }
  if (state.dirty) {
    return "working tree에 변경 사항이 있어 pull/push를 실행할 수 없습니다.";
  }
  if (state.diverged || (action === "push" && state.behind > 0)) {
    return "branch가 diverged 상태라 fast-forward pull/push를 실행할 수 없습니다.";
  }
  return null;
}

function stateSummary(state: RemoteState | null): string {
  if (!state) return "원격 상태를 확인하는 중입니다.";
  if (state.operationInProgress) {
    return "다른 Git 작업 또는 merge/rebase가 진행 중이라 원격 작업을 실행할 수 없습니다.";
  }
  if (state.detached) return "현재 HEAD가 detached 상태라 pull/push를 실행할 수 없습니다.";
  if (!state.upstream) return "현재 branch에 upstream이 없어 pull/push를 실행할 수 없습니다.";
  if (state.dirty) return "working tree에 변경 사항이 있어 pull/push를 실행할 수 없습니다.";
  if (state.diverged) return "branch가 diverged 상태라 fast-forward pull/push를 실행할 수 없습니다.";
  return "원격 작업을 실행할 수 있습니다.";
}

function createRemoteOperationId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `remote-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

/** Bounded fetch/FF-only pull/current-branch push surface for one repository. */
export default function RemoteSyncPanel({ repo }: Props) {
  const [state, setState] = useState<RemoteState | null>(null);
  const [busy, setBusy] = useState(false);
  const [action, setAction] = useState<RemoteAction | null>(null);
  const [status, setStatus] = useState("원격 상태를 불러오면 fetch·pull·push를 실행할 수 있습니다.");
  const [error, setError] = useState<string | null>(null);
  const sequenceRef = useRef(0);
  const busyRef = useRef(false);
  const mountedRef = useRef(false);
  const operationIdRef = useRef<string | null>(null);
  const [cancelPending, setCancelPending] = useState(false);

  useEffect(() => {
    mountedRef.current = true;
    sequenceRef.current += 1;
    busyRef.current = false;
    operationIdRef.current = null;
    setCancelPending(false);
    setState(null);
    setBusy(false);
    setAction(null);
    setStatus("원격 상태를 불러오면 fetch·pull·push를 실행할 수 있습니다.");
    setError(null);

    return () => {
      mountedRef.current = false;
      sequenceRef.current += 1;
      const operationId = operationIdRef.current;
      if (busyRef.current && operationId) {
        void repoRemoteCancel(operationId).catch(() => undefined);
      }
      busyRef.current = false;
    };
  }, [repo?.canonicalKey, repo?.path]);

  if (!repo) return null;

  const isCurrent = (sequence: number) => mountedRef.current && sequence === sequenceRef.current;

  const refresh = () => {
    if (busyRef.current) return;
    const sequence = ++sequenceRef.current;
    busyRef.current = true;
    setBusy(true);
    setAction(null);
    setState(null);
    setError(null);
    setStatus("원격 상태를 확인하는 중입니다.");
    void Promise.resolve()
      .then(() => repoRemoteStatus(repo.path))
      .then((next) => {
        if (!isCurrent(sequence)) return;
        setState(next);
        setStatus(stateSummary(next));
      })
      .catch((reason) => {
        if (isCurrent(sequence)) {
          setState(null);
          setError(safeRemoteError(reason));
          setStatus("원격 상태를 확인하지 못했습니다.");
        }
      })
      .finally(() => {
        if (isCurrent(sequence)) {
          busyRef.current = false;
          setBusy(false);
        }
      });
  };

  const runAction = (nextAction: RemoteAction) => {
    if (busyRef.current || !state) return;
    const reason = blockedReason(state, nextAction);
    if (reason) {
      setError(reason);
      return;
    }

    const sequence = ++sequenceRef.current;
    const path = repo.path;
    const operationId = createRemoteOperationId();
    busyRef.current = true;
    operationIdRef.current = operationId;
    setCancelPending(false);
    setBusy(true);
    setAction(nextAction);
    setState(null);
    setError(null);
    setStatus(`${ACTION_LABELS[nextAction]} 실행 중입니다.`);

    let operation: Promise<void>;
    try {
      operation = nextAction === "fetch"
        ? repoFetch(path, operationId)
        : nextAction === "pull"
          ? repoPull(path, operationId)
          : repoPush(path, operationId);
    } catch (reason) {
      if (isCurrent(sequence)) {
        setError(safeRemoteError(reason));
        setStatus("원격 작업을 완료하지 못했습니다.");
        operationIdRef.current = null;
        busyRef.current = false;
        setBusy(false);
        setAction(null);
      }
      return;
    }

    void operation
      .then(async () => {
        if (!isCurrent(sequence)) return;
        setStatus(`${ACTION_LABELS[nextAction]} 완료.`);
        try {
          const refreshed = await repoRemoteStatus(path);
          if (isCurrent(sequence)) {
            setState(refreshed);
            setStatus(`${ACTION_LABELS[nextAction]} 완료. ${stateSummary(refreshed)}`);
          }
        } catch {
          // The remote mutation succeeded, but the old snapshot is unsafe to
          // use after a failed refresh. Clear it so every next mutation is
          // disabled until the user obtains a fresh native state.
          if (isCurrent(sequence)) {
            setState(null);
            setError(GIT_REMOTE_ERROR);
            setStatus("원격 작업은 완료됐지만 상태를 확인하지 못해 다음 작업을 차단했습니다.");
          }
        }
      })
      .catch((reason) => {
        if (!isCurrent(sequence)) return;
        setState(null);
        setError(safeRemoteError(reason));
        setStatus("원격 작업을 완료하지 못했습니다.");
      })
      .finally(() => {
        if (isCurrent(sequence)) {
          if (operationIdRef.current === operationId) operationIdRef.current = null;
          setCancelPending(false);
          busyRef.current = false;
          setBusy(false);
          setAction(null);
        }
      });
  };

  const cancel = () => {
    const operationId = operationIdRef.current;
    if (!busyRef.current || !action || !operationId || cancelPending) return;
    setCancelPending(true);
    setStatus("취소 요청 중입니다.");
    void repoRemoteCancel(operationId)
      .then((accepted) => {
        if (!accepted && mountedRef.current && operationIdRef.current === operationId) {
          setStatus("취소 요청이 반영되기 전에 완료되었을 수 있습니다.");
        }
      })
      .catch(() => {
        if (mountedRef.current && operationIdRef.current === operationId) {
          setStatus("취소 요청을 확인하지 못했습니다. 작업 결과를 기다리는 중입니다.");
        }
      });
  };

  const pullBlocked = state ? blockedReason(state, "pull") !== null : true;
  const pushBlocked = state ? blockedReason(state, "push") !== null : true;

  return (
    <section className="remote-sync-panel" aria-label="Git remote sync" aria-busy={busy}>
      <div className="remote-sync-head">
        <div>
          <h2>Remote sync</h2>
          <div className="history-repository mono">{repo.path}</div>
        </div>
        <div className="remote-sync-actions">
          <button type="button" className="btn" disabled={busy} onClick={refresh}>
            {busy && !action ? "확인 중…" : "원격 상태 새로고침"}
          </button>
          {busy && action ? (
            <button type="button" className="btn" disabled={cancelPending} onClick={cancel}>
              {cancelPending ? "취소 요청 중…" : "취소"}
            </button>
          ) : null}
        </div>
      </div>

      {error ? <div className="error remote-sync-error" role="alert">{error}</div> : null}
      <div className="remote-sync-status" role="status" aria-live="polite" aria-atomic="true">
        {busy && action && !cancelPending ? `${ACTION_LABELS[action]} 처리 중입니다.` : status}
      </div>

      <dl className="remote-state-grid">
        <div><dt>branch</dt><dd className="mono">{state?.detached ? "(detached)" : state?.currentBranch ?? "—"}</dd></div>
        <div><dt>upstream</dt><dd className="mono">{state?.upstream ?? "없음"}</dd></div>
        <div><dt>ahead / behind</dt><dd className="mono">{state ? `${state.ahead} / ${state.behind}` : "—"}</dd></div>
        <div><dt>working tree</dt><dd>{state ? (state.dirty ? "dirty" : "clean") : "—"}</dd></div>
        <div><dt>operation</dt><dd>{state?.operationInProgress ? "진행 중" : "없음"}</dd></div>
      </dl>

      <div className="remote-sync-buttons" aria-label="Git remote actions">
        <button
          type="button"
          className="btn primary"
          disabled={busy || !state || Boolean(state.operationInProgress)}
          onClick={() => runAction("fetch")}
        >
          Fetch
        </button>
        <button
          type="button"
          className="btn primary"
          disabled={busy || pullBlocked}
          title={state ? blockedReason(state, "pull") ?? "fast-forward-only pull" : undefined}
          onClick={() => runAction("pull")}
        >
          Pull (FF only)
        </button>
        <button
          type="button"
          className="btn primary"
          disabled={busy || pushBlocked}
          title={state ? blockedReason(state, "push") ?? "current branch push" : undefined}
          onClick={() => runAction("push")}
        >
          Push
        </button>
      </div>
      <div className="remote-sync-help dim">
        Pull은 fast-forward만 허용하며 force push·merge/rebase 자동화는 제공하지 않습니다.
      </div>
    </section>
  );
}
