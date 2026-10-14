import { type QueryClient, skipToken, useQuery } from "@tanstack/react-query";
import { createContext, type ReactNode, useContext } from "react";
import type { Methods } from "../../rpc/gen/rpc";
import { compareRev, useRpcQuery, useTopicInvalidation } from "../../rpc/query";
export const PROJECT_KEYS = [["projects.list", {}], ["projects.summary", {}], ["projects.config"]] as const;
const REMOVED_KEY = ["projects.removed"];
type RemovedProjects = Record<string, string>;
/** Read immediately before execution; query observers may not have rendered a removal yet. */
export function isRegisteredProject(qc: QueryClient, id: string) {
  const list = qc.getQueryData<Methods["projects.list"]["result"]>(PROJECT_KEYS[0]);
  const removed = qc.getQueryData<RemovedProjects>(REMOVED_KEY)?.[id];
  return !!list?.items.some((project) => project.id === id) && (!removed || compareRev(list.rev, removed) > 0);
}
const Context = createContext<ReturnType<typeof useProjectList> | null>(null);
/** Session cache survives popup unmounts and resets with the RPC daemon identity. */
export function recordProjectRemoval(qc: QueryClient, id: string, rev: string) {
  qc.setQueryData<RemovedProjects>(REMOVED_KEY, (old) => ({ ...old, [id]: rev }));
}
function useProjectList() {
  const list = useRpcQuery("projects.list", {});
  const removed = useQuery<RemovedProjects>({ queryKey: REMOVED_KEY, queryFn: skipToken, initialData: {} });
  const snapshot = list.data;
  const data = snapshot && {
    ...snapshot,
    // Never advance the cached list revision using a mutation result: that
    // would hide other changes between the cached list and this removal.
    items: snapshot.items.filter((project) => {
      const rev = removed.data?.[project.id];
      return !rev || compareRev(snapshot.rev, rev) > 0;
    }),
  };
  return { ...list, data };
}
/** One query/subscription owner for the title and switcher mounted by Shell. */
export function ProjectDataProvider({ children }: { children: ReactNode }) {
  const list = useProjectList();
  useTopicInvalidation("projects.changed", PROJECT_KEYS);
  return <Context.Provider value={list}>{children}</Context.Provider>;
}
export function ProjectDataBoundary({ children }: { children: ReactNode }) {
  const list = useContext(Context);
  return list ? children : <ProjectDataProvider>{children}</ProjectDataProvider>;
}
export function useProjectData() {
  const list = useContext(Context);
  if (!list) throw new Error("ProjectDataProvider missing");
  return list;
}
