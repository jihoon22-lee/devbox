import { expect, it } from "vitest";
import { workspaceMessages, workspaceIssueMessage } from "./shared";
import { runtimeIssueMessage } from "./runtime";
import { wslIssueMessage } from "./terminal";
import { problemsIssueMessage } from "./problems";
it("retains recovery guidance and never renders unknown native text", () => {
  expect(runtimeIssueMessage("runtime_control_recovery_required")).toContain("자동으로 다시 실행하지 않았습니다");
  expect(wslIssueMessage("wsl_distro_stopped")).toContain("중지되어 있습니다");
  const remote = "synthetic_token /private/path response";
  expect(workspaceIssueMessage(remote)).not.toContain(remote);
  for (const lookup of [runtimeIssueMessage, wslIssueMessage, problemsIssueMessage])
    expect(lookup(remote)).toBeUndefined();
});
it("provides localized text for every declared native code", () => {
  for (const [code, message] of Object.entries(workspaceMessages)) {
    expect(message, code).toMatch(/[가-힣]/);
    expect(message, code).not.toBe(code);
  }
});
