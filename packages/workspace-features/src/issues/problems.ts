import type { WorkspaceIssue } from "../generated/WorkspaceIssue";
import { workspaceMessages, knownIssueMessage } from "./shared";
export const problemsMessages: Record<WorkspaceIssue, string> = workspaceMessages;
export const problemsIssueMessage = knownIssueMessage;
