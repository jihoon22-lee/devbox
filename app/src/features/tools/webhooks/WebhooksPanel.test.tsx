import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { useApiTabs } from "../api/store";

const state = vi.hoisted(() => ({ calls: vi.fn(), selected: false }));
vi.mock("../../../rpc/query", () => ({
  useTopicInvalidation: vi.fn(),
  useRpcQuery: (method: string, params: { id?: string }) => ({
    refetch: vi.fn(),
    data:
      method === "webhooks.listeners"
        ? { items: [{ id: "l", port: 1234, lan: false, addresses: ["127.0.0.1:1234"], running: false }] }
        : method === "webhooks.rules"
          ? { items: [] }
          : method === "webhooks.history"
            ? {
                items: [
                  { id: "r", listenerId: "l", method: "POST", path: "/hook", sizeBytes: 2, atMs: 1, truncated: false },
                ],
              }
            : method === "webhooks.history_get" && params.id
              ? {
                  received: {
                    id: "r",
                    listenerId: "l",
                    method: "POST",
                    path: "/hook",
                    sizeBytes: 2,
                    atMs: 1,
                    truncated: false,
                  },
                  headers: [["Authorization", "Bearer fixture-token"]],
                  query: "",
                  bodyBase64: "e30=",
                  previewTruncated: false,
                }
              : undefined,
  }),
  useRpcMutation: (method: string) => ({
    mutate: (params: unknown) => state.calls(method, params),
    mutateAsync: async (params: { rule?: { id?: string } }) => {
      state.calls(method, params);
      return { ...params.rule, id: params.rule?.id || "assigned-id" };
    },
    isPending: false,
  }),
}));
vi.mock("../../../ui/code/CodeView", () => ({ CodeView: ({ value }: { value: string }) => <pre>{value}</pre> }));

import { WebhooksPanel } from "./WebhooksPanel";

beforeEach(() => {
  state.calls.mockReset();
  useApiTabs.getState().reset();
});
it("does not automatically start stored listeners and defaults to loopback", () => {
  render(<WebhooksPanel openApi={() => {}} />);
  expect(state.calls).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("수신 시작"));
  expect(state.calls).toHaveBeenCalledWith("webhooks.start", { port: 8081, lan: false });
});
it("shows LAN forwarding instructions without executing commands", () => {
  render(<WebhooksPanel openApi={() => {}} />);
  fireEvent.click(screen.getByLabelText("LAN에서 받기"));
  expect(screen.getByText(/netsh interface portproxy/)).toBeInTheDocument();
  expect(state.calls).not.toHaveBeenCalled();
});
it("opens a draft with a storage sanitization notice and never sends automatically", async () => {
  const open = vi.fn();
  render(<WebhooksPanel openApi={open} />);
  fireEvent.click(screen.getByText(/POST \/hook/));
  await waitFor(() => expect(screen.getByText("API 요청으로 열기")).toBeInTheDocument());
  expect(screen.getByText(/프로젝트에 저장할 때 인증 정보/)).toBeInTheDocument();
  fireEvent.click(screen.getByText("API 요청으로 열기"));
  expect(open).toHaveBeenCalledOnce();
  expect(state.calls).not.toHaveBeenCalled();
  expect(useApiTabs.getState().tabs[0]?.draft.headers[0]?.value).toBe("Bearer fixture-token");
});

it("adopts the rule identity returned by the mutation for later saves", async () => {
  render(<WebhooksPanel openApi={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: "규칙 저장" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "규칙 저장" })).not.toBeDisabled());
  fireEvent.change(screen.getByLabelText("응답 본문"), { target: { value: "edited after save" } });
  fireEvent.click(screen.getByRole("button", { name: "규칙 저장" }));
  expect(state.calls).toHaveBeenLastCalledWith(
    "webhooks.rule_upsert",
    expect.objectContaining({
      listenerId: "l",
      rule: expect.objectContaining({ id: "assigned-id", body: "edited after save" }),
    }),
  );
  await waitFor(() => expect(screen.getByRole("button", { name: "규칙 저장" })).not.toBeDisabled());
});
