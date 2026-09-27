import type { AgentResources } from "./AgentResources";
import type { AgentTask } from "./AgentTask";
import type { UsageReport } from "./UsageReport";

export type AgentsResults = {
  bind_worktree: AgentTask;
  finish: AgentTask;
  forget: null;
  list: Array<AgentTask>;
  plan: AgentTask;
  record_worktree: AgentTask;
  resources: Array<AgentResources>;
  usage: UsageReport;
};
