import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import { create } from "zustand";
import { RpcError } from "../../rpc/client";
import type { Run } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { useRpcMutation } from "../../rpc/query";
import { useRpcSession } from "../../rpc/RpcProvider";
import { Banner, useToasts } from "../../ui";
import { isProjectTrustError, useProjectTrust } from "../projects/useProjectTrust";

interface UndoRequest {
  run: Run;
  client: object;
  generation: number;
}
const undoRequests = create<{ queue: UndoRequest[] }>(() => ({ queue: [] }));
export function requestRunStopUndo(request: UndoRequest) {
  undoRequests.setState((state) => ({ queue: [...state.queue, request] }));
}
/** Stay mounted in the shell so the ten-second undo survives section/project navigation. */
export function RunStopUndo() {
  const { client, generation, state } = useRpcSession();
  const current = undoRequests((value) => value.queue[0]);
  const restart = useRpcMutation("runtime.restart");
  const qc = useQueryClient();
  const trust = useProjectTrust(`run-stop-undo:${current?.run.id ?? ""}`);
  const running = useRef<UndoRequest | undefined>(undefined);
  const latest = useRef({ restart, trust });
  latest.current = { restart, trust };
  useEffect(() => {
    if (!current || running.current === current) return;
    running.current = current;
    const finish = () => {
      undoRequests.setState((value) => ({ queue: value.queue.filter((item) => item !== current) }));
      if (running.current === current) running.current = undefined;
    };
    if (current.client !== client || current.generation !== generation || state !== "ready") {
      useToasts
        .getState()
        .push({ tone: "warn", title: "연결이 바뀌어 중지를 되돌리지 않았습니다. 실행을 다시 확인하세요." });
      finish();
      return;
    }
    const attempt = () => latest.current.restart.mutateAsync({ runId: current.run.id });
    void (async () => {
      try {
        try {
          await attempt();
        } catch (error) {
          if (!(error instanceof RpcError) || !isProjectTrustError(error)) throw error;
          if (
            !(await latest.current.trust.ensure({
              id: current.run.projectId,
              operation: "run",
              worktrees: [],
              agentId: null,
              worktree: null,
            }))
          )
            return;
          await attempt();
        }
        await Promise.all([
          qc.invalidateQueries({ queryKey: ["runtime.list"] }),
          qc.invalidateQueries({ queryKey: ["runtime.get"] }),
        ]);
        useToasts.getState().push({ tone: "ok", title: `${current.run.defId} 다시 시작됨` });
      } catch (failure) {
        const error = failure instanceof RpcError ? failure : new RpcError("internal");
        useToasts.getState().push({ tone: "danger", title: messageFor(error.code, error.detail).title });
      } finally {
        finish();
      }
    })();
  }, [current, client, generation, state, qc]);
  return (
    <>
      {trust.notice && <Banner tone="warn">{trust.notice}</Banner>}
      {trust.dialog}
    </>
  );
}
