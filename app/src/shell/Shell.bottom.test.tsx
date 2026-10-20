import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useCurrentProject } from "../features/projects/store";
import { ProjectWire } from "../features/projects/test-wire";
import { usePinnedLogSources } from "../features/runs/pinnedSources";
import { RpcProvider } from "../rpc/RpcProvider";
import { TooltipProvider } from "../ui";
import { useBottomTabs } from "./BottomPanel";
import { useLayout } from "./layout";
import { Shell } from "./Shell";

vi.mock("../features/terminal/TerminalView", () => ({ TerminalView: () => <div /> }));

test("shell registers terminal logs and problems; changing tabs and folding clean up live logs", async () => {
  window.history.replaceState({}, "", "/notes");
  vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  useCurrentProject.setState({ id: "p" });
  usePinnedLogSources.setState({ byProject: { p: [{ kind: "run", runId: "r" }] } });
  useLayout.setState({ bottomOpen: true });
  const wire = new ProjectWire();
  const view = render(
    <RpcProvider client={wire.client}>
      <TooltipProvider>
        <Shell />
      </TooltipProvider>
    </RpcProvider>,
  );
  await screen.findByRole("tab", { name: "터미널" });
  fireEvent.click(screen.getByRole("tab", { name: "로그" }));
  await waitFor(() => expect(wire.requests("logs.open")).toHaveLength(1));
  await act(async () => wire.reply("logs.open", { streamId: 7 }));
  fireEvent.click(screen.getByRole("tab", { name: "문제" }));
  expect(wire.sent.filter((m) => m.type === "streamClose" && m.stream === 7)).toHaveLength(1);
  fireEvent.click(screen.getByRole("tab", { name: "로그" }));
  await waitFor(() => expect(wire.requests("logs.open")).toHaveLength(2));
  await act(async () => wire.reply("logs.open", { streamId: 8 }));
  fireEvent.click(screen.getByRole("button", { name: "하단 패널 접기" }));
  expect(wire.sent.filter((m) => m.type === "streamClose" && m.stream === 8)).toHaveLength(1);
  view.unmount();
  expect(useBottomTabs.getState().tabs).toHaveLength(0);
  window.history.replaceState({}, "", "/");
});

test("status navigation renders port and system details and returns to runs with transport search intact", async () => {
  window.history.replaceState({}, "", "/notes?gateway=ws%3A%2F%2Ffixture&renderer=dom");
  vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  useCurrentProject.setState({ id: undefined });
  useLayout.setState({ bottomOpen: false });
  const wire = new ProjectWire();
  const view = render(
    <RpcProvider client={wire.client}>
      <TooltipProvider>
        <Shell />
      </TooltipProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("runtime.summary")).toHaveLength(1));
  await act(async () => {
    wire.reply("runtime.summary", { rev: "1", active: 205 });
    wire.reply("observe.ports", [
      {
        port: 8080,
        proto: "tcp",
        addr: "127.0.0.1",
        process: "owned",
        pid: 123,
        runId: null,
        projectId: null,
        containerId: null,
      },
    ]);
    wire.reply("observe.system", {
      memoryTotalBytes: 20 * 1024 ** 3,
      memoryAvailableBytes: 10 * 1024 ** 3,
      swapTotalBytes: 0,
      swapUsedBytes: 0,
      cpuPressure: null,
      memoryPressure: null,
      ioPressure: null,
      homeFilesystem: null,
    });
  });
  fireEvent.click(await screen.findByRole("button", { name: "포트 8080" }));
  expect(await screen.findByRole("heading", { name: "포트 목록" })).toBeInTheDocument();
  expect(screen.getByText("owned")).toBeInTheDocument();
  expect(window.location.search).toContain("view=ports");
  fireEvent.click(screen.getByRole("button", { name: "WSL 10/20GB" }));
  expect(await screen.findByRole("heading", { name: "WSL 리소스" })).toBeInTheDocument();
  expect(screen.getByText("10/20GB")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "실행 205" }));
  await waitFor(() => expect(window.location.search).not.toContain("view="));
  expect(window.location.pathname).toBe("/runs");
  expect(window.location.search).toContain("gateway=");
  expect(window.location.search).toContain("renderer=dom");
  view.unmount();
  window.history.replaceState({}, "", "/");
});
