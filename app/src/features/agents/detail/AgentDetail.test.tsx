import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../../rpc/RpcProvider";
import { TooltipProvider } from "../../../ui";
import { ProjectDataProvider } from "../../projects/ProjectData";
import { ProjectWire } from "../../projects/test-wire";
import { AgentDataProvider } from "../AgentData";
import { useAgentsData } from "../hooks";
import { task } from "../test-fixtures";
import { AgentDetail } from "./AgentDetail";
import { useDrafts } from "./drafts";

// The terminal itself is xterm; what matters here is whether the detail view asks it for focus.
vi.mock("../../terminal/TerminalView", () => ({
  TerminalView: ({ focusRequest }: { focusRequest?: number }) => (
    <div data-testid="terminal" data-focus-request={focusRequest ?? "none"} />
  ),
}));

const WAITING = "에이전트가 터미널에서 선택을 기다리는 동안에는 보낼 수 없습니다.";
function Detail() {
  const data = useAgentsData();
  return <AgentDetail id="w" data={data} />;
}
async function show(state: ReturnType<typeof task>["state"]) {
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
  await act(async () => wire.reply("agents.get", task("w", state)));
  // The composer takes input only while the task's terminal is alive.
  await waitFor(() => expect(wire.requests("terminal.list").length).toBeGreaterThan(0));
  await act(async () =>
    wire.reply("terminal.list", {
      rev: "1",
      items: [
        {
          id: "t",
          projectId: "a",
          title: "agent",
          cwd: "/fixture",
          kind: "agent",
          alive: true,
          command: "x",
          createdMs: 1,
        },
      ],
    }),
  );
  const box = await screen.findByLabelText("에이전트에게 보낼 지시");
  await waitFor(() => expect(box).toBeEnabled());
  return { wire, box };
}
beforeEach(() => useDrafts.setState({ text: {} }));
afterEach(cleanup);

test("a waiting task keeps the draft, sends nothing and points to its terminal, which the button then focuses", async () => {
  const { wire, box } = await show("waiting");
  fireEvent.change(box, { target: { value: "승인 화면에 들어가면 안 되는 지시" } });
  expect(screen.getByText(WAITING)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /^보내기/ })).toBeDisabled();
  fireEvent.keyDown(box, { key: "Enter", ctrlKey: true });
  fireEvent.submit(box.closest("form")!);
  await act(async () => {});
  expect(wire.requests("agents.send")).toHaveLength(0);
  expect(box).toHaveValue("승인 화면에 들어가면 안 되는 지시");
  // The terminal tab is open, but nothing has asked it for focus yet.
  expect(screen.getByTestId("terminal")).toHaveAttribute("data-focus-request", "none");
  await userEvent.click(screen.getByRole("button", { name: "터미널 보기" }));
  expect(screen.getByTestId("terminal")).toHaveAttribute("data-focus-request", "1");
  await userEvent.click(screen.getByRole("button", { name: "터미널 보기" }));
  expect(screen.getByTestId("terminal")).toHaveAttribute("data-focus-request", "2");
});

test("from another tab the button opens the terminal with a focus request, which a tab picked by hand does not carry", async () => {
  await show("waiting");
  await userEvent.click(screen.getByRole("tab", { name: "변경" }));
  expect(screen.queryByTestId("terminal")).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "터미널 보기" }));
  expect(screen.getByTestId("terminal")).toHaveAttribute("data-focus-request", "1");
  // Leaving and coming back by hand is an ordinary tab change: the terminal does not grab focus.
  await userEvent.click(screen.getByRole("tab", { name: "변경" }));
  await userEvent.click(screen.getByRole("tab", { name: "터미널" }));
  expect(screen.getByTestId("terminal")).toHaveAttribute("data-focus-request", "none");
});
