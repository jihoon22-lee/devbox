import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { RpcError } from "../../rpc/client";
import type { GitOperation, SourceTarget } from "../../rpc/gen/rpc";
import { useRpcQuery } from "../../rpc/query";
import { useRpc, useRpcSession } from "../../rpc/RpcProvider";
import { Banner, Button, ConfirmDialog } from "../../ui";
import { GitSnapshot } from "./GitSnapshot";
import { refreshGitConfirmation } from "./gitConfirmation";
import {
  gitError,
  operationMissing,
  retryUncertain,
  runGit,
  sourceTargetKey,
  updateGit,
  useSourceGit,
} from "./gitState";

const labels = {
  "git.push": "push",
  "git.pull": "pull",
  "git.discard": "변경 버리기",
  "git.branch_delete": "브랜치 삭제",
  "git.stash_drop": "stash 삭제",
  "git.cleanup": "작업 폴더 정리",
  "git.stage": "스테이징",
  "git.unstage": "스테이징 해제",
  "git.checkout": "브랜치 전환",
  "git.stash": "stash",
  "git.fetch": "fetch",
};
export function receiveOperation(operation: GitOperation, daemonId?: string) {
  const entry = useSourceGit.getState().entries[sourceTargetKey(operation.target)];
  const old = entry?.operation;
  if (daemonId !== undefined && entry?.operationDaemonId !== daemonId) return;
  if (entry?.lostOperation && operation.state === "running") return;
  if (!old || old.id !== operation.id || (old.state !== "running" && operation.state === "running")) return;
  updateGit(operation.target, {
    operation,
    notice:
      operation.state === "running"
        ? "Git 작업 실행 중…"
        : operation.state === "succeeded"
          ? "Git 작업을 마쳤습니다."
          : operation.state === "cancelled"
            ? "원격 작업을 취소했습니다."
            : "원격 작업을 마치지 못했습니다.",
    busy: !!entry?.lostOperation || operation.state === "running",
    ...(entry?.lostOperation ? { observed: undefined, lostOperation: { ...entry.lostOperation } } : {}),
    error: operation.error ? gitError(operation.error) : undefined,
  });
}
export function SourceGitSession() {
  const client = useRpc(),
    cache = useQueryClient();
  const entries = useSourceGit((state) => state.entries);
  useEffect(() => {
    const invalidate = (target: SourceTarget) => {
      void cache.invalidateQueries({
        predicate: (query) => {
          const [method, params] = query.queryKey;
          return (
            typeof method === "string" &&
            method.startsWith("git.") &&
            !!params &&
            typeof params === "object" &&
            "target" in params &&
            sourceTargetKey(params.target as SourceTarget) === sourceTargetKey(target)
          );
        },
      });
    };
    const offStore = useSourceGit.subscribe((now, before) => {
      for (const [key, entry] of Object.entries(now.entries))
        if (entry !== before.entries[key] && !entry.busy) {
          const [projectId, worktreeId] = JSON.parse(key) as [string, string | null];
          invalidate({ projectId, ...(worktreeId ? { worktreeId } : {}) });
        }
    });
    const offChanged = client.subscribe(
      "git.changed",
      (_rev, target) =>
        invalidate({ projectId: target.projectId, ...(target.worktreeId ? { worktreeId: target.worktreeId } : {}) }),
      () => {
        void cache.invalidateQueries({
          predicate: (q) => typeof q.queryKey[0] === "string" && q.queryKey[0].startsWith("git."),
        });
      },
    );
    const offOperation = client.subscribe(
      "git.operation_changed",
      (_rev, op) => receiveOperation(op, client.daemonId),
      () => {
        void cache.invalidateQueries({ queryKey: ["git.operation_status"] });
      },
    );
    return () => {
      offStore();
      offChanged();
      offOperation();
    };
  }, [client, cache]);
  return (
    <>
      {Object.entries(entries)
        .filter(([, entry]) => entry.operation?.state === "running" && !entry.lostOperation)
        .map(([key, entry]) => (
          <OperationWatch
            key={`${key}:${entry.operation!.id}`}
            operation={entry.operation!}
            daemonId={entry.operationDaemonId}
          />
        ))}
      <GitConfirmationDialog />
    </>
  );
}
function OperationWatch({ operation, daemonId }: { operation: GitOperation; daemonId?: string }) {
  const { client, state } = useRpcSession();
  const currentDaemon = client.daemonId;
  useEffect(() => {
    if (state === "ready" && daemonId !== currentDaemon) operationMissing(operation, daemonId);
  }, [operation, daemonId, currentDaemon, state]);
  const status = useRpcQuery(
    "git.operation_status",
    { target: operation.target, operationId: operation.id },
    { refetchInterval: 2000, staleTime: 0 },
  );
  useEffect(() => {
    if (status.data) receiveOperation(status.data, currentDaemon);
  }, [status.data, currentDaemon]);
  useEffect(() => {
    if (!status.error) return;
    if (status.error instanceof RpcError && status.error.code === "git.invalid_target")
      operationMissing(operation, daemonId);
    else {
      const current = useSourceGit.getState().entries[sourceTargetKey(operation.target)];
      if (
        current?.operation?.id === operation.id &&
        current.operation.state === "running" &&
        current.operationDaemonId === daemonId &&
        !current.lostOperation
      )
        updateGit(operation.target, { error: gitError(status.error) });
    }
  }, [status.error, operation, daemonId]);
  return null;
}
function GitConfirmationDialog() {
  const client = useRpc();
  const confirmation = useSourceGit((state) => state.confirmation);
  if (!confirmation) return null;
  const { action, id } = confirmation,
    { target } = action.params;
  const execute = async () => {
    if (useSourceGit.getState().confirmation?.id !== id || confirmation.busy || confirmation.error) return;
    useSourceGit.setState({ confirmation: { ...confirmation, busy: true } });
    const ok = await runGit(client, action, id);
    if (useSourceGit.getState().confirmation?.id !== id) return;
    if (ok) useSourceGit.setState({ confirmation: undefined });
    else
      useSourceGit.setState({
        confirmation: {
          ...confirmation,
          error: useSourceGit.getState().entries[sourceTargetKey(target)]?.error ?? "상태를 다시 확인하세요.",
        },
      });
  };
  return (
    <ConfirmDialog
      open
      title={`${labels[action.method]} 확인`}
      body={`프로젝트 ${target.projectId}${target.worktreeId ? ` · 작업 폴더 ${target.worktreeId}` : ""}의 아래 대상을 확인하세요.`}
      confirmLabel="확인 후 실행"
      disabled={confirmation.busy || !!confirmation.error}
      onCancel={() => {
        if (!confirmation.busy) useSourceGit.setState({ confirmation: undefined });
      }}
      onConfirm={() => void execute()}
    >
      {action.method === "git.push" && (
        <p>
          {action.params.forceWithLease
            ? "force-with-lease: 확인한 원격 커밋이 그대로일 때만 덮어씁니다."
            : "일반 push"}
        </p>
      )}
      {action.method === "git.pull" && <p>{action.params.rebase ? "rebase로 pull" : "merge로 pull"}</p>}
      <div style={{ overflowWrap: "anywhere", maxHeight: 300, overflow: "auto" }}>
        <GitSnapshot action={action} />
      </div>
      {confirmation.error && (
        <Banner tone="danger">
          {confirmation.error}
          {useSourceGit.getState().entries[sourceTargetKey(target)]?.uncertain ? (
            <Button disabled={confirmation.busy} onClick={() => void retryUncertain(client, target)}>
              같은 요청 결과 다시 확인
            </Button>
          ) : (
            <Button disabled={confirmation.busy} onClick={() => void refreshGitConfirmation(client, id)}>
              새 상태 확인
            </Button>
          )}
        </Banner>
      )}
    </ConfirmDialog>
  );
}
