import type {
  Job,
  ServiceInstance,
  WorkspaceTaskControlPreview,
  WorkspaceTaskControlReceipt,
  WorkspaceTaskOperation,
  WorkspaceTaskOperationRunStatus,
  WorkspaceTaskOperationStatus,
  WorkspaceTaskState,
} from "../types";

export function targetLabel(job: Job): string {
  return job.targetKind === "wsl" ? `WSL · ${job.targetDistro ?? "배포판 없음"}` : "Windows";
}

export function scheduleLabel(job: Job): string {
  return job.cronExpr ?? "일정 없음";
}

export function restartLabel(job: Job): string {
  return job.restartPolicy === "on-failure"
    ? "실패 시 재시작"
    : job.restartPolicy === "always"
      ? "항상 재시작"
      : "재시작 안 함";
}

export function serviceStateLabel(state: ServiceInstance["state"]): string {
  switch (state) {
    case "running":
      return "실행 중";
    case "starting":
      return "시작 중";
    case "stopping":
      return "정지 중";
    case "retry_waiting":
      return "재시작 대기";
    case "stopped":
      return "정지됨";
  }
}

export function shortRevision(revision: string): string {
  return revision ? revision.slice(0, 8) : "--------";
}

export function workspaceSourceLabel(sourceRoot: string): string {
  const normalized = sourceRoot.replace(/[\\/]+$/, "");
  const parts = normalized.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? sourceRoot;
}

export function canUseWorkspaceTask(task: WorkspaceTaskState | undefined, snapshotFresh: boolean): boolean {
  return snapshotFresh && (!task || workspaceTaskGateCode(task) === null);
}

/** Return the stable native gate code for the first failed workspace guard. */
export function workspaceTaskGateCode(task: WorkspaceTaskState | undefined): string | null {
  if (!task) return null;
  if (!task.trusted) return "source-untrusted";
  if (!task.available) return "unavailable";
  if (task.taskKind === "shell" && !task.shellTrusted) return "shell-untrusted";
  return null;
}

export function workspaceTaskGateHint(task: WorkspaceTaskState | undefined): string {
  switch (workspaceTaskGateCode(task)) {
    case "source-untrusted":
      return "workspace task source 승인이 필요합니다.";
    case "unavailable":
      return "workspace task 원본이 변경되어 다시 가져와야 합니다.";
    case "shell-untrusted":
      return "셸 실행 별도 승인이 필요합니다.";
    default:
      return "workspace task 상태를 다시 확인해야 합니다.";
  }
}

export function isWorkspaceOperationTerminal(status: WorkspaceTaskOperationStatus): boolean {
  return status === "succeeded" || status === "failed" || status === "cancelled";
}

export function workspaceOperationStatusLabel(status: WorkspaceTaskOperationStatus): string {
  switch (status) {
    case "queued":
      return "대기 중";
    case "running":
      return "실행 중";
    case "stopping":
      return "중지 중";
    case "succeeded":
      return "완료";
    case "failed":
      return "실패";
    case "cancelled":
      return "취소됨";
  }
}

export function workspaceOperationRunStatusLabel(status: WorkspaceTaskOperationRunStatus): string {
  switch (status) {
    case "pending":
      return "대기 중";
    case "launching":
      return "시작 중";
    case "running":
      return "실행 중";
    case "succeeded":
      return "완료";
    case "failed":
      return "실패";
    case "cancelled":
      return "취소됨";
    case "skipped":
      return "건너뜀";
  }
}

export function workspaceOperationProgressLabel(operation: WorkspaceTaskOperation): string {
  const completed = operation.runs.filter(
    (run) =>
      run.status === "succeeded" || run.status === "failed" || run.status === "cancelled" || run.status === "skipped",
  ).length;
  return `child ${completed}/${operation.runs.length}`;
}

export function isWorkspaceOperationRunTerminal(status: WorkspaceTaskOperationRunStatus): boolean {
  return status === "succeeded" || status === "failed" || status === "cancelled" || status === "skipped";
}

export function taskControlActionLabel(action: WorkspaceTaskControlPreview["action"]): string {
  return action === "start" ? "시작" : "중지";
}

export function taskControlReceiptStatusLabel(status: WorkspaceTaskControlReceipt["status"]): string {
  switch (status) {
    case "accepted":
      return "승인됨";
    case "rejected":
      return "거절됨";
    case "started":
      return "시작됨";
    case "stopped":
      return "중지됨";
    case "failed":
      return "실패";
  }
}
