import type { WorkspaceIssue } from "../generated/WorkspaceIssue";
import { workspaceMessages, knownIssueMessage } from "./shared";
export const registryMessages: Record<WorkspaceIssue, string> = workspaceMessages;
export const registryIssueMessage = knownIssueMessage;
