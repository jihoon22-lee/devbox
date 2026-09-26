import type { WorkspaceIssue } from "../generated/WorkspaceIssue";
import { workspaceMessages, knownIssueMessage } from "./shared";
export const setupMessages: Record<WorkspaceIssue, string> = workspaceMessages;
export const setupIssueMessage = knownIssueMessage;
