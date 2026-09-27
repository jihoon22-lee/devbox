import type { AgentIssue } from "@devbox/workspace-features/generated/AgentIssue";
import { workspaceIssueMessage } from "@devbox/workspace-features/issues/shared";
export const messages: Record<AgentIssue | "agent_worktree_unexpected" | "agent_context_changed", string> = {
  agent_task_missing: "작업을 찾을 수 없습니다. 목록을 다시 읽어 주세요.",
  agent_task_changed: "작업 상태가 바뀌었습니다. 목록을 다시 읽고 시도해 주세요.",
  agent_task_state_invalid: "현재 작업 상태에서는 실행할 수 없습니다.",
  agent_task_limit: "작업 목록이 가득 찼습니다. 끝난 작업을 목록에서 지워 주세요.",
  agent_title_invalid: "제목은 제어 문자 없이 120자 이내로 입력해 주세요.",
  agent_command_invalid: "명령을 확인해 주세요. 여러 줄 명령이나 평문 비밀 값은 사용할 수 없습니다.",
  agent_target_invalid: "기본 작업 폴더 밖의 WSL 경로를 선택해 주세요.",
  agent_slug_exhausted: "같은 이름의 작업이 너무 많습니다. 다른 제목을 입력해 주세요.",
  agent_wsl_required: "에이전트 작업은 WSL 프로젝트에서 사용할 수 있습니다.",
  agent_task_context_mismatch: "작업에 연결된 프로젝트와 폴더를 확인해 주세요.",
  agent_resources_unavailable: "작업 리소스를 읽지 못했습니다. 잠시 뒤 다시 확인해 주세요.",
  agent_usage_unavailable: "토큰 사용량 기록을 읽지 못했습니다. WSL과 도구 기록을 확인해 주세요.",
  agent_store_unavailable: "에이전트 작업 기록을 읽거나 저장하지 못했습니다.",
  agent_worktree_unexpected: "작업 폴더를 자동으로 연결할 수 없습니다. 소스 화면에서 경로와 branch를 확인해 주세요.",
  agent_context_changed: "선택한 프로젝트가 바뀌었습니다. 작업 폴더를 다시 선택해 주세요.",
};
export function agentMessage(code: string): string | undefined {
  return Object.prototype.hasOwnProperty.call(messages, code) ? messages[code as keyof typeof messages] : undefined;
}
export function errorCode(error: unknown): string {
  return typeof error === "object" && error !== null && "code" in error && typeof error.code === "string"
    ? error.code
    : "operation_failed";
}
export function errorMessage(error: unknown): string {
  const code = errorCode(error);
  if (code === "source_review_required") return "기본 작업 폴더의 Git 설정을 먼저 확인해 주세요.";
  return agentMessage(code) ?? workspaceIssueMessage(code);
}
