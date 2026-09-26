import type { RunView } from "./RunView";
import type { ServiceInstanceView } from "./ServiceInstanceView";
import type { WorkspaceTaskOperationView } from "./WorkspaceTaskOperationView";

export type ControlResults = {
  run_job_now: RunView;
  stop_active_run: RunView | null;
  start_service: ServiceInstanceView;
  stop_service: ServiceInstanceView | null;
  restart_service: ServiceInstanceView;
  run_workspace_task_operation: WorkspaceTaskOperationView;
  stop_workspace_task_operation: WorkspaceTaskOperationView;
};
