import type { AgentTask } from "./AgentTask";

export type AgentsResults = {
  bind_worktree: AgentTask;
  finish: AgentTask;
  forget: null;
  list: Array<AgentTask>;
  plan: AgentTask;
  record_worktree: AgentTask;
};
