import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps } from "react";
import { RpcProvider } from "../../../rpc/RpcProvider";
import { ProjectWire } from "../../projects/test-wire";
import { task } from "../test-fixtures";
import { Compose, useDrafts } from "./Compose";

beforeEach(() => useDrafts.setState({ text: {} }));
async function setup(props: Partial<ComponentProps<typeof Compose>> = {}) {
  const wire = new ProjectWire();
  const view = render(
    <RpcProvider client={wire.client}>
      <Compose agentId="a1" {...props} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  return { wire, view, box: screen.getByLabelText("에이전트에게 보낼 지시") };
}
test("IME sends final DOM value once and preserves edits made while sending", async () => {
  const { wire, box } = await setup();
  fireEvent.change(box, { target: { value: "지시 ㄱ" } });
  fireEvent.compositionStart(box);
  fireEvent.keyDown(box, { key: "Enter", ctrlKey: true, isComposing: true });
  fireEvent.keyDown(box, { key: "Enter", ctrlKey: true, repeat: true, isComposing: true });
  expect(wire.requests("agents.send")).toHaveLength(0);
  fireEvent.compositionEnd(box, { target: { value: "지시 가" }, data: "가" });
  fireEvent.submit(box.closest("form")!);
  await waitFor(() => expect(wire.requests("agents.send")).toHaveLength(1));
  expect(wire.request("agents.send").params).toEqual({ id: "a1", text: "지시 가" });
  fireEvent.change(box, { target: { value: "다음 지시" } });
  await act(async () => wire.reply("agents.send", task("a1")));
  await waitFor(() => expect(box).toHaveValue("다음 지시"));
});
test("plain Enter is native newline; changing agent cancels queued IME intent", async () => {
  const { wire, box, view } = await setup();
  fireEvent.change(box, { target: { value: "초안" } });
  expect(fireEvent.keyDown(box, { key: "Enter" })).toBe(true);
  fireEvent.compositionStart(box);
  fireEvent.keyDown(box, { key: "Enter", ctrlKey: true, isComposing: true });
  view.rerender(
    <RpcProvider client={wire.client}>
      <Compose agentId="a2" />
    </RpcProvider>,
  );
  fireEvent.compositionEnd(screen.getByLabelText("에이전트에게 보낼 지시"));
  await act(async () => {});
  expect(wire.requests("agents.send")).toHaveLength(0);
  expect(useDrafts.getState().text.a1).toBe("초안");
});

test("confirmed send clears its original unchanged draft after unmount", async () => {
  const { wire, box, view } = await setup();
  fireEvent.change(box, { target: { value: "완료할 지시" } });
  fireEvent.keyDown(box, { key: "Enter", ctrlKey: true });
  await waitFor(() => expect(wire.requests("agents.send")).toHaveLength(1));
  view.rerender(<RpcProvider client={wire.client}>{null}</RpcProvider>);
  await act(async () => wire.reply("agents.send", task("a1")));
  await waitFor(() => expect(useDrafts.getState().text.a1).toBe(""));
});

test("unknown delivery consumes only the submitted unchanged draft and prevents immediate resend", async () => {
  const { wire, box } = await setup();
  fireEvent.change(box, { target: { value: "확인할 지시" } });
  fireEvent.submit(box.closest("form")!);
  await waitFor(() => expect(wire.requests("agents.send")).toHaveLength(1));
  await act(async () =>
    wire.deliver({ type: "response", id: wire.request("agents.send").id, error: { code: "agents.delivery_unknown" } }),
  );
  await waitFor(() => expect(box).toHaveValue(""));
  expect(screen.getByRole("alert")).toHaveTextContent("확인할 수 없습니다");
  fireEvent.submit(box.closest("form")!);
  expect(wire.requests("agents.send")).toHaveLength(1);
});
test("unknown delivery preserves a newer draft revision including an identical new edit", async () => {
  const { wire, box } = await setup();
  fireEvent.change(box, { target: { value: "보낸 지시" } });
  fireEvent.submit(box.closest("form")!);
  await waitFor(() => expect(wire.requests("agents.send")).toHaveLength(1));
  fireEvent.change(box, { target: { value: "새 지시" } });
  fireEvent.change(box, { target: { value: "보낸 지시" } });
  await act(async () =>
    wire.deliver({ type: "response", id: wire.request("agents.send").id, error: { code: "agents.delivery_unknown" } }),
  );
  expect(box).toHaveValue("보낸 지시");
  expect(useDrafts.getState().text.a1).toBe("보낸 지시");
});
test("proven send failure keeps the original draft for normal retry", async () => {
  const { wire, box } = await setup();
  fireEvent.change(box, { target: { value: "전송 안 된 지시" } });
  fireEvent.submit(box.closest("form")!);
  await waitFor(() => expect(wire.requests("agents.send")).toHaveLength(1));
  await act(async () => wire.fail("agents.send"));
  expect(box).toHaveValue("전송 안 된 지시");
});
const WAITING = "에이전트가 터미널에서 선택을 기다리는 동안에는 보낼 수 없습니다.";
test("waiting agent keeps the draft but sends nothing and points to its terminal", async () => {
  const onShowTerminal = vi.fn();
  const { wire, box } = await setup({ agentState: "waiting", onShowTerminal });
  fireEvent.change(box, { target: { value: "승인 화면에 들어가면 안 되는 지시" } });
  expect(box).toBeEnabled();
  expect(screen.getByRole("button", { name: /^보내기/ })).toBeDisabled();
  fireEvent.keyDown(box, { key: "Enter", ctrlKey: true });
  fireEvent.keyUp(box, { key: "Enter", ctrlKey: true });
  fireEvent.submit(box.closest("form")!);
  fireEvent.compositionStart(box);
  fireEvent.keyDown(box, { key: "Enter", ctrlKey: true, isComposing: true });
  fireEvent.compositionEnd(box, { target: { value: "승인 화면에 들어가면 안 되는 지시" }, data: "시" });
  await act(async () => {});
  expect(wire.requests("agents.send")).toHaveLength(0);
  expect(box).toHaveValue("승인 화면에 들어가면 안 되는 지시");
  expect(screen.getByRole("status")).toHaveTextContent(WAITING);
  fireEvent.click(screen.getByRole("button", { name: "터미널 보기" }));
  expect(onShowTerminal).toHaveBeenCalledTimes(1);
});
test.each(["idle", "running"] as const)(
  "%s agent sends with Ctrl+Enter and shows no waiting notice",
  async (agentState) => {
    const { wire, box } = await setup({ agentState, onShowTerminal: vi.fn() });
    expect(screen.queryByText(WAITING)).toBeNull();
    expect(screen.queryByRole("button", { name: "터미널 보기" })).toBeNull();
    fireEvent.change(box, { target: { value: "다음 지시" } });
    expect(screen.getByRole("button", { name: /^보내기/ })).toBeEnabled();
    fireEvent.keyDown(box, { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(wire.requests("agents.send")).toHaveLength(1));
    expect(wire.request("agents.send").params).toEqual({ id: "a1", text: "다음 지시" });
  },
);
