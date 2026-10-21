import { useQueryClient } from "@tanstack/react-query";
import { useRef, useState } from "react";
import { RpcError } from "../../rpc/client";
import type { ContainerInfo, Definition, Run } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { useRpcMutation } from "../../rpc/query";
import { useRpcSession } from "../../rpc/RpcProvider";
import { Banner, ConfirmDialog, useToasts } from "../../ui";
import { isProjectTrustError, useProjectTrust } from "../projects/useProjectTrust";
import { requestRunStopUndo } from "./RunStopUndo";
import { liveRun } from "./runPresentation";
export function useRunActions(projectId: string, onRun: (run: Run) => void) {
  const qc = useQueryClient();
  const { client, generation } = useRpcSession();
  const start = useRpcMutation("runtime.start"),
    stop = useRpcMutation("runtime.stop"),
    restart = useRpcMutation("runtime.restart");
  const container = useRpcMutation("runtime.container_action");
  const trust = useProjectTrust(`runs:${projectId}`);
  const [error, setError] = useState<RpcError>();
  const [busy, setBusy] = useState(false);
  const active = useRef(false);
  const [confirm, setConfirm] = useState<Run>();
  const refresh = () =>
    Promise.all([
      qc.invalidateQueries({ queryKey: ["runtime.list"] }),
      qc.invalidateQueries({ queryKey: ["runtime.get"] }),
      qc.invalidateQueries({ queryKey: ["runtime.schedules"] }),
    ]);
  const withTrust = async (operation: () => Promise<Run>) => {
    try {
      return await operation();
    } catch (error) {
      if (!(error instanceof RpcError) || !isProjectTrustError(error)) throw error;
      if (!(await trust.ensure({ id: projectId, operation: "run", worktrees: [], agentId: null, worktree: null })))
        return undefined;
      // Exactly one retry after this approval. A second stale result is visible.
      return await operation();
    }
  };
  const perform = async (operation: () => Promise<void>) => {
    if (active.current) return;
    active.current = true;
    setBusy(true);
    setError(undefined);
    try {
      await operation();
    } catch (failure) {
      setError(failure instanceof RpcError ? failure : new RpcError("internal"));
    } finally {
      active.current = false;
      setBusy(false);
    }
  };
  const restartRun = (run: Run) =>
    perform(async () => {
      const result = await withTrust(() => restart.mutateAsync({ runId: run.id }));
      if (result) {
        onRun(result);
        await refresh();
      }
    });
  const stopRun = (run: Run) =>
    perform(async () => {
      const result = await stop.mutateAsync({ runId: run.id });
      onRun(result);
      useToasts.getState().push({
        tone: "info",
        title: `${run.defId} 중지됨`,
        ...(run.kind === "service" && liveRun(run)
          ? { undo: () => requestRunStopUndo({ run: result, client, generation }) }
          : {}),
      });
      await refresh();
    });
  const action = (kind: "start" | "stop" | "restart", definition: Definition, run?: Run) => {
    if (kind === "stop" && run) {
      if (run.kind !== "service") setConfirm(run);
      else void stopRun(run);
    } else if (kind === "restart" && run) void restartRun(run);
    else if (kind === "start")
      void perform(async () => {
        const id = definition.def.kind === "schedule" ? definition.def.task : definition.def.id;
        const result = await withTrust(() => start.mutateAsync({ projectId, id, worktree: null }));
        if (result) {
          onRun(result);
          await refresh();
        }
      });
  };
  const containerAction = (kind: "start" | "stop" | "restart", row: ContainerInfo) =>
    void perform(async () => {
      if (kind === "stop" && row.state !== "running") return;
      const result = await container.mutateAsync({ id: row.id, action: kind });
      await qc.invalidateQueries({ queryKey: ["runtime.containers"] });
      useToasts.getState().push({
        tone: "info",
        title: `${row.name} ${kind === "stop" ? "중지됨" : kind === "start" ? "시작됨" : "재시작됨"}`,
        ...(result.undo && kind === "stop"
          ? {
              undo: () =>
                void perform(async () => {
                  await container.mutateAsync(result.undo!);
                  await qc.invalidateQueries({ queryKey: ["runtime.containers"] });
                }),
            }
          : {}),
      });
    });
  return {
    action,
    containerAction,
    busy: busy || trust.pending,
    feedback: (
      <>
        {error && <Banner tone="danger">{messageFor(error.code, error.detail).title}</Banner>}
        {trust.notice && <Banner tone="warn">{trust.notice}</Banner>}
        {trust.dialog}
        {confirm && (
          <ConfirmDialog
            open
            title="작업 중지"
            body="실행 중인 작업을 중지합니다. 작업 중지는 되돌릴 수 없습니다."
            confirmLabel="중지"
            onCancel={() => setConfirm(undefined)}
            onConfirm={() => {
              const run = confirm;
              setConfirm(undefined);
              void stopRun(run);
            }}
          />
        )}
      </>
    ),
  };
}
