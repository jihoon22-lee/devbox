import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { RunnerPanel } from "./RunnerPanel";
import { SessionVariables } from "./lib/runner";
import { emptyRequest } from "./lib/importers";
const entry = {
  id: "r",
  name: "Health",
  folder: "Ops",
  saved_at: 1,
  requiresSecretReview: false,
  request: { ...emptyRequest(), url: "https://x.test", requiresSecretReview: false },
  assertions: [
    { id: "a", enabled: true, source: "body" as const, target: "", operator: "equals" as const, expected: "expected" },
  ],
};
const response = {
  status: 200,
  status_text: "OK",
  headers: [],
  duration_ms: 5,
  size_bytes: 12,
  body: "private-body",
  is_json: false,
  final_url: "https://x.test",
  redirects: [],
  cookies: [],
  response_id: null,
  raw_headers_available: false,
  headers_truncated: false,
};
afterEach(cleanup);
it("runs a selected folder, shows progress and copies metadata without response bodies", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
  const select = vi.fn();
  const { container } = render(
    <RunnerPanel
      entries={[entry]}
      environment={[]}
      session={new SessionVariables()}
      deps={{ send: vi.fn().mockResolvedValue(response), seal: async () => "sealed", sleep: async () => {} }}
      onSelect={select}
      onClose={() => {}}
      onSessionChange={() => {}}
    />,
  );
  fireEvent.change(screen.getByLabelText("실행 범위"), { target: { value: "Ops" } });
  fireEvent.click(screen.getByRole("button", { name: "실행" }));
  await screen.findByText("통과 0 · 실패 1 · 오류 0 · 건너뜀 0");
  fireEvent.click(screen.getByRole("button", { name: "Health 실패" }));
  expect(select).toHaveBeenCalledWith("r");
  await assertNoA11yViolations(container);
  fireEvent.click(screen.getByRole("button", { name: "결과 복사" }));
  await waitFor(() => expect(writeText).toHaveBeenCalledOnce());
  expect(writeText.mock.calls[0][0]).not.toContain("private-body");
});
it("stops the in-flight request and reports cancellation", async () => {
  let aborted = false;
  render(
    <RunnerPanel
      entries={[entry]}
      environment={[]}
      session={new SessionVariables()}
      deps={{
        send: (_r, _v, signal) =>
          new Promise((_resolve, reject) =>
            signal.addEventListener("abort", () => {
              aborted = true;
              reject(new Error());
            }),
          ),
        seal: async () => "sealed",
        sleep: async () => {},
      }}
      onSelect={() => {}}
      onClose={() => {}}
      onSessionChange={() => {}}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "실행" }));
  fireEvent.click(screen.getByRole("button", { name: "중지" }));
  await screen.findByText("실행이 중지되었습니다.");
  expect(aborted).toBe(true);
});
