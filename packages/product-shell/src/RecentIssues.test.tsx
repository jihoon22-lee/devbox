import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { version } from "../../../apps/devbox-workspace/package.json";
import { ProductIssueError, recordIssue, resetIssues } from "./issues";
import { RecentIssues } from "./RecentIssues";
afterEach(() => {
  cleanup();
  resetIssues();
});
it("updates recent issues live and copies only the seven diagnostic fields", async () => {
  const copy = vi.fn(async (_text: string) => {});
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: copy } });
  const { container } = render(<RecentIssues product="workspace" />);
  expect(screen.getByText("최근 오류가 없습니다.")).toBeTruthy();
  act(() =>
    recordIssue(
      new ProductIssueError("C:\\Users\\private\\secret.md", {
        code: "source_cancelled",
        product: "workspace",
        component: "workspace.source",
        method: "repo_stage",
        requestId: "r-1",
      }),
    ),
  );
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "진단 복사" }));
  });
  const copied = JSON.parse(copy.mock.calls[0][0]);
  expect(copied).toEqual({
    product: "workspace",
    version,
    component: "workspace.source",
    method: "repo_stage",
    code: "source_cancelled",
    requestId: "r-1",
    occurredAt: expect.any(String),
  });
  expect(container.textContent).not.toContain("private");
  expect(copy.mock.calls[0][0]).not.toContain("private");
  await assertNoA11yViolations(container);
});
