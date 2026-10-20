import { focusManager } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RpcProvider } from "../../rpc/RpcProvider";
import { ProjectWire } from "../projects/test-wire";
import { SystemOverview } from "./SystemOverview";

vi.mock("@tanstack/react-router", () => ({ useNavigate: () => vi.fn() }));
afterEach(() => {
  focusManager.setFocused(undefined);
  vi.useRealTimers();
  vi.restoreAllMocks();
});
test("process polling pauses while hidden and resumes every three seconds", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  let visible = false;
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() => (visible ? "visible" : "hidden"));
  focusManager.setFocused(false);
  const wire = new ProjectWire();
  const view = render(
    <RpcProvider client={wire.client}>
      <SystemOverview view="processes" />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  expect(wire.requests("observe.processes")).toHaveLength(0);
  await act(async () => {
    visible = true;
    fireEvent(document, new Event("visibilitychange"));
  });
  await waitFor(() => expect(wire.requests("observe.processes")).toHaveLength(1));
  expect(wire.request("observe.processes").params).toEqual({ projectId: null });
  await act(async () => wire.reply("observe.processes", []));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3100);
  });
  expect(wire.requests("observe.processes")).toHaveLength(2);
  await act(async () => wire.reply("observe.processes", []));
  await act(async () => {
    visible = false;
    fireEvent(document, new Event("visibilitychange"));
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(10000);
  });
  expect(wire.requests("observe.processes")).toHaveLength(2);
  view.unmount();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(10000);
  });
  expect(wire.requests("observe.processes")).toHaveLength(2);
});
test("changing projects discards a late previous-project process response and supports retry", async () => {
  const wire = new ProjectWire();
  const view = render(
    <RpcProvider client={wire.client}>
      <SystemOverview view="processes" projectId="a" />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("observe.processes")).toHaveLength(1));
  const oldRequest = wire.request("observe.processes");
  view.rerender(
    <RpcProvider client={wire.client}>
      <SystemOverview view="processes" projectId="b" />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("observe.processes")).toHaveLength(2));
  expect(wire.request("observe.processes", -1).params).toEqual({ projectId: "b" });
  await act(async () => wire.fail("observe.processes"));
  expect(await screen.findByRole("alert")).toHaveTextContent("프로세스를 불러오지 못했습니다");
  fireEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  await waitFor(() => expect(wire.requests("observe.processes")).toHaveLength(3));
  await act(async () => {
    wire.reply("observe.processes", []);
    wire.deliver({
      type: "response",
      id: oldRequest.id,
      result: [{ pid: 99, name: "stale", startTimeMs: 1, cpuPercent: 0, rssBytes: 0 }],
    });
  });
  expect(await screen.findByText("프로세스가 없습니다.")).toBeVisible();
  expect(screen.queryByText("stale")).not.toBeInTheDocument();
});
