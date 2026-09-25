import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { IncomingReviewContext, type IncomingReview } from "@devbox/product-shell/incoming";
import type { Description } from "@devbox/product-shell/api";
import { componentCall } from "./native";
import IncomingWebhookLog from "./IncomingWebhookLog";
vi.mock("./native", () => ({ componentCall: vi.fn() }));
afterEach(cleanup);
beforeEach(() => vi.mocked(componentCall).mockReset());
const review: IncomingReview = {
  operationId: "review-one",
  revision: "a".repeat(64),
  commandRevision: "b".repeat(64),
  label: "Webhook",
  route: "logs",
  context: null,
  target: { kind: "entity", entity: "artifact", id: "artifact-one" },
};
const source = {
  kind: "webhookCapture",
  capture: {
    schemaVersion: 1,
    method: "POST",
    target: "/events",
    receivedAtMs: 1788000000000,
    headerNames: [],
    bodyPreview: "ordinary",
    redacted: true,
    truncated: false,
  },
};
const description = { context: null } as Description;
function mount(incoming: IncomingReview | null = review, onOpen = vi.fn()) {
  const clear = vi.fn();
  render(
    <IncomingReviewContext.Provider value={{ review: incoming, clear }}>
      <IncomingWebhookLog description={description} onOpen={onOpen} />
    </IncomingReviewContext.Provider>,
  );
  return { clear, onOpen };
}
it("uses only the accepted native operation and source revision", async () => {
  vi.mocked(componentCall).mockResolvedValue(source);
  const { onOpen, clear } = mount();
  await waitFor(() => expect(onOpen).toHaveBeenCalledWith({ id: review.operationId, source }));
  expect(componentCall).toHaveBeenCalledWith(
    description,
    "workspace.logs",
    "open_webhook_log",
    { id: "artifact-one", revision: review.commandRevision, operationId: review.operationId },
    "logs",
  );
  expect(clear).toHaveBeenCalledOnce();
});
it("does not claim without explicit incoming review", () => {
  mount(null);
  expect(componentCall).not.toHaveBeenCalled();
});
it.each(["offline", "webhook_log_stale", "webhook_log_claimed"])(
  "preserves existing Logs on %s and retries the same operation",
  async (problem) => {
    vi.mocked(componentCall).mockRejectedValueOnce(new Error(problem)).mockResolvedValueOnce(source);
    const { onOpen, clear } = mount();
    await screen.findByRole("alert");
    expect(onOpen).not.toHaveBeenCalled();
    expect(clear).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "다시 확인" }));
    await waitFor(() => expect(clear).toHaveBeenCalledOnce());
    expect(onOpen).toHaveBeenCalledOnce();
    expect(vi.mocked(componentCall).mock.calls[0]).toEqual(vi.mocked(componentCall).mock.calls[1]);
  },
);
it("retains the reviewed request if the Logs queue cannot accept it", async () => {
  vi.mocked(componentCall).mockResolvedValue(source);
  const onOpen = vi.fn().mockImplementationOnce(() => {
    throw new Error("full");
  });
  const { clear } = mount(review, onOpen);
  await screen.findByRole("alert");
  expect(clear).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "다시 확인" }));
  await waitFor(() => expect(clear).toHaveBeenCalledOnce());
});
