import type { WorkspaceIssue } from "../generated/WorkspaceIssue";
import { workspaceMessages, knownIssueMessage } from "./shared";
export const filesMessages: Record<WorkspaceIssue, string> = workspaceMessages;
export const filesIssueMessage = knownIssueMessage;
