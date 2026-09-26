import type { WorkspaceIssue } from "../generated/WorkspaceIssue";
import { workspaceMessages, knownIssueMessage } from "./shared";
export const sourceMessages: Record<WorkspaceIssue, string> = workspaceMessages;
export const sourceIssueMessage = knownIssueMessage;
