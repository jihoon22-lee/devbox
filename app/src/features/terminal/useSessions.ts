import { useQueryClient } from "@tanstack/react-query";
import { useMemo } from "react";
import type { Methods } from "../../rpc/gen/rpc";
import { useRpcMutation, useRpcQuery, useTopicInvalidation } from "../../rpc/query";

export type Session = Methods["terminal.create"]["result"];
export type NewSession = Methods["terminal.create"]["params"];
export function useSessions(projectId: string | null, options: { enabled?: boolean } = {}) {
  const params = { projectId };
  const query = useRpcQuery("terminal.list", params, {
    ...options,
    staleTime: 0,
    refetchInterval: 2000,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: true,
  });
  useTopicInvalidation("terminal.changed", [["terminal.list", params]]);
  const items = useMemo(
    () => query.data?.items.filter((session) => session.projectId === projectId) ?? [],
    [query.data, projectId],
  );
  return { ...query, items };
}
export function useCreateSession() {
  const mutation = useRpcMutation("terminal.create");
  const cache = useQueryClient();
  return async (params: NewSession) => {
    const session = await mutation.mutateAsync(params);
    // A terminal.changed refetch may start before create's reply. Cancel that older
    // snapshot before refreshing the list so pruning cannot erase the new pane.
    await cache.cancelQueries({ queryKey: ["terminal.list"] });
    // Query snapshots carry daemon revisions. A synthetic update at the old
    // revision is intentionally rejected by shareByRevision; wait for the real
    // post-create snapshot before releasing the caller's pruning guard.
    await cache.invalidateQueries({ queryKey: ["terminal.list"], refetchType: "all" });
    return session;
  };
}
