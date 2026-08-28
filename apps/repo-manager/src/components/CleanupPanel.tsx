import { useEffect, useRef, useState } from "react";
import {
  GIT_CLEANUP_BUSY,
  GIT_CLEANUP_CANCELLED,
  GIT_CLEANUP_ERROR,
  GIT_CLEANUP_STATE_CHANGED,
  repoCleanup,
  repoCleanupCancel,
  repoCleanupPreview,
  type BranchCleanupEntry,
  type CleanupPreview,
  type CleanupResult,
  type RepoEntry,
  type WorktreeCleanupEntry,
} from "../api";
import ConfirmDialog from "./ConfirmDialog";

interface Props {
  repo: RepoEntry | null;
}

const REASON_LABELS: Record<string, string> = {
  mergedIntoCurrent: "현재 branch에 이미 병합됨",
  upstreamGone: "upstream 추적 대상이 사라짐",
  inactive: "90일 이상 갱신되지 않음",
  primaryWorktree: "기본 worktree",
  linkedWorktree: "연결된 worktree",
  detachedWorktree: "detached worktree",
  bareWorktree: "bare worktree",
  prunableWorktree: "정리할 수 없는 stale worktree 기록",
};

const BLOCK_LABELS: Record<string, string> = {
  currentBranch: "현재 branch라서 차단됨",
  mainBranch: "main branch라서 차단됨",
  checkedOut: "worktree에서 사용 중이라 차단됨",
  mainWorktree: "기본 worktree라서 차단됨",
  currentWorktree: "현재 열려 있는 worktree라서 차단됨",
  locked: "locked worktree라서 차단됨",
  dirty: "커밋되지 않은 변경이 있어 차단됨",
  untracked: "untracked 파일이 있어 차단됨",
  ignored: "ignored 파일이 있어 차단됨",
  prunable: "stale worktree 기록은 자동 제거하지 않음",
  bareWorktree: "bare worktree는 이 흐름에서 제거하지 않음",
  stateUnavailable: "상태를 확인하지 못해 차단됨",
  gitFailed: "Git이 작업을 완료하지 못함",
  notCandidate: "정리 후보가 아님",
  notExecuted: "앞선 작업이 실패해 실행하지 않음",
};

function reasonLabel(reason: string): string {
  return REASON_LABELS[reason] ?? reason;
}

function blockLabel(reason: string): string {
  return BLOCK_LABELS[reason] ?? "안전 조건을 충족하지 않아 차단됨";
}

function outcomeLabel(outcome: CleanupResult["items"][number]["outcome"]): string {
  if (outcome === "removed") return "제거됨";
  if (outcome === "blocked") return "차단됨";
  return "실패";
}

function safeCleanupError(cause: unknown): string {
  const message = typeof cause === "string"
    ? cause
    : cause instanceof Error
      ? cause.message
      : "";
  return new Set([
    GIT_CLEANUP_ERROR,
    GIT_CLEANUP_CANCELLED,
    GIT_CLEANUP_BUSY,
    GIT_CLEANUP_STATE_CHANGED,
  ]).has(message)
    ? message
    : GIT_CLEANUP_ERROR;
}

export function createCleanupOperationId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `cleanup-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

function eligibleBranches(preview: CleanupPreview | null): BranchCleanupEntry[] {
  return (preview?.branches ?? []).filter((branch) => branch.eligible);
}

function eligibleWorktrees(preview: CleanupPreview | null): WorktreeCleanupEntry[] {
  return (preview?.worktrees ?? []).filter((worktree) => worktree.eligible);
}

function cleanupConfirmationSummary(pending: {
  branches: string[];
  worktrees: string[];
}): string[] {
  return [
    `local branch ${pending.branches.length}개와 worktree ${pending.worktrees.length}개를 정리합니다.`,
    "정리 대상:",
    ...pending.branches.map((name) => `branch ${name}`),
    ...pending.worktrees.map((path) => `worktree ${path}`),
    "최신 preview revision이 달라지거나 안전 조건이 바뀌면 전체 작업을 실행하지 않습니다.",
    "force delete·reset·clean은 사용하지 않으며, dirty·untracked·locked·main·현재 worktree는 차단됩니다.",
  ];
}

/** Preview-first branch/worktree cleanup with fail-closed selection and result handling. */
export default function CleanupPanel({ repo }: Props) {
  const [preview, setPreview] = useState<CleanupPreview | null>(null);
  const [selectedBranches, setSelectedBranches] = useState<Set<string>>(new Set());
  const [selectedWorktrees, setSelectedWorktrees] = useState<Set<string>>(new Set());
  const [result, setResult] = useState<CleanupResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [action, setAction] = useState<"preview" | "cleanup" | null>(null);
  const [cancelPending, setCancelPending] = useState(false);
  const [status, setStatus] = useState("정리 후보를 확인하면 안전 조건과 판단 근거를 표시합니다.");
  const [error, setError] = useState<string | null>(null);
  const [confirmation, setConfirmation] = useState<{
    repositoryKey: string;
    repositoryPath: string;
    revision: string;
    branches: string[];
    worktrees: string[];
  } | null>(null);
  const sequenceRef = useRef(0);
  const mountedRef = useRef(false);
  const busyRef = useRef(false);
  const operationIdRef = useRef<string | null>(null);
  const cancelledOperationRef = useRef<string | null>(null);

  useEffect(() => {
    mountedRef.current = true;
    sequenceRef.current += 1;
    busyRef.current = false;
    operationIdRef.current = null;
    cancelledOperationRef.current = null;
    setPreview(null);
    setSelectedBranches(new Set());
    setSelectedWorktrees(new Set());
    setResult(null);
    setBusy(false);
    setAction(null);
    setCancelPending(false);
    setConfirmation(null);
    setStatus("정리 후보를 확인하면 안전 조건과 판단 근거를 표시합니다.");
    setError(null);

    return () => {
      mountedRef.current = false;
      sequenceRef.current += 1;
      const operationId = operationIdRef.current;
      if (busyRef.current && operationId) {
        void repoCleanupCancel(operationId).catch(() => undefined);
      }
      busyRef.current = false;
      operationIdRef.current = null;
      cancelledOperationRef.current = null;
    };
  }, [repo?.canonicalKey, repo?.path]);

  if (!repo) return null;

  const isCurrent = (sequence: number) =>
    mountedRef.current && sequence === sequenceRef.current;

  const loadPreview = async () => {
    if (busyRef.current) return;
    const sequence = ++sequenceRef.current;
    const operationId = createCleanupOperationId();
    busyRef.current = true;
    operationIdRef.current = operationId;
    cancelledOperationRef.current = null;
    setBusy(true);
    setAction("preview");
    setError(null);
    setResult(null);
    setConfirmation(null);
    setStatus("정리 후보와 worktree 안전 상태를 확인하는 중입니다.");
    try {
      const next = await repoCleanupPreview(repo.path, operationId);
      if (!isCurrent(sequence)) return;
      setPreview(next);
      setSelectedBranches(new Set());
      setSelectedWorktrees(new Set());
      const branchCount = eligibleBranches(next).length;
      const worktreeCount = eligibleWorktrees(next).length;
      setStatus(`정리 후보 ${branchCount + worktreeCount}개를 확인했습니다. 차단 사유도 함께 표시됩니다.`);
    } catch (cause) {
      if (isCurrent(sequence)) {
        setPreview(null);
        setSelectedBranches(new Set());
        setSelectedWorktrees(new Set());
        setError(safeCleanupError(cause));
        setStatus("정리 후보를 확인하지 못했습니다.");
      }
    } finally {
      const cancelled = cancelledOperationRef.current === operationId;
      if (isCurrent(sequence) || (cancelled && mountedRef.current)) {
        if (cancelled) {
          cancelledOperationRef.current = null;
          setPreview(null);
          setSelectedBranches(new Set());
          setSelectedWorktrees(new Set());
          setConfirmation(null);
          setResult(null);
          setError(GIT_CLEANUP_CANCELLED);
          setStatus("정리 후보 검사를 취소했습니다. 최신 후보를 다시 검사하세요.");
        }
        operationIdRef.current = null;
        busyRef.current = false;
        setBusy(false);
        setAction(null);
      }
    }
  };

  const toggleBranch = (name: string) => {
    if (busyRef.current) return;
    setSelectedBranches((current) => {
      const next = new Set(current);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
    setError(null);
    setConfirmation(null);
  };

  const toggleWorktree = (path: string) => {
    if (busyRef.current) return;
    setSelectedWorktrees((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
    setError(null);
    setConfirmation(null);
  };

  const requestCleanup = () => {
    if (busyRef.current || !preview) return;
    const branches = [...selectedBranches];
    const worktrees = [...selectedWorktrees];
    if (branches.length + worktrees.length === 0) {
      setError("정리 후보를 하나 이상 선택하세요.");
      return;
    }
    setError(null);
    setConfirmation({
      repositoryKey: repo.canonicalKey,
      repositoryPath: repo.path,
      revision: preview.revision,
      branches,
      worktrees,
    });
  };

  const runCleanup = async (pending: NonNullable<typeof confirmation>) => {
    if (busyRef.current) return;
    if (
      !preview
      || pending.repositoryKey !== repo.canonicalKey
      || pending.repositoryPath !== repo.path
      || pending.revision !== preview.revision
      || pending.branches.length !== selectedBranches.size
      || pending.branches.some((name) => !selectedBranches.has(name))
      || pending.worktrees.length !== selectedWorktrees.size
      || pending.worktrees.some((path) => !selectedWorktrees.has(path))
    ) {
      setConfirmation(null);
      setPreview(null);
      setSelectedBranches(new Set());
      setSelectedWorktrees(new Set());
      setError(GIT_CLEANUP_STATE_CHANGED);
      setStatus("정리 상태가 바뀌었습니다. 최신 후보를 다시 검사하세요.");
      return;
    }

    const sequence = ++sequenceRef.current;
    const operationId = createCleanupOperationId();
    busyRef.current = true;
    operationIdRef.current = operationId;
    cancelledOperationRef.current = null;
    setBusy(true);
    setAction("cleanup");
    setCancelPending(false);
    setConfirmation(null);
    setError(null);
    setResult(null);
    setStatus("선택한 branch·worktree를 정리하는 중입니다.");
    try {
      const next = await repoCleanup(
        repo.path,
        pending.branches,
        pending.worktrees,
        pending.revision,
        operationId,
      );
      if (!isCurrent(sequence)) return;
      if (next.previewRevision !== pending.revision) {
        setPreview(null);
        setSelectedBranches(new Set());
        setSelectedWorktrees(new Set());
        setError(GIT_CLEANUP_STATE_CHANGED);
        setStatus("정리 결과가 승인한 preview와 달라졌습니다. 최신 후보를 다시 검사하세요.");
        return;
      }
      operationIdRef.current = null;
      setResult(next);
      setPreview(null);
      setSelectedBranches(new Set());
      setSelectedWorktrees(new Set());
      setStatus(`${next.removed}개 정리를 완료했습니다. 다시 후보를 확인하면 최신 상태를 읽습니다.`);
    } catch (cause) {
      if (isCurrent(sequence)) {
        // A failed/cancelled mutation may have partially observed or changed
        // repository state. Force an explicit fresh read before the next
        // destructive confirmation instead of retaining stale selections.
        setPreview(null);
        setSelectedBranches(new Set());
        setSelectedWorktrees(new Set());
        setError(safeCleanupError(cause));
        setStatus("정리 상태를 확정하지 못했습니다. 최신 후보를 다시 검사하세요.");
      }
    } finally {
      const cancelled = cancelledOperationRef.current === operationId;
      if (isCurrent(sequence) || (cancelled && mountedRef.current)) {
        if (cancelled) {
          cancelledOperationRef.current = null;
          setPreview(null);
          setSelectedBranches(new Set());
          setSelectedWorktrees(new Set());
          setConfirmation(null);
          setResult(null);
          setError(GIT_CLEANUP_CANCELLED);
          setStatus("정리 작업을 취소했습니다. 최신 후보를 다시 검사하세요.");
        }
        operationIdRef.current = null;
        busyRef.current = false;
        setBusy(false);
        setAction(null);
        setCancelPending(false);
      }
    }
  };

  const cancel = () => {
    const operationId = operationIdRef.current;
    if (!busyRef.current || !action || !operationId || cancelPending) return;
    cancelledOperationRef.current = operationId;
    sequenceRef.current += 1;
    setCancelPending(true);
    setStatus(action === "preview" ? "정리 후보 검사 취소를 요청하는 중입니다." : "정리 취소를 요청하는 중입니다.");
    void repoCleanupCancel(operationId)
      .then((accepted) => {
        if (!accepted && mountedRef.current && operationIdRef.current === operationId) {
          setStatus("취소 요청이 반영되기 전에 작업이 끝났을 수 있습니다.");
        }
      })
      .catch(() => {
        if (mountedRef.current && operationIdRef.current === operationId) {
          setStatus("취소 요청을 확인하지 못했습니다. 작업 결과를 기다리는 중입니다.");
        }
      });
  };

  const eligibleBranchEntries = eligibleBranches(preview);
  const eligibleWorktreeEntries = eligibleWorktrees(preview);
  const selectedCount = selectedBranches.size + selectedWorktrees.size;

  return (
    <section className="cleanup-panel" aria-label="Git branch·worktree 안전 정리" aria-busy={busy}>
      <div className="cleanup-panel-head">
        <div>
          <h2>Branch · worktree 안전 정리</h2>
          <div className="history-repository mono">{repo.path}</div>
        </div>
        <button type="button" className="btn" disabled={busy} onClick={() => void loadPreview()}>
          {busy && action === "preview" ? "검사 중…" : "정리 후보 검사"}
        </button>
        {busy && action ? (
          <button type="button" className="btn" disabled={cancelPending} onClick={cancel}>
            {cancelPending ? "취소 요청 중…" : action === "preview" ? "검사 취소" : "취소"}
          </button>
        ) : null}
      </div>

      {error ? <div className="error cleanup-error" role="alert">{error}</div> : null}
      <div className="cleanup-status" role="status" aria-live="polite" aria-atomic="true">
        {busy && action === "preview" && !cancelPending
          ? "정리 후보를 확인하는 중입니다."
          : busy && action === "cleanup" && !cancelPending
            ? "선택한 정리를 실행하는 중입니다."
            : status}
      </div>

      {preview ? (
        <>
          <div className="cleanup-summary">
            <span>branch 후보 {eligibleBranchEntries.length}개</span>
            <span>worktree 후보 {eligibleWorktreeEntries.length}개</span>
            <span>선택 {selectedCount}개</span>
          </div>
          <div className="cleanup-groups">
            <fieldset className="cleanup-group" aria-label="Branch cleanup candidates">
              <legend>Branch 후보</legend>
              {preview.branches.map((branch) => (
                <label className={`cleanup-row ${branch.eligible ? "" : "blocked"}`} key={branch.name}>
                  <input
                    type="checkbox"
                    aria-label={`branch ${branch.name}`}
                    checked={selectedBranches.has(branch.name)}
                    disabled={busy || !branch.eligible}
                    onChange={() => toggleBranch(branch.name)}
                  />
                  <span className="cleanup-target mono">{branch.name}</span>
                  <span className="cleanup-reasons">
                    {branch.reasons.map(reasonLabel).join(" · ") || "정리 후보 아님"}
                    {branch.blocked.length > 0 ? ` · ${branch.blocked.map(blockLabel).join(" · ")}` : ""}
                  </span>
                </label>
              ))}
              {preview.branches.length === 0 ? <div className="cleanup-empty dim">local branch가 없습니다.</div> : null}
            </fieldset>

            <fieldset className="cleanup-group" aria-label="Worktree cleanup candidates">
              <legend>Worktree 후보</legend>
              {preview.worktrees.map((worktree) => (
                <label className={`cleanup-row ${worktree.eligible ? "" : "blocked"}`} key={worktree.path}>
                  <input
                    type="checkbox"
                    aria-label={`worktree ${worktree.path}`}
                    checked={selectedWorktrees.has(worktree.path)}
                    disabled={busy || !worktree.eligible}
                    onChange={() => toggleWorktree(worktree.path)}
                  />
                  <span className="cleanup-target mono">{worktree.path}</span>
                  <span className="cleanup-reasons">
                    {worktree.reasons.map(reasonLabel).join(" · ")}
                    {worktree.blocked.length > 0 ? ` · ${worktree.blocked.map(blockLabel).join(" · ")}` : ""}
                  </span>
                </label>
              ))}
              {preview.worktrees.length === 0 ? <div className="cleanup-empty dim">worktree가 없습니다.</div> : null}
            </fieldset>
          </div>
          <div className="cleanup-footer">
            <span className="dim">main·현재 worktree·현재 branch·locked·dirty·untracked 대상은 항상 차단합니다. force delete/reset/clean은 실행하지 않습니다.</span>
            <button type="button" className="btn primary" disabled={busy || selectedCount === 0} onClick={requestCleanup}>
              선택 항목 정리 ({selectedCount})
            </button>
          </div>
        </>
      ) : null}

      {result ? (
        <div className="cleanup-result" role="status" aria-live="polite">
          <strong>정리 결과</strong>
          <span>{result.removed}개 제거 · {result.attempted}개 실행</span>
          {result.items.some((item) => item.outcome !== "removed") ? (
            <span>{result.items.filter((item) => item.outcome !== "removed").length}개 항목은 차단 또는 실패했습니다.</span>
          ) : null}
          <ul className="cleanup-result-items" aria-label="정리 결과 항목">
            {result.items.map((item) => (
              <li key={`${item.kind}:${item.target}`}>
                <span className="cleanup-target mono">{item.kind === "branch" ? "branch" : "worktree"} {item.target}</span>
                <span>{outcomeLabel(item.outcome)}</span>
                {item.reason ? <span className="dim">{blockLabel(item.reason)}</span> : null}
              </li>
            ))}
          </ul>
        </div>
      ) : null}

      {confirmation ? (
        <ConfirmDialog
          title="선택한 정리를 실행할까요?"
          summary={cleanupConfirmationSummary(confirmation)}
          confirmLabel="정리 실행"
          onCancel={() => setConfirmation(null)}
          onConfirm={() => void runCleanup(confirmation)}
        />
      ) : null}
    </section>
  );
}
