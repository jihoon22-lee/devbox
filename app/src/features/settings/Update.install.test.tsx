import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { Update, UpdateStatus } from "./Update";

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), off: vi.fn() }));
const rpc = vi.hoisted(() => ({ call: vi.fn(), state: "ready", generation: 1 }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: native.listen }));
vi.mock("../../rpc/RpcProvider", () => ({
  useRpcSession: () => ({ client: rpc, state: rpc.state, generation: rpc.generation }),
}));
let receive: (event: { payload: unknown }) => void;
const ready = {
  enabled: true,
  disabledReason: null,
  phase: "ready",
  generation: 7,
  revision: 10,
  currentVersion: "1.3.0",
  availableVersion: "1.4.0",
  receivedBytes: 100,
  totalBytes: 100,
  error: null,
  nextAction: "install",
};
const summary = { generation: 7, confirmationId: 81, version: "1.4.0", nativeActive: 0, blocked: null };
const agents = {
  rev: "8",
  items: [
    { state: "running", toolExited: false },
    { state: "waiting", toolExited: false },
    { state: "review", toolExited: true },
    { state: "closed", toolExited: true },
    { state: "failed", toolExited: false },
  ],
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
beforeEach(() => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  native.off.mockReset();
  native.listen.mockReset().mockImplementation(async (_name, listener) => {
    receive = listener;
    return native.off;
  });
  native.invoke.mockReset().mockImplementation(async (command) => {
    if (command === "update_status") return ready;
    if (command === "update_install_summary") return summary;
    if (command === "update_install_confirm")
      return { ...ready, phase: "installing", generation: 8, revision: 11, nextAction: "cancel" };
    throw new Error("unexpected native command");
  });
  rpc.state = "ready";
  rpc.generation = 1;
  rpc.call.mockReset().mockImplementation(async (command) => {
    if (command === "agents.list") return agents;
    if (command === "runtime.summary") return { rev: "8", active: 7, activeServices: 3 };
    throw new Error("unexpected daemon command");
  });
});
afterEach(() => {
  cleanup();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});
async function open() {
  render(<Update />);
  fireEvent.click(await screen.findByRole("button", { name: "업데이트" }));
  return screen.findByRole("dialog", { name: "업데이트 설치" });
}
const confirms = () => native.invoke.mock.calls.filter(([name]) => name === "update_install_confirm");

test("ready requires fresh summary and explicit confirmation before any install command", async () => {
  render(<Update />);
  const button = await screen.findByRole("button", { name: "업데이트" });
  expect(native.invoke.mock.calls.map(([name]) => name)).toEqual(["update_status"]);
  fireEvent.click(button);
  const dialog = await screen.findByRole("dialog", { name: "업데이트 설치" });
  expect(await within(dialog).findByText("에이전트 작업: 실행 중 2개 · 열린 에이전트 작업 4개")).toBeVisible();
  expect(within(dialog).getByText("서비스: 실행 중 3개 · 작업·서비스 전체 7개")).toBeVisible();
  expect(within(dialog).getByText(/WSL의 에이전트 작업과 서비스는 계속 실행됩니다/)).toBeVisible();
  expect(native.invoke).toHaveBeenCalledWith("update_install_summary", { generation: 7 });
  expect(confirms()).toHaveLength(0);
  fireEvent.click(within(dialog).getByRole("button", { name: "설치하고 다시 시작" }));
  await waitFor(() => expect(confirms()).toEqual([["update_install_confirm", { generation: 7, confirmationId: 81 }]]));
  expect(await screen.findByRole("button", { name: "설치 준비 취소" })).toBeVisible();
});

test("canceling confirmation or unmounting never installs or stops any running work", async () => {
  const dialog = await open();
  fireEvent.click(within(dialog).getByRole("button", { name: "취소" }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  cleanup();
  expect(confirms()).toHaveLength(0);
  expect(native.invoke.mock.calls.some(([name]) => name === "update_cancel")).toBe(false);
  expect(rpc.call.mock.calls.every(([name]) => ["agents.list", "runtime.summary"].includes(name))).toBe(true);
});

test("duplicate confirm is latched and keeps cancellation disabled until the immediate acknowledgment", async () => {
  const dialog = await open();
  const button = within(dialog).getByRole("button", { name: "설치하고 다시 시작" });
  await waitFor(() => expect(button).toBeEnabled());
  const pending = deferred<unknown>();
  native.invoke.mockImplementationOnce(() => pending.promise);
  fireEvent.click(button);
  fireEvent.click(button);
  expect(confirms()).toHaveLength(1);
  expect(within(dialog).getByRole("button", { name: "취소" })).toBeDisabled();
  await act(async () =>
    pending.resolve({ ...ready, phase: "installing", generation: 8, revision: 11, nextAction: "cancel" }),
  );
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

test.each([
  ["nativeActive", 2, /앱에서 실행한 Windows 작업 2개/],
  ["nativeUnknown", null, /Windows 작업 상태를 확인하지 못했습니다/],
  ["notReady", 0, /업데이트 파일 상태가 바뀌었습니다/],
  ["busy", 0, /다른 업데이트 준비가 진행 중입니다/],
] as const)("native %s blocks confirmation with a recovery action", async (blocked, nativeActive, message) => {
  native.invoke.mockImplementation(async (name) =>
    name === "update_status" ? ready : { ...summary, blocked, nativeActive, confirmationId: null },
  );
  const dialog = await open();
  expect(await within(dialog).findByText(message)).toBeVisible();
  expect(within(dialog).getByRole("button", { name: "설치하고 다시 시작" })).toBeDisabled();
  expect(within(dialog).getByRole("button", { name: "다시 조회" })).toBeEnabled();
  expect(confirms()).toHaveLength(0);
});

test.each(["agents.list", "runtime.summary"])(
  "failed %s summary is unknown rather than zero and retry refreshes the ticket",
  async (failed) => {
    const original = rpc.call.getMockImplementation()!;
    rpc.call.mockImplementation(async (...args) => {
      if (args[0] === failed) throw new Error("https://private.invalid/raw");
      return original(...args);
    });
    const dialog = await open();
    expect(
      await within(dialog).findByText("실행 요약을 조회하지 못했습니다. 연결을 확인하고 다시 조회하세요."),
    ).toBeVisible();
    expect(within(dialog).getByRole("button", { name: "설치하고 다시 시작" })).toBeDisabled();
    expect(dialog).not.toHaveTextContent("실행 중 0개");
    expect(dialog).not.toHaveTextContent("private.invalid");
    rpc.call.mockImplementation(original);
    native.invoke.mockResolvedValueOnce({ ...summary, confirmationId: 82 });
    fireEvent.click(within(dialog).getByRole("button", { name: "다시 조회" }));
    await waitFor(() => expect(within(dialog).getByRole("button", { name: "설치하고 다시 시작" })).toBeEnabled());
    fireEvent.click(within(dialog).getByRole("button", { name: "설치하고 다시 시작" }));
    await waitFor(() => expect(confirms()[0]?.[1]).toEqual({ generation: 7, confirmationId: 82 }));
  },
);

test("disconnected daemon explicitly blocks confirmation without querying stale cached counts", async () => {
  rpc.state = "reconnecting";
  const dialog = await open();
  expect(
    await within(dialog).findByText("실행 요약을 조회하지 못했습니다. 연결을 확인하고 다시 조회하세요."),
  ).toBeVisible();
  expect(within(dialog).getByRole("button", { name: "설치하고 다시 시작" })).toBeDisabled();
  expect(rpc.call).not.toHaveBeenCalled();
});

test("a changed ready generation closes confirmation and drops its late summary instead of confirming a replacement", async () => {
  const pending = deferred<unknown>();
  native.invoke.mockImplementation(async (name) => (name === "update_status" ? ready : pending.promise));
  const dialog = await open();
  expect(within(dialog).getByRole("button", { name: "설치하고 다시 시작" })).toBeDisabled();
  act(() => receive({ payload: { ...ready, generation: 9, revision: 20 } }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  await act(async () => pending.resolve(summary));
  expect(confirms()).toHaveLength(0);
});

test("a stale ticket rejection keeps the app visible and requires a new confirmation", async () => {
  const dialog = await open();
  const button = within(dialog).getByRole("button", { name: "설치하고 다시 시작" });
  await waitFor(() => expect(button).toBeEnabled());
  native.invoke.mockRejectedValueOnce("staleOperation");
  fireEvent.click(button);
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  expect(screen.getByRole("alert")).toHaveTextContent("현재 상태를 다시 불러오세요");
  expect(screen.getByRole("heading", { name: "앱 업데이트" })).toBeVisible();
});

test.each(["quitRejected", "nativeActive", "nativeUnknown", "runner"])(
  "install rejection %s leaves the app and ready state visible",
  async (error) => {
    const dialog = await open();
    const button = within(dialog).getByRole("button", { name: "설치하고 다시 시작" });
    await waitFor(() => expect(button).toBeEnabled());
    native.invoke.mockRejectedValueOnce(error);
    fireEvent.click(button);
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getByRole("alert")).toBeVisible();
    expect(screen.getByText("업데이트 파일 준비 완료")).toBeVisible();
    expect(
      native.invoke.mock.calls.every(([name]) =>
        ["update_status", "update_install_summary", "update_install_confirm"].includes(name),
      ),
    ).toBe(true);
  },
);

test("precommit cancellation uses the newest native generation and committed state has no cancel action", async () => {
  native.invoke.mockResolvedValueOnce({
    ...ready,
    phase: "installing",
    generation: 8,
    revision: 11,
    nextAction: "cancel",
  });
  render(<Update />);
  const cancel = await screen.findByRole("button", { name: "설치 준비 취소" });
  native.invoke.mockResolvedValueOnce({
    ...ready,
    phase: "installing",
    generation: 9,
    revision: 12,
    nextAction: "none",
    error: "cancelled",
  });
  fireEvent.click(cancel);
  await screen.findByText("설치 준비를 취소하고 있습니다.");
  expect(native.invoke).toHaveBeenLastCalledWith("update_cancel", { generation: 8 });
  expect(screen.queryByRole("button", { name: "설치 준비 취소" })).not.toBeInTheDocument();
  act(() => receive({ payload: { ...ready, phase: "installing", generation: 10, revision: 13, nextAction: "none" } }));
  expect(screen.getByText("앱 종료 후 업데이트를 설치합니다.")).toBeVisible();
  expect(screen.queryByRole("button", { name: "설치 준비 취소" })).not.toBeInTheDocument();
});

test("unknown runner cleanup is a wait state, never a committed-install or retry claim", async () => {
  native.invoke.mockResolvedValueOnce({
    ...ready,
    phase: "installing",
    revision: 12,
    nextAction: "none",
    error: "runner",
  });
  render(<Update />);
  expect(
    await screen.findByText("업데이트 설치 도우미 정리를 확인하고 있습니다. 확인이 끝날 때까지 기다리세요."),
  ).toBeVisible();
  expect(screen.queryByText("앱 종료 후 업데이트를 설치합니다.")).not.toBeInTheDocument();
  expect(screen.queryByRole("button")).not.toBeInTheDocument();
});

test.each([
  [false, "runner", "업데이트 설치 도우미 정리를 확인하고 있습니다. 확인이 끝날 때까지 기다리세요."],
  [
    false,
    "recoveryRequired",
    "업데이트 설치 상태를 확인할 수 없습니다. 설치 프로그램이 끝났는지 확인한 뒤 앱을 다시 시작하세요.",
  ],
  [
    true,
    "recoveryRequired",
    "업데이트 설치 상태를 확인할 수 없습니다. 설치 프로그램이 끝났는지 확인한 뒤 앱을 다시 시작하세요.",
  ],
] as const)("recovery remains visible with public updater enabled=%s and error=%s", async (enabled, error, message) => {
  native.invoke.mockResolvedValue({
    ...ready,
    enabled,
    disabledReason: enabled ? null : "channel",
    phase: "installing",
    nextAction: "none",
    error,
  });
  render(
    <>
      <Update />
      <UpdateStatus />
    </>,
  );
  expect(await screen.findByText(message)).toBeVisible();
  const chip = screen.getByRole("button", { name: "앱 업데이트 정보" });
  expect(chip).toHaveTextContent(error === "runner" ? "업데이트 준비 상태 확인 중" : "업데이트 설치 상태 확인 필요");
  expect(screen.getAllByRole("button")).toHaveLength(1);
  expect(native.invoke.mock.calls.every(([name]) => name === "update_status")).toBe(true);
});

test.each([
  { ...summary, confirmationId: null },
  { ...summary, confirmationId: Number.MAX_SAFE_INTEGER + 1 },
  { ...summary, generation: 6 },
  { ...summary, nativeActive: 1 },
  { ...summary, version: "https://private.invalid" },
])("malformed or stale native summary %j cannot authorize confirmation", async (value) => {
  native.invoke.mockImplementation(async (name) => (name === "update_status" ? ready : value));
  const dialog = await open();
  expect(
    await within(dialog).findByText("설치 준비 상태를 확인하지 못했습니다. 상태를 확인하고 다시 조회하세요."),
  ).toBeVisible();
  expect(within(dialog).getByRole("button", { name: "설치하고 다시 시작" })).toBeDisabled();
  expect(confirms()).toHaveLength(0);
  expect(dialog).not.toHaveTextContent("private.invalid");
});

test("blocked NotReady retains its specific cause when native falls back to the current version", async () => {
  native.invoke.mockImplementation(async (name) =>
    name === "update_status"
      ? ready
      : {
          ...summary,
          confirmationId: null,
          blocked: "notReady",
          version: ready.currentVersion,
        },
  );
  const dialog = await open();
  expect(await within(dialog).findByText("업데이트 파일 상태가 바뀌었습니다. 상태를 다시 확인하세요.")).toBeVisible();
  expect(within(dialog).getByRole("button", { name: "설치하고 다시 시작" })).toBeDisabled();
  expect(confirms()).toHaveLength(0);
});

test("unblocked ticket for a different valid version never authorizes the captured confirmation", async () => {
  native.invoke.mockImplementation(async (name) =>
    name === "update_status" ? ready : { ...summary, version: "1.5.0" },
  );
  const dialog = await open();
  expect(
    await within(dialog).findByText("설치 준비 상태를 확인하지 못했습니다. 상태를 확인하고 다시 조회하세요."),
  ).toBeVisible();
  expect(within(dialog).getByRole("button", { name: "설치하고 다시 시작" })).toBeDisabled();
  expect(confirms()).toHaveLength(0);
});
