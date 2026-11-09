import { create } from "zustand";
/** What the new task form starts from. Missing fields keep the form's usual defaults; nothing starts until [시작]. */
export interface AgentDraft {
  prompt: string;
  truncated?: boolean;
  title?: string;
  /** One no longer registered when the form opens gives way to the usual default project. */
  projectId?: string;
  baseBranch?: string;
  worktree?: boolean;
}
export const useAgentsUi = create<{
  creating: boolean;
  draft?: AgentDraft;
  setCreating(creating: boolean, draft?: AgentDraft): void;
}>((set) => ({
  creating: false,
  setCreating: (creating, draft) => set({ creating, draft: creating ? draft : undefined }),
}));
