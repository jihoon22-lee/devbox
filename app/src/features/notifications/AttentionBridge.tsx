import { useEffect } from "react";
import type { Methods, NotificationReason } from "../../rpc/gen/rpc";
import { useRpcSession } from "../../rpc/RpcProvider";
import { useNativeRuntime } from "../../shell/nativeRuntime";
import { useActivityHealth } from "../activity/useActivityStatus";
import { useAgentData } from "../agents/AgentData";
import { STATE_LABEL } from "../agents/model";
import { nativeAttention, nativeBadge, type RunNavigation } from "./native";
import { useNotifications } from "./store";

const runReason: Record<NotificationReason, string> = {
  startLimit: "서비스 재시작 한도 초과",
  schedule_failed: "예약 실행 실패",
  schedule_trust_required: "예약 실행 승인 필요",
};
// Keep replay identity for the RPC session even after the user clears visible history.
const seenRunEvents = new WeakMap<object, Set<string>>();
const noLocation = () => ({ pathname: "", search: {} });

/** Shell mounts this only for the main host, regardless of route changes. */
export function AttentionBridge({
  location = noLocation,
}: {
  location?: () => { pathname: string; search: Record<string, unknown> };
}) {
  const { client, qc, state } = useRpcSession();
  const data = useAgentData();
  useActivityHealth();
  useEffect(
    () =>
      client.subscribe(
        "notes.rename_incomplete",
        (_revision, event) => {
          useNotifications.getState().pushOnce({
            key: `notes:rename:${event.opId}`,
            kind: "error",
            title: `링크를 고치지 못한 노트 ${event.count}개`,
            body: "고치지 못한 노트를 확인하고 다시 시도하세요.",
            target: { to: "/notes", search: { rename: event.opId } },
          });
        },
        () => {},
      ),
    [client],
  );
  useEffect(
    () =>
      client.subscribe(
        "agents.attention",
        (_revision, event) => {
          const list = qc.getQueryData<Methods["agents.list"]["result"]>(["agents.list", { includeClosedHours: 24 }]);
          const task = list?.items.find((item) => item.id === event.id);
          const title = `${task?.title ?? "에이전트 작업"} · ${STATE_LABEL[event.state]}`;
          const current = location();
          void nativeAttention({
            id: event.id,
            title,
            body: event.message,
            visible:
              !!task &&
              document.visibilityState === "visible" &&
              current.pathname === "/agents" &&
              current.search.id === event.id &&
              current.search.view === "list",
          });
          useNotifications.getState().push({
            key: `agent:${event.id}`,
            id: event.id,
            kind: "agent",
            title,
            body: event.message,
            target: { to: "/agents", search: { id: event.id, view: "list" } },
          });
        },
        () => {},
      ),
    [client, qc, location],
  );
  useEffect(
    () =>
      client.subscribe(
        "notify.raised",
        (_revision, event) => {
          let seen = seenRunEvents.get(client);
          if (!seen) {
            seen = new Set();
            seenRunEvents.set(client, seen);
          }
          if (seen.has(event.id)) return;
          seen.add(event.id);
          const target: RunNavigation = {
            to: "/runs",
            search: {
              projectId: event.projectId,
              ...(event.runId ? { runId: event.runId } : {}),
              ...(event.task ? { task: event.task } : {}),
            },
          };
          const title = runReason[event.reason];
          const body = event.task ?? event.runId ?? "실행 설정을 확인하세요.";
          // Event identity differs after a manual restart, even when the run ID is reused.
          if (
            !useNotifications
              .getState()
              .pushOnce({ key: `run:${event.id}`, id: event.id, kind: "run", title, body, target })
          )
            return;
          void nativeAttention({ id: event.id, title, body, visible: false, target });
        },
        () => {},
      ),
    [client],
  );
  useEffect(() => {
    nativeBadge(data.attention, data.tasks.filter((item) => item.state !== "closed").length);
    useNativeRuntime.setState({
      snapshotCurrent: state === "ready" && !!data.list.data && !data.list.isFetching && !data.list.isError,
      ...(data.list.data ? { openTasks: data.tasks.filter((item) => item.state !== "closed").length } : {}),
    });
  }, [data.attention, data.tasks, data.list.data, data.list.isFetching, data.list.isError, state]);
  useEffect(
    () => () => {
      nativeBadge(0, 0);
      useNativeRuntime.setState({ snapshotCurrent: false });
    },
    [],
  );
  return null;
}
