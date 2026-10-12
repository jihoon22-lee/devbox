import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ConnectionGate, GateView } from "./ConnectionGate";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
const status = (state: string, revision = "1") => ({ state, revision, instance: "prod" });
beforeEach(() => {
  mocks.invoke.mockReset();
  mocks.listen.mockReset();
  mocks.listen.mockResolvedValue(vi.fn());
});
test("event wins over stale snapshot; ready children stay mounted through bridge recovery", async () => {
  let snapshot!: (s: unknown) => void;
  mocks.invoke.mockReturnValue(
    new Promise((resolve) => {
      snapshot = resolve;
    }),
  );
  render(<ConnectionGate>{(instance) => <input aria-label={instance} defaultValue="retained" />}</ConnectionGate>);
  await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("gate_status"));
  const event = mocks.listen.mock.calls[0]?.[1];
  act(() => event({ payload: status("ready", "10") }));
  await act(async () => snapshot(status("checking", "9")));
  expect(screen.getByLabelText("prod")).toHaveValue("retained");
  act(() => event({ payload: status("bridgeFailed", "11") }));
  expect(screen.getByLabelText("prod")).toHaveValue("retained");
});
test("retry rejection is actionable and pending listeners clean up after unmount", async () => {
  mocks.invoke.mockResolvedValueOnce(status("noWsl")).mockRejectedValueOnce({ kind: "busy" });
  const view = render(<ConnectionGate>content</ConnectionGate>);
  await screen.findByText("WSL이 필요합니다");
  await userEvent.click(screen.getByText("다시 시도"));
  await screen.findByText(/다시 시도하지 못했습니다/);
  view.unmount();
  let resolve!: (off: () => void) => void;
  const off = vi.fn();
  mocks.listen.mockReturnValue(
    new Promise((r) => {
      resolve = r;
    }),
  );
  const second = render(<ConnectionGate>content</ConnectionGate>);
  second.unmount();
  await act(async () => resolve(off));
  expect(off).toHaveBeenCalledOnce();
});
test("systemd instructions warn that shutdown ends running programs", () => {
  render(
    <GateView
      status={{ ...status("systemdOff"), state: "systemdOff", instance: "dev", detail: "Ubuntu" }}
      onRetry={() => {}}
    />,
  );
  expect(screen.getByText(/wsl --shutdown/)).toHaveTextContent("실행 중인 WSL 프로그램이 모두 종료됩니다");
});
test("failed event subscription can be retried without constructing children", async () => {
  mocks.listen.mockRejectedValueOnce({ kind: "unavailable" }).mockResolvedValueOnce(vi.fn());
  mocks.invoke.mockImplementation(async (cmd) => (cmd === "gate_status" ? status("ready", "2") : undefined));
  render(<ConnectionGate>native child</ConnectionGate>);
  await screen.findByRole("alert");
  expect(screen.queryByText("native child")).not.toBeInTheDocument();
  await userEvent.click(screen.getByText("다시 시도"));
  await screen.findByText("native child");
  expect(mocks.invoke).toHaveBeenCalledWith("gate_retry", { distro: null });
});
test("distro selection is forwarded and revision remains monotonic after retry", async () => {
  mocks.invoke.mockImplementation(async (cmd) =>
    cmd === "gate_status"
      ? { ...status("chooseDistro", "1"), distros: [{ name: "Ubuntu", version: 2, default: true }] }
      : undefined,
  );
  render(<ConnectionGate>native child</ConnectionGate>);
  await userEvent.click(await screen.findByRole("button", { name: "Ubuntu 사용" }));
  await waitFor(() => expect(mocks.listen).toHaveBeenCalledTimes(2));
  expect(mocks.invoke).toHaveBeenCalledWith("gate_retry", { distro: "Ubuntu" });
  act(() => mocks.listen.mock.calls[1]?.[1]({ payload: status("ready", "3") }));
  expect(screen.getByText("native child")).toBeInTheDocument();
});
test("post-ready recovery retains user state and permits repeated retry and distro selection", async () => {
  let latest = {
    ...status("ready", "1"),
    detail: "",
    distros: [] as { name: string; version: number; default: boolean }[],
  };
  mocks.invoke.mockImplementation(async (cmd) => (cmd === "gate_status" ? latest : undefined));
  render(
    <ConnectionGate>
      <input aria-label="retained state" defaultValue="initial" />
    </ConnectionGate>,
  );
  const input = await screen.findByLabelText("retained state");
  await userEvent.type(input, " user edit");
  const publish = (state: string, revision: string) => {
    latest = {
      ...latest,
      ...status(state, revision),
      detail: "units FAIL",
      distros: [{ name: "Ubuntu", version: 2, default: true }],
    };
    act(() => mocks.listen.mock.calls.at(-1)?.[1]({ payload: latest }));
  };
  publish("setupFailed", "2");
  expect(screen.getByText("units FAIL")).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  await waitFor(() => expect(mocks.listen).toHaveBeenCalledTimes(2));
  publish("bridgeFailed", "3");
  await userEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  await waitFor(() => expect(mocks.listen).toHaveBeenCalledTimes(3));
  publish("chooseDistro", "4");
  await userEvent.click(screen.getByRole("button", { name: "Ubuntu 사용" }));
  await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("gate_retry", { distro: "Ubuntu" }));
  expect(mocks.invoke.mock.calls.filter((c) => c[0] === "gate_retry")).toHaveLength(3);
  expect(screen.getByLabelText("retained state")).toBe(input);
  expect(input).toHaveValue("initial user edit");
});
