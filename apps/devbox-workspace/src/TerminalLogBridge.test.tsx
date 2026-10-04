import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import type { Description } from "@devbox/product-shell/api";
import TerminalLogBridge from "./TerminalLogBridge";
const { call } = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("./native", () => ({ typedComponentCall: () => call }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
afterEach(cleanup);
beforeEach(() => call.mockReset());
const first = { context: null } as Description;
const second = { context: { projectId: "project", worktreeId: "tree" } } as Description;
const issue = "터미널의 로그 요청을 확인하지 못했습니다.";
function bridge(description = first, onOpen = vi.fn()) {
  return <TerminalLogBridge description={description} consumedId={null} onOpen={onOpen} />;
}
it("clears a previous read failure after a successful empty queue read", async () => {
  call.mockRejectedValueOnce(new Error("transient")).mockResolvedValue(null);
  const view = render(bridge());
  await screen.findByText(issue);
  view.rerender(bridge(second));
  await waitFor(() => expect(screen.queryByText(issue)).toBeNull());
});
it("keeps an error when handling a newly read valid request fails", async () => {
  call.mockResolvedValue({
    id: "a".repeat(32),
    context: null,
    source: { kind: "wslFile", distro: "Ubuntu", path: "/owned/log" },
  });
  const onOpen = vi.fn(() => {
    throw new Error("queue full");
  });
  render(bridge(first, onOpen));
  await screen.findByText(issue);
  expect(onOpen).toHaveBeenCalledTimes(1);
});
it("ignores an old effect's late success instead of clearing the current read error", async () => {
  let finish!: (value: null) => void;
  call
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    )
    .mockRejectedValueOnce(new Error("current read failed"));
  const view = render(bridge());
  await waitFor(() => expect(call).toHaveBeenCalledTimes(1));
  view.rerender(bridge(second));
  await screen.findByText(issue);
  await act(async () => finish(null));
  expect(screen.getByText(issue)).toBeTruthy();
});
