import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { LspStatus } from "./LspStatus";

const rpc = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("../../../rpc/RpcProvider", () => ({ useRpcSession: () => ({ client: rpc, state: "ready" }) }));
vi.mock("../../terminal/TerminalView", () => ({
  TerminalView: ({ id }: { id: string }) => <div data-testid="install-terminal">{id}</div>,
}));
afterEach(() => {
  cleanup();
  rpc.call.mockReset();
});
const result = (state = "missing") => ({
  items: [
    {
      language: "typescript",
      command: "typescript-language-server",
      args: ["--stdio"],
      install: "npm install -g typescript typescript-language-server",
      state,
      generation: 1,
      pid: state === "ready" ? 42 : null,
      memoryBytes: state === "ready" ? 1048576 : null,
      clients: 1,
    },
  ],
  inotify: { maxUserInstances: 128, maxUserWatches: 10000, ownedInstances: 1, ownedWatches: 12, usageComplete: false },
});
test("installation binds the displayed fixed command and opens its returned terminal", async () => {
  rpc.call.mockImplementation(async (method) => (method === "lsp.servers" ? result() : { terminalId: "install-42" }));
  render(<LspStatus projectId="p" language="typescript" />);
  await screen.findByText("설치되지 않음");
  fireEvent.click(screen.getByRole("button", { name: "설치" }));
  await waitFor(() =>
    expect(rpc.call).toHaveBeenCalledWith("lsp.install", {
      projectId: "p",
      language: "typescript",
      expectedCommand: result().items[0]!.install,
    }),
  );
  expect(await screen.findByTestId("install-terminal")).toHaveTextContent("install-42");
});
test.each(["starting", "ready", "failed", "idle_stopped"])(
  "shows %s status, owned resource scope and restart",
  async (state) => {
    rpc.call.mockResolvedValue(result(state));
    render(<LspStatus projectId="p" language="typescript" />);
    await screen.findByText(/소유한 언어 서버/);
    if (state === "ready") {
      expect(screen.getByText(/PID 42/)).toBeInTheDocument();
      expect(screen.getByText(/1.0 MiB/)).toBeInTheDocument();
    }
    fireEvent.click(screen.getByRole("button", { name: "다시 시작" }));
    await waitFor(() =>
      expect(rpc.call).toHaveBeenCalledWith("lsp.restart", { projectId: "p", language: "typescript" }),
    );
  },
);
test("the status poll stops while the window is hidden, keeps its text and asks at once when shown", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  let state: DocumentVisibilityState = "visible";
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() => state);
  rpc.call.mockResolvedValue(result("ready"));
  render(<LspStatus projectId="p" language="typescript" />);
  await screen.findByText(/PID 42/);
  expect(rpc.call).toHaveBeenCalledTimes(1);
  await act(async () => {
    state = "hidden";
    fireEvent(document, new Event("visibilitychange"));
    await vi.advanceTimersByTimeAsync(30_000);
  });
  expect(rpc.call).toHaveBeenCalledTimes(1);
  expect(screen.getByText(/PID 42/)).toBeInTheDocument();
  await act(async () => {
    state = "visible";
    fireEvent(document, new Event("visibilitychange"));
  });
  expect(rpc.call).toHaveBeenCalledTimes(2);
  expect(screen.getByText(/PID 42/)).toBeInTheDocument();
  vi.useRealTimers();
});
