declare module "@tanstack/react-router" {
  interface HistoryState {
    runActivation?: string;
  }
}
/** A repeated click on the same notification is still a new selection request. */
export function runNavigationState() {
  return { runActivation: crypto.randomUUID() };
}
export type RunsSearch = {
  view?: "ports" | "system" | "processes";
  projectId?: string;
  runId?: string;
  task?: string;
};
export function runsSearch(search: Record<string, unknown>): RunsSearch {
  const result: RunsSearch = {};
  for (const key of ["projectId", "runId", "task"] as const) {
    if (typeof search[key] === "string" && search[key].trim()) result[key] = search[key];
  }
  if (search.view === "ports" || search.view === "system" || search.view === "processes") result.view = search.view;
  return result;
}
