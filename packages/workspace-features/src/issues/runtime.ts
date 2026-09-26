import type { WorkspaceIssue } from "../generated/WorkspaceIssue";
import { workspaceMessages, knownIssueMessage } from "./shared";
export const runtimeMessages: Record<WorkspaceIssue, string> = workspaceMessages;
export const runtimeIssueMessage = knownIssueMessage;
