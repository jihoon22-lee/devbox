import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RpcProvider } from "../../rpc/RpcProvider";
import { ProjectWire } from "../projects/test-wire";
import { SystemOverview } from "./SystemOverview";

const navigate = vi.hoisted(() => vi.fn());
vi.mock("@tanstack/react-router", () => ({ useNavigate: () => navigate }));
const gib = 1024 ** 3;

test("system overview shows actual resource values, no ports request, and return navigation", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <SystemOverview view="system" />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("observe.system")).toHaveLength(1));
  await act(async () =>
    wire.reply("observe.system", {
      memoryTotalBytes: 20 * gib,
      memoryAvailableBytes: 13.9 * gib,
      swapTotalBytes: 4 * gib,
      swapUsedBytes: 0.1 * gib,
      cpuPressure: { some: { avg10: 1.25 } },
      memoryPressure: null,
      ioPressure: null,
      homeFilesystem: { totalBytes: 100 * gib, usedBytes: 25 * gib, availableBytes: 75 * gib },
    }),
  );
  expect(await screen.findByText("6.1/20GB")).toBeInTheDocument();
  expect(screen.getByText("1.25%")).toBeInTheDocument();
  expect(wire.requests("observe.ports")).toHaveLength(0);
  fireEvent.click(screen.getByRole("button", { name: "실행 목록" }));
  expect(navigate).toHaveBeenCalledWith({ to: "/runs", search: {} });
});

test("ports overview lists owners and retries failures without querying system metrics", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <SystemOverview view="ports" />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("observe.ports")).toHaveLength(1));
  await act(async () => wire.fail("observe.ports"));
  expect(await screen.findByRole("alert")).toHaveTextContent("포트를 불러오지 못했습니다");
  fireEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  await waitFor(() => expect(wire.requests("observe.ports")).toHaveLength(2));
  await act(async () =>
    wire.reply("observe.ports", [
      {
        port: 8080,
        proto: "tcp",
        addr: "127.0.0.1",
        pid: 123,
        process: "owned",
        projectId: "p",
        runId: "run-1",
        containerId: null,
      },
    ]),
  );
  expect(await screen.findByText("8080")).toBeInTheDocument();
  expect(screen.getByText("owned")).toBeInTheDocument();
  expect(screen.getByText("123")).toBeInTheDocument();
  expect(wire.requests("observe.system")).toHaveLength(0);
});
