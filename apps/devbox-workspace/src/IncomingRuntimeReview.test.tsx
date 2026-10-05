import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { IncomingReviewContext, type IncomingReview } from "@devbox/product-shell/incoming";
import type { Description } from "@devbox/product-shell/api";
import { componentCall } from "./native";
import IncomingRuntimeReview from "./IncomingRuntimeReview";

vi.mock("./native", () => ({ componentCall: vi.fn() }));
afterEach(cleanup);
beforeEach(() => {
  vi.mocked(componentCall).mockReset();
});
const description = { context: null } as Description;
const first: IncomingReview = {
  operationId: "first-review",
  revision: "a".repeat(64),
  commandRevision: "b".repeat(64),
  label: "First run",
  route: "tasks",
  context: null,
  target: { kind: "entity", entity: "run", id: "run-one" },
};

it.each(["resolve", "reject"] as const)("preserves a newer review when the previous log open %ss", async (outcome) => {
  let resolve!: (value: unknown) => void;
  let reject!: (reason: unknown) => void;
  const pending = new Promise((done, fail) => {
    resolve = done;
    reject = fail;
  });
  vi.mocked(componentCall).mockImplementation(async (_description, _component, method, args) => {
    if (method === "open_run_log_in_log_lens") return pending;
    return { id: args.id, status: "succeeded", logsAvailable: true, startedAt: 1, createdAt: 1 };
  });
  const clear = vi.fn();
  const content = (review: IncomingReview) => (
    <IncomingReviewContext.Provider value={{ review, clear }}>
      <IncomingRuntimeReview description={description} />
    </IncomingReviewContext.Provider>
  );
  const { rerender } = render(content(first));
  fireEvent.click(await screen.findByRole("button", { name: "출력 로그 열기" }));
  // Even two accepted requests for the same run own distinct review slots.
  rerender(content({ ...first, operationId: "second-review", label: "Second review" }));
  await act(async () => {
    if (outcome === "resolve") resolve(null);
    else reject(new Error("old request failed"));
    await pending.catch(() => undefined);
  });
  expect(clear).not.toHaveBeenCalled();
  expect(screen.queryByRole("alert")).toBeNull();
  expect((screen.getByRole("button", { name: "출력 로그 열기" }) as HTMLButtonElement).disabled).toBe(false);
});

it("clears the current review after opening its log", async () => {
  vi.mocked(componentCall).mockImplementation(async (_description, _component, method) =>
    method === "get_run"
      ? { id: "run-one", status: "succeeded", logsAvailable: true, startedAt: 1, createdAt: 1 }
      : null,
  );
  const clear = vi.fn();
  render(
    <IncomingReviewContext.Provider value={{ review: first, clear }}>
      <IncomingRuntimeReview description={description} />
    </IncomingReviewContext.Provider>,
  );
  fireEvent.click(await screen.findByRole("button", { name: "출력 로그 열기" }));
  await act(async () => {});
  expect(clear).toHaveBeenCalledOnce();
});
