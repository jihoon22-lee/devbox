import { useEffect } from "react";
import type { Methods } from "../../rpc/gen/rpc";
import { useRpcSession } from "../../rpc/RpcProvider";
import { useAgentData } from "../agents/AgentData";
import { STATE_LABEL } from "../agents/model";
import { nativeBadge } from "./native";
import { useNotifications } from "./store";

/** Shell mounts this only for the main host, regardless of route changes. */
export function AttentionBridge() {
  const { client, qc } = useRpcSession();
  const data = useAgentData();
  useEffect(
    () =>
      client.subscribe(
        "agents.attention",
        (_revision, event) => {
          const list = qc.getQueryData<Methods["agents.list"]["result"]>(["agents.list", { includeClosedHours: 24 }]);
          const task = list?.items.find((item) => item.id === event.id);
          useNotifications.getState().push({
            key: `agent:${event.id}`,
            id: event.id,
            kind: "agent",
            title: `${task?.title ?? "에이전트 작업"} · ${STATE_LABEL[event.state]}`,
            body: event.message,
            target: { to: "/agents", search: { id: event.id, view: "list" } },
          });
        },
        () => {},
      ),
    [client, qc],
  );
  useEffect(() => {
    nativeBadge(data.attention, data.tasks.filter((item) => item.state !== "closed").length);
  }, [data.attention, data.tasks]);
  useEffect(() => () => nativeBadge(0, 0), []);
  return null;
}
