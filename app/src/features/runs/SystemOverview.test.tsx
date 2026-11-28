import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RpcProvider } from "../../rpc/RpcProvider";
import { ProjectWire } from "../projects/test-wire";
import { SystemOverview } from "./SystemOverview";

const gib = 1024 ** 3;

test("system overview shows actual resource values without requesting ports", async () => {
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
        startTimeMs: 1,
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

test("ports overview of a project says its ports come first and marks them", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <SystemOverview view="ports" projectId="p" />
    </RpcProvider>,
  );
  expect(screen.getByText("현재 프로젝트 관련 포트를 먼저 표시합니다.")).toBeVisible();
  await waitFor(() => expect(wire.requests("observe.ports")).toHaveLength(1));
  const row = (port: number, projectId: string | null) => ({
    port,
    proto: "tcp",
    addr: "127.0.0.1",
    pid: null,
    process: null,
    startTimeMs: null,
    runId: null,
    containerId: projectId ? "abc" : null,
    projectId,
  });
  await act(async () => wire.reply("observe.ports", [row(3000, null), row(8080, "p")]));
  const rows = await screen.findAllByRole("row");
  expect(rows[1]).toHaveTextContent("8080");
  expect(rows[1]).toHaveTextContent("현재 프로젝트");
  expect(rows[2]).toHaveTextContent("3000");
  expect(rows[2]).not.toHaveTextContent("현재 프로젝트");
});

test("user units overview polls only its own read-only query and retries a failure", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <SystemOverview view="units" projectId="p" />
    </RpcProvider>,
  );
  expect(screen.getByRole("heading", { name: "사용자 서비스" })).toBeVisible();
  await waitFor(() => expect(wire.requests("observe.units")).toHaveLength(1));
  expect(wire.request("observe.units").params).toEqual({});
  await act(async () => wire.fail("observe.units"));
  expect(await screen.findByRole("alert")).toHaveTextContent("사용자 서비스를 불러오지 못했습니다");
  fireEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  await waitFor(() => expect(wire.requests("observe.units")).toHaveLength(2));
  await act(async () =>
    wire.reply("observe.units", {
      available: true,
      truncated: false,
      items: [
        {
          name: "app-backup.timer",
          description: "Nightly app backup",
          loadState: "loaded",
          activeState: "active",
          subState: "waiting",
          kind: "timer",
        },
        {
          name: "app-web.service",
          description: "App web server",
          loadState: "loaded",
          activeState: "active",
          subState: "running",
          kind: "service",
          projectId: "p",
        },
      ],
    }),
  );
  const rows = await screen.findAllByRole("row");
  expect(rows[1]).toHaveTextContent("app-web.service 현재 프로젝트");
  expect(rows[2]).toHaveTextContent("app-backup.timer");
  for (const method of ["observe.system", "observe.ports", "observe.processes"])
    expect(wire.requests(method)).toHaveLength(0);
  expect(screen.queryByRole("button")).toBeNull(); // No action starts or stops a user unit.
});

test("a user manager that does not answer in time is named as the cause, with the next step", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <SystemOverview view="units" />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("observe.units")).toHaveLength(1));
  await act(async () =>
    wire.deliver({
      type: "response",
      id: wire.request("observe.units").id,
      error: { code: "observe.source_unavailable" },
    }),
  );
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent(
    "systemd 사용자 관리자가 제때 답하지 않았거나 답을 읽지 못해 사용자 서비스를 불러오지 못했습니다",
  );
  expect(alert).toHaveTextContent("다시 시도하고, 계속되면 터미널에서 systemctl --user status를 확인하세요.");
  fireEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  await waitFor(() => expect(wire.requests("observe.units")).toHaveLength(2));
  // Another failure keeps the plain title and no systemd hint.
  await act(async () => wire.fail("observe.units"));
  await waitFor(() => expect(screen.getByRole("alert")).not.toHaveTextContent("systemctl"));
  expect(screen.getByRole("alert")).toHaveTextContent("사용자 서비스를 불러오지 못했습니다");
});
