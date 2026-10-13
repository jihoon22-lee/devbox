import { useRef } from "react";
import { create } from "zustand";
import { messageFor } from "../../../rpc/messages";
import { useRpcMutation, useRpcQuery } from "../../../rpc/query";
import { useRpcSession } from "../../../rpc/RpcProvider";
import { Button, EmptyState, Tooltip } from "../../../ui";
import { TerminalView } from "../../terminal/TerminalView";
import type { AgentTask } from "../model";
import s from "./Detail.module.css";
import { QueryError } from "./QueryError";
export const useAgentRuns = create<{
  terminals: Record<string, string>;
  set: (id: string, terminalId: string) => void;
}>((set, get) => ({
  terminals: {},
  set: (id, terminalId) => set({ terminals: { ...get().terminals, [id]: terminalId } }),
}));
export function RunTab({ task }: { task: AgentTask }) {
  const id = useAgentRuns((state) => state.terminals[task.id]);
  const test = useRpcMutation("agents.test");
  const pending = useRef(false);
  const run = async () => {
    if (pending.current) return;
    pending.current = true;
    try {
      const result = await test.mutateAsync({ id: task.id });
      useAgentRuns.getState().set(task.id, result.terminalId);
    } catch {
      /* The typed error is shown below; retry preserves request identity. */
    } finally {
      pending.current = false;
    }
  };
  const { state } = useRpcSession();
  const config = useRpcQuery("projects.config", { id: task.projectId });
  const hasTest = !!config.data?.config.agent.test && !config.data.error;
  if (id)
    return (
      <div className={s.terminal}>
        <TerminalView id={id} readOnly={task.state === "closed"} />
      </div>
    );
  return (
    <>
      <QueryError query={config} />
      {test.error && (
        <p role="alert">
          {messageFor(test.error.code, test.error.detail).title} {messageFor(test.error.code, test.error.detail).action}
          {test.error.diagnosticId && ` · 진단 ${test.error.diagnosticId}`}
        </p>
      )}
      <EmptyState
        title="아직 테스트를 실행하지 않았습니다"
        action={
          task.state !== "closed" && (
            <Tooltip
              content={hasTest ? "설정한 테스트를 실행합니다" : ".devbox/devbox.toml의 [agent] test를 설정하세요"}
            >
              <span>
                <Button disabled={!hasTest || state !== "ready" || test.isPending} onClick={() => void run()}>
                  테스트 실행
                </Button>
              </span>
            </Tooltip>
          )
        }
      />
    </>
  );
}
