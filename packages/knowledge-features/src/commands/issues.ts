import type { QuitIssue } from "../generated/QuitIssue";
export const commandsMessages: Record<QuitIssue, string> = {
  unavailable: "종료 작업을 완료하지 못했습니다. 잠시 후 다시 시도해 주세요.",
  quit_unavailable: "종료 상태를 확인하지 못했습니다. 다시 시도해 주세요.",
  quit_review_stale: "종료 요청이 바뀌었습니다. 다시 종료를 선택해 주세요.",
};
