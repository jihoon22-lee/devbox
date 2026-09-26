import { afterEach, describe, expect, it } from "vitest";
import { diagnosticText, ProductIssueError, recentIssues, recordIssue, resetIssues } from "./issues";

afterEach(() => resetIssues());

describe("recent issues", () => {
  it("keeps the newest 20 and copies only identifiers", () => {
    for (let index = 0; index < 25; index++) {
      recordIssue(
        new ProductIssueError("저장하지 못했습니다. C:\\Users\\me\\a.md", {
          code: "note_conflict",
          product: "knowledge",
          component: "knowledge.notes",
          method: "write_file",
          requestId: `r-${index}`,
        }),
      );
    }
    expect(recentIssues()).toHaveLength(20);
    expect(recentIssues()[0].requestId).toBe("r-24");
    const text = diagnosticText(recentIssues()[0], "0.9.0");
    expect(JSON.parse(text)).toEqual({
      product: "knowledge",
      version: "0.9.0",
      component: "knowledge.notes",
      method: "write_file",
      code: "note_conflict",
      requestId: "r-24",
      occurredAt: expect.any(String),
    });
    expect(text).not.toContain("Users");
  });
});

it("never copies paths smuggled into identifier fields", () => {
  const error = new ProductIssueError("private message", {
    code: "C:/private/code",
    product: "private/path",
    component: "private/path",
    method: "read /private",
    requestId: "C:\\private",
  });
  const copied = diagnosticText(error, "private/version");
  expect(Object.keys(JSON.parse(copied))).toEqual([
    "product",
    "version",
    "component",
    "method",
    "code",
    "requestId",
    "occurredAt",
  ]);
  expect(copied).not.toContain("private");
});
