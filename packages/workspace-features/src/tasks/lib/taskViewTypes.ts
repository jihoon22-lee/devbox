import type { WorkspaceTaskDiagnostics } from "../types";

export type Screen = "jobs" | "editor" | "services" | "service-editor" | "history";

export interface WorkspaceDiagnosticState {
  status: "loading" | "ready" | "error";
  diagnostics?: WorkspaceTaskDiagnostics;
  error?: string;
}
