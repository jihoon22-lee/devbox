import { createContext, type ReactNode, useContext } from "react";
import { useRpcQuery, useTopicInvalidation } from "../../rpc/query";
import { useVisibleClock } from "../../shell/visibility";
import { attentionCount, groupByUrgency, hookMissing } from "./model";
export const AGENT_KEYS = [
  ["agents.list", { includeClosedHours: 24 }],
  ["agents.git_stats", {}],
  ["agents.get"],
  ["agents.changes"],
  ["agents.commits"],
  ["agents.patch"],
  ["agents.usage"],
  ["agents.discard_check"],
];
const Context = createContext<ReturnType<typeof useSharedAgents> | null>(null);
function useSharedAgents() {
  const list = useRpcQuery("agents.list", { includeClosedHours: 24 });
  const profiles = useRpcQuery("agents.profiles", {});
  useTopicInvalidation("agents.changed", AGENT_KEYS);
  const now = useVisibleClock();
  const tasks = list.data?.items ?? [];
  return {
    list,
    profiles,
    tasks,
    now,
    groups: groupByUrgency(tasks, now),
    attention: attentionCount(tasks),
    waiting: tasks.filter((t) => t.state === "waiting" || t.state === "idle").length,
    failed: tasks.some((t) => t.state === "failed"),
    hookMissing: hookMissing(tasks, profiles.data?.items ?? [], now),
  };
}
/** Shell owns this once; badges, status and navigation share its live list and subscription. */
export function AgentDataProvider({ children }: { children: ReactNode }) {
  const data = useSharedAgents();
  return <Context.Provider value={data}>{children}</Context.Provider>;
}
export function useAgentSummary() {
  return useContext(Context);
}
export function useAgentData() {
  const data = useAgentSummary();
  if (!data) throw new Error("AgentDataProvider missing");
  return data;
}
