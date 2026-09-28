import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { CodePanel } from "./CodePanel";
import { emptyRequest } from "./lib/importers";
const writeText = vi.fn();
const request = { ...emptyRequest(), url: "https://example.test/{{missing}}" };
afterEach(cleanup);
beforeEach(() => {
  writeText.mockReset().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
});
it("shows language tabs, placeholders and copied code with accessible keyboard navigation", async () => {
  const { container } = render(<CodePanel request={request} environment={[]} />);
  expect(screen.getByText("채워 넣을 값: missing")).toBeTruthy();
  fireEvent.click(screen.getByRole("tab", { name: "Python" }));
  expect(screen.getByRole("tabpanel").textContent).toContain("import requests");
  fireEvent.click(screen.getByRole("button", { name: "복사" }));
  await waitFor(() => expect(writeText).toHaveBeenCalledWith(expect.stringContaining("import requests")));
  expect(await screen.findByText("복사했습니다.")).toBeTruthy();
  const python = screen.getByRole("tab", { name: "Python" });
  fireEvent.keyDown(python, { key: "ArrowRight" });
  expect(screen.getByRole("tab", { name: "Go", selected: true })).toBeTruthy();
  expect(document.activeElement).toBe(screen.getByRole("tab", { name: "Go" }));
  await assertNoA11yViolations(container);
});
it("shows a fixed copy error instead of clipboard internals", async () => {
  writeText.mockRejectedValue(new Error("private-path"));
  render(<CodePanel request={request} environment={[]} />);
  fireEvent.click(screen.getByRole("button", { name: "복사" }));
  expect((await screen.findByRole("alert")).textContent).toContain("코드를 복사하지 못했습니다.");
  expect(screen.queryByText(/private-path/)).toBeNull();
});
it("reports invalid GraphQL configuration without crashing the editor", () => {
  render(
    <CodePanel
      request={{
        ...request,
        body_kind: "graphql",
        graphql: { query: "broken {", variables: "private-body", operation_name: "" },
      }}
      environment={[]}
    />,
  );
  expect(screen.getByRole("alert").textContent).toContain("요청 구성을 확인해 주세요.");
  expect(screen.queryByText(/private-body/)).toBeNull();
  expect((screen.getByRole("button", { name: "복사" }) as HTMLButtonElement).disabled).toBe(true);
});
