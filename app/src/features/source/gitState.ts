import { create } from "zustand";
import { type RpcClient, RpcError } from "../../rpc/client";
import type {
  CleanupCandidate,
  GitCommitParams,
  GitOperation,
  Methods,
  SourceStatus,
  SourceTarget,
} from "../../rpc/gen/rpc";
import { RPC_ERROR_CODES } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
export type GitMethod =
  | "git.stage"
  | "git.unstage"
  | "git.discard"
  | "git.checkout"
  | "git.branch_delete"
  | "git.stash"
  | "git.stash_drop"
  | "git.cleanup"
  | "git.fetch"
  | "git.push"
  | "git.pull";
export type GitAction = { [M in GitMethod]: { method: M; params: Methods[M]["params"] } }[GitMethod];
export type GitAttempt = {
  action: GitAction | { method: "git.commit"; params: GitCommitParams; push: boolean };
  requestId: string;
  daemonId?: string;
};
export type GitEntry = {
  busy: boolean;
  notice?: string;
  error?: string;
  committed?: boolean;
  needsPush?: boolean;
  operation?: GitOperation;
  operationDaemonId?: string;
  lostOperation?: { id: string; daemonId?: string };
  uncertain?: GitAttempt;
  observed?: SourceStatus;
  cleanupFailure?: CleanupCandidate[];
};
export type GitConfirmation = { id: string; action: GitAction; error?: string; busy?: boolean };
export const useSourceGit = create<{ entries: Record<string, GitEntry>; confirmation?: GitConfirmation }>(() => ({
  entries: {},
}));
export function sourceTargetKey(target: SourceTarget) {
  return JSON.stringify([target.projectId, target.worktreeId ?? null]);
}
export function gitError(error: unknown) {
  if (!(error instanceof RpcError)) {
    if (error && typeof error === "object" && "code" in error && typeof error.code === "string") {
      const code = RPC_ERROR_CODES.find((code) => code === error.code);
      if (!code) return "Git 작업을 마치지 못했습니다. 상태를 다시 확인하세요.";
      const message = messageFor(code, "detail" in error ? error.detail : undefined);
      return `${message.title} ${message.action ?? ""}`;
    }
    return "Git 작업을 마치지 못했습니다. 상태를 다시 확인하세요.";
  }
  if (error.code === "connection_lost")
    return "연결이 끊겨 완료 여부를 알 수 없습니다. Git 상태를 확인한 뒤 다음 작업을 진행하세요.";
  const message = messageFor(error.code, error.detail);
  return `${message.title} ${message.action ?? ""}`;
}
export function updateGit(target: SourceTarget, patch: Partial<GitEntry>) {
  const key = sourceTargetKey(target);
  useSourceGit.setState((state) => ({
    entries: { ...state.entries, [key]: { busy: false, ...state.entries[key], ...patch } },
  }));
}
export function requestGit(action: GitAction) {
  if (
    useSourceGit.getState().confirmation ||
    useSourceGit.getState().entries[sourceTargetKey(action.params.target)]?.busy ||
    useSourceGit.getState().entries[sourceTargetKey(action.params.target)]?.uncertain
  )
    return;
  useSourceGit.setState({ confirmation: { id: crypto.randomUUID(), action: structuredClone(action) } });
}
export async function runGit(client: RpcClient, input: GitAction, requestId: string = crypto.randomUUID()) {
  if (client.state !== "ready") {
    updateGit(input.params.target, { error: "데몬에 다시 연결한 뒤 실행하세요." });
    return false;
  }
  const action = structuredClone(input),
    target = action.params.target;
  const previous = useSourceGit.getState().entries[sourceTargetKey(target)];
  if (previous?.busy || (previous?.uncertain && previous.uncertain.requestId !== requestId)) return false;
  const attempt: GitAttempt = { action, requestId, daemonId: client.daemonId };
  updateGit(target, { busy: true, error: undefined, notice: undefined, observed: undefined });
  try {
    const result = await client.call(action.method, action.params, { clientRequestId: requestId });
    updateGit(target, {
      uncertain: undefined,
      ...(action.method === "git.push" ? { needsPush: false } : {}),
      ...(action.method === "git.cleanup" ? { cleanupFailure: undefined } : {}),
      busy: "id" in result && result.state === "running",
      notice: "id" in result && result.state === "running" ? "Git 작업 실행 중…" : "Git 작업을 마쳤습니다.",
      ...("id" in result ? { operation: result, operationDaemonId: attempt.daemonId, lostOperation: undefined } : {}),
    });
    return true;
  } catch (error) {
    updateGit(target, {
      busy: false,
      error: gitError(error),
      uncertain: error instanceof RpcError && error.code === "connection_lost" ? attempt : undefined,
      ...(action.method === "git.cleanup" ? { cleanupFailure: action.params.candidates } : {}),
    });
    return false;
  }
}
/** Always read the post-action snapshot. A failed read must never replay the preceding commit. */
export async function prepareRemote(
  client: RpcClient,
  input: SourceTarget,
  method: "git.push" | "git.pull",
  option: boolean,
) {
  if (client.state !== "ready") {
    updateGit(input, { error: "데몬에 다시 연결한 뒤 실행하세요." });
    return;
  }
  const target = structuredClone(input);
  if (useSourceGit.getState().entries[sourceTargetKey(target)]?.busy) return;
  updateGit(target, { busy: true, error: undefined });
  try {
    const status = await client.call("git.status", { target });
    updateGit(target, { busy: false });
    requestGit(
      method === "git.push"
        ? { method, params: { target, expect: status.expect.state, forceWithLease: option } }
        : { method, params: { target, expect: status.expect.state, rebase: option } },
    );
  } catch (error) {
    updateGit(target, { busy: false, error: gitError(error) });
  }
}
export async function commitSource(
  client: RpcClient,
  input: GitCommitParams,
  push: boolean,
  requestId: string = crypto.randomUUID(),
) {
  if (client.state !== "ready") {
    updateGit(input.target, { error: "데몬에 다시 연결한 뒤 실행하세요." });
    return;
  }
  const params = structuredClone(input),
    target = params.target;
  const previous = useSourceGit.getState().entries[sourceTargetKey(target)];
  if (previous?.busy || (previous?.uncertain && previous.uncertain.requestId !== requestId)) return;
  const attempt: GitAttempt = { action: { method: "git.commit", params, push }, requestId, daemonId: client.daemonId };
  updateGit(target, { busy: true, error: undefined, notice: undefined, committed: false, needsPush: false });
  try {
    await client.call("git.commit", params, { clientRequestId: requestId });
  } catch (error) {
    updateGit(target, {
      busy: false,
      error: gitError(error),
      uncertain: error instanceof RpcError && error.code === "connection_lost" ? attempt : undefined,
    });
    return;
  }
  updateGit(target, {
    busy: false,
    uncertain: undefined,
    committed: true,
    needsPush: push,
    notice: "커밋을 완료했습니다.",
  });
  if (push) await prepareRemote(client, target, "git.push", false);
}

export async function retryUncertain(client: RpcClient, target: SourceTarget) {
  const entry = useSourceGit.getState().entries[sourceTargetKey(target)],
    attempt = entry?.uncertain;
  if (!attempt || entry.busy) return;
  if (attempt.daemonId !== client.daemonId) {
    updateGit(target, { error: "데몬이 바뀌어 같은 요청을 다시 보낼 수 없습니다. 현재 Git 상태를 확인하세요." });
    return;
  }
  if (attempt.action.method === "git.commit")
    await commitSource(client, attempt.action.params, attempt.action.push, attempt.requestId);
  else if (await runGit(client, attempt.action, attempt.requestId)) {
    if (useSourceGit.getState().confirmation?.id === attempt.requestId)
      useSourceGit.setState({ confirmation: undefined });
  }
}
export async function inspectUncertain(client: RpcClient, target: SourceTarget) {
  const entry = useSourceGit.getState().entries[sourceTargetKey(target)];
  const uncertain = entry?.uncertain ?? entry?.lostOperation;
  if (!uncertain) return;
  try {
    const observed = await client.call("git.status", { target });
    const current = useSourceGit.getState().entries[sourceTargetKey(target)];
    if ((current?.uncertain ?? current?.lostOperation) === uncertain) updateGit(target, { observed });
  } catch (error) {
    updateGit(target, { error: gitError(error) });
  }
}

/** A forgotten daemon job has no safe mutation to retry; retain its identity until state reconciliation. */
export function operationMissing(operation: GitOperation, daemonId?: string) {
  const entry = useSourceGit.getState().entries[sourceTargetKey(operation.target)];
  if (
    !entry ||
    entry.operation?.id !== operation.id ||
    entry.operation.state !== "running" ||
    entry.operationDaemonId !== daemonId ||
    entry.lostOperation
  )
    return;
  updateGit(operation.target, {
    busy: true,
    operation: { ...entry.operation, completionUnknown: true },
    lostOperation: { id: operation.id, daemonId },
    observed: undefined,
    error: undefined,
    notice: "데몬에서 작업을 찾을 수 없어 완료 여부를 알 수 없습니다. 현재 Git 상태를 읽고 확인하세요.",
  });
}
