import type { WorkspaceIssue } from "../generated/WorkspaceIssue";
import { workspaceMessages, knownIssueMessage } from "./shared";
export const terminalMessages: Record<WorkspaceIssue, string> = workspaceMessages;
export const wslIssueMessage = knownIssueMessage;
