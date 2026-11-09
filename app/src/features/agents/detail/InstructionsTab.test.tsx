import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { METHOD_INFO } from "../../../rpc/gen/rpc";
import { RpcProvider } from "../../../rpc/RpcProvider";
import { TooltipProvider, useToasts } from "../../../ui";
import { useNotifications } from "../../notifications/store";
import { ProjectDataProvider } from "../../projects/ProjectData";
import { ProjectWire } from "../../projects/test-wire";
import { AgentDataProvider } from "../AgentData";
import { useAgentsData } from "../hooks";
import { task } from "../test-fixtures";
import { AgentDetail } from "./AgentDetail";
import { InstructionsTab } from "./InstructionsTab";

vi.mock("../../terminal/TerminalView", () => ({ TerminalView: () => <div /> }));

const toasts = () => useToasts.getState().items.map(({ tone, title }) => ({ tone, title }));
const rows = () => within(screen.getByRole("list", { name: "지시 기록" })).getAllByRole("listitem");
/** Times are shown in the local format; the machine-readable time is what the order is checked against. */
const sentAt = (row: HTMLElement) => row.querySelector("time")?.getAttribute("datetime");
beforeEach(() => {
  useToasts.setState({ items: [] });
  useNotifications.getState().clear();
});
afterEach(cleanup);

test("lists prompt then follow-ups by time and labels only delivered ones 전달됨", async () => {
  const user = userEvent.setup();
  const write = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
  render(
    <InstructionsTab
      task={{
        ...task("a", "idle", 50),
        prompt: "로그인 버그를 고쳐 주세요",
        // Stored out of time order, with two sent in the same millisecond.
        followUps: [
          { atMs: 300, text: "테스트도 추가해 주세요", instructionId: "i3", delivery: "delivered" },
          { atMs: 100, text: "먼저 원인을 찾아 주세요", instructionId: "i1", delivery: "pending" },
          { atMs: 200, text: "로그를 보여 주세요" },
          { atMs: 200, text: "같은 시각에 보낸 지시", instructionId: "i2", delivery: "unknown" },
        ],
      }}
    />,
  );
  const [first, ...followUps] = rows();
  expect(followUps).toHaveLength(4);
  // The first prompt leads, at the task's start, and carries no delivery mark.
  expect(first).toHaveTextContent("처음 지시");
  expect(first).toHaveTextContent("로그인 버그를 고쳐 주세요");
  expect(sentAt(first!)).toBe(new Date(50).toISOString());
  expect(first).not.toHaveTextContent(/전달|보내는 중/);
  // Then by time; equal times keep the order they were stored in.
  expect(followUps.map((row) => row.textContent)).toEqual([
    expect.stringContaining("먼저 원인을 찾아 주세요"),
    expect.stringContaining("로그를 보여 주세요"),
    expect.stringContaining("같은 시각에 보낸 지시"),
    expect.stringContaining("테스트도 추가해 주세요"),
  ]);
  expect(followUps.map(sentAt)).toEqual([100, 200, 200, 300].map((ms) => new Date(ms).toISOString()));
  for (const row of followUps) expect(row).toHaveTextContent("이어서 보낸 지시");
  // Only a delivered instruction says 전달됨; one without a delivery state is not counted as delivered.
  expect(followUps.map((row) => within(row).queryByText("전달됨") !== null)).toEqual([false, false, false, true]);
  expect(screen.getAllByText("전달됨")).toHaveLength(1);
  expect(within(followUps[0]!).getByText("보내는 중")).toBeInTheDocument();
  expect(within(followUps[1]!).getByText("전달 확인 안 됨")).toBeInTheDocument();
  expect(within(followUps[2]!).getByText("전달 확인 안 됨")).toBeInTheDocument();
  // Each entry copies its own text, and says so.
  await user.click(within(followUps[3]!).getByRole("button", { name: "복사" }));
  expect(write).toHaveBeenLastCalledWith("테스트도 추가해 주세요");
  await user.click(within(first!).getByRole("button", { name: "복사" }));
  expect(write).toHaveBeenLastCalledWith("로그인 버그를 고쳐 주세요");
  await waitFor(() =>
    expect(toasts()).toEqual([
      { tone: "ok", title: "지시를 복사했습니다" },
      { tone: "ok", title: "지시를 복사했습니다" },
    ]),
  );
});

test("a copy the clipboard refuses warns without posting an error notification", async () => {
  const user = userEvent.setup();
  vi.spyOn(navigator.clipboard, "writeText").mockRejectedValue(new Error("denied"));
  render(<InstructionsTab task={{ ...task("a", "idle"), prompt: "고쳐 주세요" }} />);
  await user.click(within(rows()[0]!).getByRole("button", { name: "복사" }));
  await waitFor(() => expect(toasts()).toEqual([{ tone: "warn", title: "클립보드에 복사하지 못했습니다" }]));
  expect(useNotifications.getState().items).toEqual([]);
});

test("a task started without a prompt says so and has nothing to copy", () => {
  render(<InstructionsTab task={task("a", "running")} />);
  const [first, ...rest] = rows();
  expect(rest).toHaveLength(0);
  expect(first).toHaveTextContent("처음 지시");
  expect(first).toHaveTextContent("지시문 없이 시작했습니다.");
  expect(within(first!).getByRole("button", { name: "복사" })).toBeDisabled();
  expect(screen.getByText("이어서 보낸 지시가 없습니다.")).toBeInTheDocument();
});

test("a first prompt of only whitespace counts as empty; one with text is copied exactly as stored", async () => {
  const user = userEvent.setup();
  const write = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
  const { rerender } = render(<InstructionsTab task={{ ...task("a", "running"), prompt: " \n\t \n" }} />);
  expect(rows()[0]).toHaveTextContent("지시문 없이 시작했습니다.");
  expect(within(rows()[0]!).getByRole("button", { name: "복사" })).toBeDisabled();
  rerender(<InstructionsTab task={{ ...task("a", "running"), prompt: "  고쳐 주세요\n" }} />);
  expect(rows()[0]).not.toHaveTextContent("지시문 없이 시작했습니다.");
  await user.click(within(rows()[0]!).getByRole("button", { name: "복사" }));
  expect(write).toHaveBeenCalledWith("  고쳐 주세요\n");
});

test("a re-render with the same task leaves the history alone; a changed task updates it", () => {
  const same = {
    ...task("a", "idle", 50),
    prompt: "처음",
    followUps: [{ atMs: 100, text: "다음", delivery: "delivered" as const }],
  };
  const { rerender } = render(<InstructionsTab task={same} />);
  // The detail re-renders every second for its clock. Each entry formats its time when it renders.
  const format = vi.spyOn(Date.prototype, "toLocaleString");
  rerender(<InstructionsTab task={same} />);
  expect(format).not.toHaveBeenCalled();
  rerender(
    <InstructionsTab
      task={{ ...same, followUps: [...same.followUps, { atMs: 200, text: "그다음", delivery: "pending" }] }}
    />,
  );
  expect(rows()).toHaveLength(3);
  expect(rows()[2]).toHaveTextContent("그다음");
});

function Detail() {
  const data = useAgentsData();
  return <AgentDetail id="c" data={data} />;
}

test("instruction_history_shows_closed_task_and_delivery_states_without_sending", async () => {
  const user = userEvent.setup();
  const write = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
  const wire = new ProjectWire({ trustNotRequired: true });
  render(
    <RpcProvider client={wire.client}>
      <TooltipProvider>
        <AgentDataProvider>
          <ProjectDataProvider>
            <Detail />
          </ProjectDataProvider>
        </AgentDataProvider>
      </TooltipProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.get")).toHaveLength(1));
  await act(async () =>
    wire.reply("agents.get", {
      ...task("c", "closed", 10),
      closedHow: "merged",
      prompt: "처음 할 일",
      followUps: [
        { atMs: 20, text: "전달된 지시", instructionId: "d", delivery: "delivered" },
        { atMs: 30, text: "보내던 지시", instructionId: "p", delivery: "pending" },
        { atMs: 40, text: "확인 못 한 지시", instructionId: "u", delivery: "unknown" },
      ],
    }),
  );
  await user.click(await screen.findByRole("tab", { name: "지시" }));
  const panel = screen.getByRole("tabpanel", { name: "지시" });
  const [first, delivered, pending, unknown] = within(panel).getAllByRole("listitem");
  expect(first).toHaveTextContent("처음 할 일");
  expect(within(delivered!).getByText("전달됨")).toBeInTheDocument();
  expect(within(pending!).getByText("보내는 중")).toBeInTheDocument();
  expect(within(unknown!).getByText("전달 확인 안 됨")).toBeInTheDocument();
  for (const row of [first, delivered, pending, unknown])
    await user.click(within(row!).getByRole("button", { name: "복사" }));
  expect(write.mock.calls.map(([text]) => text)).toEqual([
    "처음 할 일",
    "전달된 지시",
    "보내던 지시",
    "확인 못 한 지시",
  ]);
  // Reading and copying the history changes nothing: no send, and no other mutation either.
  expect(wire.requests("agents.send")).toHaveLength(0);
  expect(
    wire.sent.filter(
      (message) =>
        typeof message.method === "string" &&
        METHOD_INFO[message.method as keyof typeof METHOD_INFO]?.kind === "mutation",
    ),
  ).toEqual([]);
  // A closed task has no composer to send from.
  expect(screen.queryByLabelText("에이전트에게 보낼 지시")).toBeNull();
});
