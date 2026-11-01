import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
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
const failed = (state: string, revision: string, detail: string) => ({ ...status(state, revision), detail });
const tail = "devbox daemon starting\nerror: config.toml line 3: expected `=`\n";
test("자세히 shows the failed step and the log tail with 복사, and survives the next automatic attempt", async () => {
  const writeText = vi.fn(async (_text: string) => {});
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  mocks.invoke.mockImplementation(async (cmd) =>
    cmd === "gate_status"
      ? failed("bridgeFailed", "1", "bridge exited with status 1")
      : cmd === "daemon_log_tail"
        ? tail
        : undefined,
  );
  render(<ConnectionGate>native child</ConnectionGate>);
  expect(await screen.findByText("WSL 쪽 devbox에 연결하지 못했습니다")).toBeInTheDocument();
  expect(screen.getByText("[자세히]에서 원인을 확인하고 [다시 시도]하세요.")).toBeInTheDocument();
  expect(mocks.invoke).not.toHaveBeenCalledWith("daemon_log_tail", expect.anything());
  await userEvent.click(screen.getByRole("button", { name: "자세히" }));
  expect(mocks.invoke).toHaveBeenCalledWith("daemon_log_tail", { lines: 200 });
  const details = await screen.findByRole("region", { name: "연결 실패 자세히" });
  expect(await within(details).findByText(/expected `=`/)).toBeInTheDocument();
  expect(within(details).getByText("bridge exited with status 1")).toBeInTheDocument();
  await userEvent.click(within(details).getByRole("button", { name: "복사" }));
  expect(writeText).toHaveBeenCalledOnce();
  expect(writeText.mock.calls[0]?.[0]).toContain("bridge exited with status 1");
  expect(writeText.mock.calls[0]?.[0]).toContain("error: config.toml line 3");
  // The supervisor keeps retrying on its own; the open details must not vanish or refetch meanwhile.
  const publish = mocks.listen.mock.calls[0]?.[1];
  const panel = () => screen.getByRole("region", { name: "연결 실패 자세히" });
  act(() => publish({ payload: status("connecting", "2") }));
  expect(screen.getByText("WSL의 devbox에 연결하고 있습니다")).toBeInTheDocument();
  expect(panel()).toBe(details);
  expect(within(panel()).getByText(/expected `=`/)).toBeInTheDocument();
  act(() => publish({ payload: failed("bridgeFailed", "3", "bridge exited with status 2") }));
  expect(panel()).toBe(details);
  expect(within(panel()).getByText("bridge exited with status 2")).toBeInTheDocument();
  expect(mocks.invoke.mock.calls.filter(([cmd]) => cmd === "daemon_log_tail")).toHaveLength(1);
  act(() => publish({ payload: status("ready", "4") }));
  expect(screen.getByText("native child")).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "연결 실패 자세히" })).toBeNull();
});
test("a failed setup has a fixed body, and the raw text of what failed stays in the details", async () => {
  const raw = "서비스 다시 시작 실패: Failed to connect to bus: No such file or directory";
  mocks.invoke.mockImplementation(async (cmd) =>
    cmd === "gate_status" ? failed("setupFailed", "1", raw) : cmd === "daemon_log_tail" ? "journal tail" : undefined,
  );
  render(<ConnectionGate>native child</ConnectionGate>);
  expect(await screen.findByText("WSL 쪽 설치에 실패했습니다")).toBeInTheDocument();
  expect(screen.getByText("[자세히]에서 원인을 확인하고 [다시 시도]하세요.")).toBeInTheDocument();
  expect(screen.queryByText(raw)).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "자세히" }));
  const details = await screen.findByRole("region", { name: "연결 실패 자세히" });
  expect(within(details).getByText(raw)).toBeInTheDocument();
});
test("no pre-connection copy says 데몬, including details and an unreadable log", async () => {
  const states = [
    "checking",
    "connecting",
    "deploying",
    "noWsl",
    "wsl1",
    "systemdOff",
    "chooseDistro",
    "setupFailed",
    "bridgeFailed",
  ] as const;
  for (const compact of [false, true])
    for (const state of states) {
      const view = render(
        <GateView
          status={{
            ...status(state),
            state,
            instance: "prod",
            detail: "Ubuntu",
            distros: [{ name: "Ubuntu", version: 2, default: true }],
          }}
          onRetry={() => {}}
          onDetails={() => {}}
          compact={compact}
        />,
      );
      expect(view.container.textContent).not.toMatch(/데몬/);
      for (const labelled of view.container.querySelectorAll("[aria-label]"))
        expect(labelled.getAttribute("aria-label")).not.toMatch(/데몬/);
      view.unmount();
    }
  render(
    <GateView
      status={{ ...status("deploying"), state: "deploying", instance: "prod" }}
      onRetry={() => {}}
      onDetails={() => {}}
    />,
  );
  expect(screen.getByText("WSL 쪽 devbox를 준비하고 있습니다")).toBeInTheDocument();
  expect(screen.getByText("내장 실행 파일을 확인하고 필요한 서비스를 시작합니다.")).toBeInTheDocument();
  expect(document.body.textContent).not.toMatch(/터미널 세션은 그대로/);
  cleanup();
  const writeText = vi.fn(async (_text: string) => {});
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  mocks.invoke.mockImplementation(async (cmd) => {
    if (cmd === "gate_status") return failed("setupFailed", "1", "서비스 다시 시작 실패: unit unavailable");
    // A failed run carries journalctl's output in its message.
    if (cmd === "daemon_log_tail")
      return Promise.reject({
        message: "journalctl: No journal files were found.\nAuthorization: Bearer ghp_abcdefghijklmnopqrstuv",
      });
  });
  render(<ConnectionGate>native child</ConnectionGate>);
  await userEvent.click(await screen.findByRole("button", { name: "자세히" }));
  const details = await screen.findByRole("region", { name: "연결 실패 자세히" });
  const shown = await within(details).findByText(/로그를 읽지 못했습니다/);
  expect(within(details).getByText("서비스 다시 시작 실패: unit unavailable")).toBeInTheDocument();
  // A failed read can still be copied, exactly as shown.
  await userEvent.click(within(details).getByRole("button", { name: "복사" }));
  expect(writeText.mock.calls[0]?.[0]).toContain(`최근 로그\n${shown.textContent}\n`);
  expect(shown.textContent).toContain("journalctl: No journal files were found.");
  // That output is redacted like a read tail, on screen and on the clipboard.
  expect(shown).toHaveTextContent("Authorization: Bearer [REDACTED]");
  expect(writeText.mock.calls[0]?.[0]).not.toContain("ghp_");
  expect(document.body.textContent).not.toMatch(/데몬/);
  for (const labelled of document.querySelectorAll("[aria-label]"))
    expect(labelled.getAttribute("aria-label")).not.toMatch(/데몬/);
});
test("the log tail waits before 복사, opens at its newest line and is redacted as it is copied", async () => {
  const writeText = vi.fn(async (_text: string) => {});
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  // jsdom has no layout; give the log box a height to scroll to.
  vi.spyOn(Element.prototype, "scrollHeight", "get").mockReturnValue(4800);
  let deliver!: (text: string) => void;
  mocks.invoke.mockImplementation((cmd) =>
    cmd === "gate_status"
      ? Promise.resolve(failed("bridgeFailed", "1", "bridge exited with status 1"))
      : cmd === "daemon_log_tail"
        ? new Promise((resolve) => {
            deliver = resolve;
          })
        : Promise.resolve(undefined),
  );
  render(<ConnectionGate>native child</ConnectionGate>);
  await userEvent.click(await screen.findByRole("button", { name: "자세히" }));
  const details = await screen.findByRole("region", { name: "연결 실패 자세히" });
  const copy = within(details).getByRole("button", { name: "복사" });
  expect(within(details).getByText("로그를 읽는 중입니다…")).toBeInTheDocument();
  expect(copy).toBeDisabled();
  await act(async () => deliver("starting\nAuthorization: Bearer ghp_abcdefghijklmnopqrstuv\nlast line\n"));
  const box = within(details).getByText(/last line/);
  expect(box).toHaveTextContent("Authorization: Bearer [REDACTED]");
  expect(box.textContent).not.toContain("ghp_");
  expect(box.scrollTop).toBe(4800);
  expect(copy).toBeEnabled();
  await userEvent.click(copy);
  const copied = writeText.mock.calls[0]?.[0] ?? "";
  expect(copied).toContain(`최근 로그\n${box.textContent}\n`);
  expect(copied).toContain("Authorization: Bearer [REDACTED]\nlast line");
  expect(copied).not.toContain("ghp_");
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
  expect(screen.getByText("[자세히]에서 원인을 확인하고 [다시 시도]하세요.")).toBeInTheDocument();
  expect(screen.queryByText("units FAIL")).toBeNull();
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
