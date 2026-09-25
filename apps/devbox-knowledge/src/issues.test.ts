import { expect, it } from "vitest";
import { issueError } from "./issues";

it("retains machine-readable draft recovery codes independently of wording", () => {
  expect(issueError("draft_stale", "knowledge.notes").name).toBe("draft_stale");
  expect(issueError("draft_busy", "knowledge.notes").name).toBe("draft_busy");
  expect(issueError("raw path /private").name).toBe("Error");
});
