import { useQueryClient } from "@tanstack/react-query";
import { AGENT_KEYS } from "../AgentData";
import type { AgentTask } from "../model";
/** Individual task replies have no list revision: seed only their own query. */
export function useAgentRefresh() {
  const qc = useQueryClient();
  return () => {
    for (const queryKey of AGENT_KEYS) void qc.invalidateQueries({ queryKey }).catch(() => {});
  };
}
export function useAgentResult() {
  const qc = useQueryClient();
  const refresh = useAgentRefresh();
  return (task: AgentTask) => {
    void qc.cancelQueries({ queryKey: ["agents.get", { id: task.id }] }).catch(() => {});
    qc.setQueryData(["agents.get", { id: task.id }], task);
    refresh();
  };
}
