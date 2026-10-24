import { act, cleanup, render, waitFor } from "@testing-library/react";
import { RpcProvider } from "../../rpc/RpcProvider";
import { AgentDataProvider } from "../agents/AgentData";
import { task } from "../agents/test-fixtures";
import { ProjectWire } from "../projects/test-wire";
import { AttentionBridge } from "./AttentionBridge";
import { nativeBadge } from "./native";
import { useNotifications } from "./store";

afterEach(() => {
  cleanup();
  useNotifications.getState().clear();
  document.title = "devbox";
});
test("attention owns one subscription, reads current cache titles and counts tasks rather than unread", async () => {
  const wire = new ProjectWire();
  const view = render(
    <RpcProvider client={wire.client}>
      <AgentDataProvider>
        <AttentionBridge />
      </AgentDataProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.list")).toHaveLength(1));
  await act(async () => wire.reply("agents.list", { rev: "1", items: [task("a", "waiting"), task("b", "failed")] }));
  await waitFor(() => expect(document.title).toBe("(2) devbox"));
  const sub = wire.sent.find((m) => m.topic === "agents.attention")!;
  expect(wire.sent.filter((m) => m.topic === "agents.attention")).toHaveLength(1);
  await act(async () =>
    wire.deliver({
      type: "event",
      subId: sub.id,
      rev: "2",
      payload: { id: "a", state: "waiting", message: "needs Bash" },
    }),
  );
  expect(useNotifications.getState().items[0]).toMatchObject({
    title: "작업 a · 입력 대기",
    body: "needs Bash",
    target: { to: "/agents", search: { id: "a", view: "list" } },
  });
  act(() => useNotifications.getState().markAllRead());
  expect(document.title).toBe("(2) devbox");
  view.rerender(
    <RpcProvider client={wire.client}>
      <AgentDataProvider>{null}</AgentDataProvider>
    </RpcProvider>,
  );
  expect(wire.sent).toContainEqual({ type: "unsubscribe", subId: sub.id });
  expect(document.title).toBe("devbox");
});
test("badge fallback deduplicates equal values", () => {
  const setter = vi.spyOn(document, "title", "set");
  nativeBadge(5, 10);
  nativeBadge(5, 10);
  nativeBadge(0, 10);
  expect(setter).toHaveBeenCalledTimes(2);
  expect(document.title).toBe("devbox");
});

test("incomplete note rename links its one notice to the rename report", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <AgentDataProvider>
        <AttentionBridge />
      </AgentDataProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.sent.some((m) => m.topic === "notes.rename_incomplete")).toBe(true));
  const sub = wire.sent.find((m) => m.topic === "notes.rename_incomplete")!;
  act(() => {
    for (let i = 0; i < 2; i++)
      wire.deliver({ type: "event", subId: sub.id, rev: "2", payload: { opId: "abc123", count: 2, reason: "busy" } });
  });
  const notices = useNotifications.getState().items.filter((n) => n.key === "notes:rename:abc123");
  expect(notices).toHaveLength(1);
  expect(notices[0]).toMatchObject({
    title: "링크를 고치지 못한 노트 2개",
    target: { to: "/notes", search: { rename: "abc123" } },
  });
});
