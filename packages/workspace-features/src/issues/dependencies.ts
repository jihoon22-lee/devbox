import type { WorkspaceIssue } from "../generated/WorkspaceIssue";
import { workspaceMessages, knownIssueMessage } from "./shared";
export const dependenciesMessages: Record<WorkspaceIssue, string> = workspaceMessages;
export const dependenciesIssueMessage = knownIssueMessage;
