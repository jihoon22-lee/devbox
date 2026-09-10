import { describe, expect, it } from "vitest";
import { friendlyErrorMessage } from "./api";

describe("Run Manager display error boundary", () => {
  it("maps known codes and suppresses unknown native details", () => {
    expect(friendlyErrorMessage("workspace-task-source-changed")).toBe(
      "원본 tasks.json이 변경되어 승인이 무효화되었습니다. 다시 미리보고 승인하세요.",
    );
    expect(friendlyErrorMessage(new Error("native path /private/run.log"))).toBe(
      "요청을 완료하지 못했습니다.",
    );
  });
});
