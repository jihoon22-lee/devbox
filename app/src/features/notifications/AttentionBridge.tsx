import { useEffect } from "react";
import type { AgentTask, Methods, NotificationReason, ToolProfile } from "../../rpc/gen/rpc";
import { useRpcSession } from "../../rpc/RpcProvider";
import { useNativeRuntime } from "../../shell/nativeRuntime";
import { useActivityHealth } from "../activity/useActivityStatus";
import { useAgentData } from "../agents/AgentData";
import { STATE_LABEL, toolLabel } from "../agents/model";
import { nativeAttention, nativeBadge, type RunNavigation } from "./native";
import { useNotifications } from "./store";

const runReason: Record<NotificationReason, string> = {
  startLimit: "서비스 재시작 한도 초과",
  schedule_failed: "예약 실행 실패",
  schedule_trust_required: "예약 실행 승인 필요",
};
// Keep replay identity for the RPC session even after the user clears visible history.
const seenRunEvents = new WeakMap<object, Set<string>>();
/**
 * True when the daemon wrote the message with the tool's own name in front ("opencode 승인 요청",
 * "pi가 …"). The family comes from the profile's hooks; a missing profile entry falls back to the id.
 */
function startsWithPluginFamily(message: string, family: string | undefined) {
  if (family !== "opencode" && family !== "pi") return false;
  return message.startsWith(family) && !/[A-Za-z0-9-]/.test(message.charAt(family.length));
}
/** "프로젝트 · 도구 — " from the caches. opencode and pi messages already start with their tool name. */
function bodyPrefix(
  task: AgentTask | undefined,
  message: string,
  project: string | undefined,
  profiles: ToolProfile[] | undefined,
) {
  if (!task) return "";
  const family = profiles?.find((p) => p.id === task.profile)?.hooks ?? task.profile;
  const tool = startsWithPluginFamily(message, family) ? undefined : toolLabel(profiles, task.profile);
  const parts = [project, tool].filter((part): part is string => !!part);
  return parts.length ? `${parts.join(" · ")} — ` : "";
}
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
          const project = qc
            .getQueryData<Methods["projects.list"]["result"]>(["projects.list", {}])
            ?.items.find((item) => item.id === task?.projectId)?.name;
          const profiles = qc.getQueryData<Methods["agents.profiles"]["result"]>(["agents.profiles", {}]);
          const body = `${bodyPrefix(task, event.message, project, profiles?.items)}${event.message}`;
          const title = `${task?.title ?? "에이전트 작업"} · ${STATE_LABEL[event.state]}`;
          const current = location();
          void nativeAttention({
            id: event.id,
            title,
            body,
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
            body,
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
