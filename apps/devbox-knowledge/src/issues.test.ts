import { expect, it } from "vitest";
import { issueError } from "./issues";

it("retains machine-readable draft recovery codes independently of wording", () => {
  expect(issueError("draft_stale", "knowledge.notes").name).toBe("draft_stale");
  expect(issueError("draft_busy", "knowledge.notes").name).toBe("draft_busy");
  expect(issueError("raw path /private").name).toBe("Error");
});

it("keeps rename recovery guidance distinct from generic retry errors", () => {
  const issue = issueError("rename_recovery_required", "knowledge.notes");
  expect(issue.name).toBe("rename_recovery_required");
  expect(issue.message).toContain("일부 변경이 남아 있을 수 있습니다");
  expect(issue.message).toContain("외부에서 변경한 내용은 보존");
  expect(issue.message).toContain(".devbox-save-*");
  expect(issue.message).toContain("있는 경우");
  expect(issue.message).not.toContain("잠시 후 다시 시도");
});
