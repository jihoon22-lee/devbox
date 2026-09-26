import type { WorkspaceIssue } from "../generated/WorkspaceIssue";
import { workspaceMessages, knownIssueMessage } from "./shared";
export const lspMessages: Record<WorkspaceIssue, string> = workspaceMessages;
export const lspIssueMessage = knownIssueMessage;
