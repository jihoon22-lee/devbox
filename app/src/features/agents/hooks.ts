import { useRpcQuery } from "../../rpc/query";
import { usePageVisible } from "../../shell/visibility";
import { useAgentData } from "./AgentData";
export function useAgentsData() {
  const data = useAgentData();
  const visible = usePageVisible();
  const stats = useRpcQuery(
    "agents.git_stats",
    {},
    { enabled: visible, refetchInterval: visible ? 10_000 : false, refetchIntervalInBackground: true },
  );
  const resources = useRpcQuery(
    "agents.resources",
    {},
    { enabled: visible, refetchInterval: visible ? 3_000 : false, refetchIntervalInBackground: true },
  );
  return {
    ...data,
    stats: new Map((stats.data?.items ?? []).map((item) => [item.id, item])),
    resources: new Map((resources.data?.items ?? []).map((item) => [item.id, item])),
    statsError: stats.error,
    resourcesError: resources.error,
  };
}
