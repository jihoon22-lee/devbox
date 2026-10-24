import { create } from "zustand";
export interface AgentDraft {
  prompt: string;
  truncated?: boolean;
}
export const useAgentsUi = create<{
  creating: boolean;
  draft?: AgentDraft;
  setCreating(creating: boolean, draft?: AgentDraft): void;
}>((set) => ({
  creating: false,
  setCreating: (creating, draft) => set({ creating, draft: creating ? draft : undefined }),
}));
