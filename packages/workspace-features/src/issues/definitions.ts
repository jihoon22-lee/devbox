import type { WorkspaceIssue } from "../generated/WorkspaceIssue";
import { workspaceMessages, knownIssueMessage } from "./shared";
export const definitionsMessages: Record<WorkspaceIssue, string> = workspaceMessages;
export const definitionsIssueMessage = knownIssueMessage;
