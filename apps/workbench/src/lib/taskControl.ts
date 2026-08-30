import type {
  WorkspaceTaskControl,
  WorkspaceTaskControlReceipt,
  WorkspaceTaskControlReceiptStatus,
} from "../api";

/**
 * Task-control errors are intentionally translated from a closed set. Native
 * error text can contain paths or other local details and must never become UI
 * output in Workbench.
 */
const FIXED_TASK_CONTROL_ERRORS: Record<string, string> = {
  "task-control-dispatch-invalid": "Run Manager task 요청 형식이 올바르지 않습니다.",
  "task-control-snapshot-unavailable": "Run Manager task 상태를 읽을 수 없습니다. 잠시 후 다시 시도하세요.",
  "task-control-snapshot-missing": "Run Manager task snapshot이 없습니다. Run Manager를 열고 task를 새로고침하세요.",
  "task-control-snapshot-invalid": "Run Manager task snapshot을 안전하게 읽을 수 없습니다.",
  "task-control-receipt-unavailable": "Run Manager task 확인 결과를 읽을 수 없습니다.",
  "task-control-receipt-invalid": "Run Manager task 확인 결과가 올바르지 않습니다.",
  "task-control-request-invalid": "Run Manager task 요청 값이 올바르지 않습니다.",
  "task-control-run-manager-unavailable": "Run Manager를 열 수 없어 task 요청을 전달하지 못했습니다.",
  "task-control-cleanup-failed": "전달하지 못한 Run Manager task 요청을 정리하지 못했습니다.",
  "task-control-task-not-found": "Run Manager에서 이 workspace task를 찾지 못했습니다. 목록을 새로고침하세요.",
  "task-control-source-changed": "task snapshot 이후 원본 revision이 변경되었습니다. 목록을 새로고침하고 다시 확인하세요.",
  "task-control-operation-not-active": "현재 실행 중인 workspace task operation이 없습니다.",
  "task-control-request-replayed": "이미 처리된 task 요청입니다. 최신 상태를 새로고침하세요.",
  "task-control-user-rejected": "Run Manager에서 task 요청을 거절했습니다.",
  "task-control-interrupted": "Run Manager가 종료되어 처리 중이던 task 요청이 중단되었습니다.",
  "task-control-claim-failed": "Run Manager가 task 요청의 소유권을 확정하지 못했습니다.",
  "task-control-receipt-storage": "Run Manager가 task 확인 결과를 저장하지 못했습니다.",
  "workspace-task-source-untrusted": "이 workspace task의 소스가 아직 승인되지 않았습니다. Run Manager에서 현재 revision을 승인하세요.",
  "workspace-task-shell-untrusted": "이 셸 task의 실행이 아직 승인되지 않았습니다. Run Manager에서 셸 실행을 별도로 승인하세요.",
  "workspace-task-unavailable": "이 workspace task는 현재 사용할 수 없습니다. Run Manager에서 원본을 다시 확인하세요.",
  "workspace-task-source-changed": "원본 tasks.json이 변경되어 승인이 무효화되었습니다. Run Manager에서 다시 확인하세요.",
  "workspace-task-dependency-selection-incomplete": "선택한 task의 선행 dependency가 빠졌습니다. Run Manager에서 dependency를 함께 선택하세요.",
  "workspace-task-dependency-unavailable": "선행 dependency를 사용할 수 없어 task를 실행할 수 없습니다.",
  "workspace-task-dependency-cycle": "task dependency에 순환 참조가 있어 실행할 수 없습니다.",
  "workspace-task-orchestration-required": "이 workspace task는 dependency가 있어 Run Manager orchestration이 필요합니다.",
  "workspace-task-operation-active": "이 workspace task에는 이미 실행 중인 Run Manager operation이 있습니다.",
  "workspace-task-operation-not-found": "workspace task operation을 찾지 못했습니다. task 목록을 새로고침하세요.",
  "workspace-task-operation-state-changed": "workspace task operation 상태가 바뀌어 요청을 완료하지 못했습니다.",
  "workspace-task-operation-cancelled": "workspace task operation이 취소되었습니다.",
  "workspace-task-operation-stop-failed": "workspace task operation을 안전하게 중지하지 못했습니다.",
  "workspace-task-operation-stop-timeout": "workspace task operation 중지가 제한 시간 안에 끝나지 않았습니다.",
  "workspace-task-operation-ownership-changed": "workspace task operation의 실행 소유권이 바뀌어 중지할 수 없습니다.",
  "workspace-task-start-failed": "workspace task를 시작하지 못했습니다.",
  "workspace-task-run-failed": "workspace task 실행이 실패했습니다.",
  "workspace-task-run-cancelled": "workspace task 실행이 취소되었습니다.",
  "workspace-task-dependency-failed": "선행 dependency가 실패해 operation을 진행하지 못했습니다.",
};

export function taskControlErrorMessage(cause: unknown): string {
  const value = cause instanceof Error ? cause.message : String(cause);
  const code = value.trim();
  return FIXED_TASK_CONTROL_ERRORS[code]
    ?? "Run Manager task 요청을 완료하지 못했습니다.";
}

export function canStartWorkspaceTask(task: WorkspaceTaskControl): boolean {
  return !task.operationActive
    && task.trusted
    && task.available
    && (task.taskKind === "process" || task.shellTrusted);
}

export function canStopWorkspaceTask(task: WorkspaceTaskControl): boolean {
  return task.operationActive;
}

export function isTerminalTaskControlReceipt(
  status: WorkspaceTaskControlReceiptStatus,
): boolean {
  return status === "rejected"
    || status === "started"
    || status === "stopped"
    || status === "failed";
}

export function taskControlReceiptMessage(receipt: WorkspaceTaskControlReceipt): string {
  if (receipt.status === "started") return "Run Manager가 시작했습니다.";
  if (receipt.status === "stopped") return "Run Manager가 중지했습니다.";
  if (receipt.status === "rejected" || receipt.status === "failed") {
    return taskControlErrorMessage(receipt.failureCode ?? receipt.status);
  }
  return "Run Manager 창의 확인을 기다리는 중…";
}
