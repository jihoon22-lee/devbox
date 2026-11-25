import { render, screen, within } from "@testing-library/react";
import type { PortInfo } from "../../rpc/gen/rpc";
import { PortsTable, portBrowserUrl } from "./ObservationTables";

const port = (number: number, projectId: string | null, containerId: string | null = null): PortInfo => ({
  port: number,
  proto: "tcp",
  addr: "127.0.0.1",
  pid: null,
  process: null,
  startTimeMs: null,
  runId: null,
  containerId,
  projectId,
});
const actions = { busy: false, request: () => {} };
const shownPorts = () =>
  screen
    .getAllByRole("row")
    .slice(1)
    .map((row) => within(row).getAllByRole("cell")[0]?.textContent);

test("the current project's ports come first, marked, and every group keeps its order", () => {
  const ports = [port(3000, null), port(5432, "p", "abc"), port(6379, "q", "def"), port(8080, "p"), port(9000, null)];
  const { rerender } = render(<PortsTable ports={ports} actions={actions} projectId="p" />);
  expect(shownPorts()).toEqual(["5432 현재 프로젝트", "8080 현재 프로젝트", "3000", "6379", "9000"]);
  const [, first, second, third] = screen.getAllByRole("row");
  expect(within(first!).getByText("현재 프로젝트")).toBeVisible();
  expect(within(second!).getByText("현재 프로젝트")).toBeVisible();
  expect(within(third!).queryByText("현재 프로젝트")).not.toBeInTheDocument();
  expect(ports.map((row) => row.port)).toEqual([3000, 5432, 6379, 8080, 9000]);

  // Without a current project nothing is reordered or marked.
  rerender(<PortsTable ports={ports} actions={actions} />);
  expect(shownPorts()).toEqual(["3000", "5432", "6379", "8080", "9000"]);
  expect(screen.queryByText("현재 프로젝트")).not.toBeInTheDocument();
});

test.each([
  ["0.0.0.0", "http://127.0.0.1:8080"],
  ["127.0.0.1", "http://127.0.0.1:8080"],
  ["::", "http://[::1]:8080"],
  ["::1", "http://[::1]:8080"],
  ["192.168.1.2", "http://192.168.1.2:8080"],
  ["2001:db8::1", "http://[2001:db8::1]:8080"],
  ["::ffff:127.0.0.1", "http://[::ffff:7f00:1]:8080"],
])("browser uses a valid address for %s", (addr, expected) => {
  expect(portBrowserUrl({ proto: "tcp", addr, port: 8080 })).toBe(expected);
});
test("browser rejects non-TCP, invalid ports and non-IP addresses", () => {
  for (const addr of ["example.com", "abc", "127.0.0.1/path", "127.0.0.1@evil", "999.1.1.1", ":::1"])
    expect(portBrowserUrl({ proto: "tcp", addr, port: 8080 })).toBeUndefined();
  for (const port of [0, -1, 65536, 1.5])
    expect(portBrowserUrl({ proto: "tcp", addr: "127.0.0.1", port })).toBeUndefined();
  expect(portBrowserUrl({ proto: "udp", addr: "127.0.0.1", port: 8080 })).toBeUndefined();
});
