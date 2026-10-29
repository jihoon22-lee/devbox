import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NativeMainController } from "./NativeMainController";
import { useNativeRuntime } from "./nativeRuntime";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), label: "main" }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ label: mocks.label }) }));
let listeners: Map<string, (event: { payload: unknown }) => void>;
beforeEach(() => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  mocks.label = "main";
  mocks.invoke
    .mockReset()
    .mockImplementation(async (name) =>
      name === "app_preferences" ? { autostartAsked: false, autostart: false, trayNoticeShown: false } : undefined,
    );
  listeners = new Map();
  mocks.listen.mockReset().mockImplementation(async (name, callback) => {
    listeners.set(name, callback);
    return () => listeners.delete(name);
  });
  useNativeRuntime.setState({ openTasks: null, intents: [] });
});
afterEach(() => {
  cleanup();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});
test("quit remains safe before the connection gate is ready and unknown tasks require cancellation-first confirmation", async () => {
  render(<NativeMainController ready={false} />);
  await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("app_intents_ready"));
  expect(mocks.invoke).not.toHaveBeenCalledWith("app_preferences");
  act(() => listeners.get("devbox://quit")?.({ payload: null }));
  expect(await screen.findByRole("dialog", { name: "devbox 종료" })).toHaveTextContent("확인하지 못했습니다");
  expect(screen.getByRole("button", { name: "취소" })).toHaveFocus();
  await userEvent.click(screen.getByRole("button", { name: "취소" }));
  expect(mocks.invoke).not.toHaveBeenCalledWith("quit_app");
});
test("first run appears only ready on main, records choice and does not reopen on reconnection", async () => {
  const view = render(<NativeMainController ready={false} />);
  view.rerender(<NativeMainController ready />);
  expect(await screen.findByRole("checkbox")).toBeChecked();
  await userEvent.click(screen.getByRole("button", { name: "시작하기" }));
  await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("set_autostart", { enabled: true, firstRun: true }));
  view.rerender(<NativeMainController ready={false} />);
  view.rerender(<NativeMainController ready />);
  expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  expect(mocks.invoke.mock.calls.filter(([name]) => name === "app_preferences")).toHaveLength(1);
});
test("known open task count survives disconnection and full exit uses native exit command", async () => {
  useNativeRuntime.setState({ openTasks: 3 });
  render(<NativeMainController ready={false} />);
  await waitFor(() => expect(listeners.has("devbox://quit")).toBe(true));
  act(() => listeners.get("devbox://quit")?.({ payload: null }));
  expect(await screen.findByRole("dialog")).toHaveTextContent("에이전트 3개");
  expect(screen.getByRole("dialog")).toHaveTextContent("약 15초 뒤");
  await userEvent.click(screen.getByRole("button", { name: "종료" }));
  expect(mocks.invoke).toHaveBeenCalledWith("quit_app");
});
test("popouts never register global intents or read first run settings", async () => {
  mocks.label = "popout-terminal-owned";
  render(<NativeMainController ready />);
  await act(async () => {});
  expect(mocks.invoke).not.toHaveBeenCalled();
  expect(mocks.listen).not.toHaveBeenCalled();
});
test("zero task count from a disconnected snapshot still requires a warning", async () => {
  useNativeRuntime.setState({ openTasks: 0, snapshotCurrent: false });
  render(<NativeMainController ready={false} />);
  await waitFor(() => expect(listeners.has("devbox://quit")).toBe(true));
  act(() => listeners.get("devbox://quit")?.({ payload: null }));
  expect(await screen.findByRole("dialog")).toHaveTextContent("확인하지 못했습니다");
  expect(mocks.invoke).not.toHaveBeenCalledWith("quit_app");
});
test("a current confirmed empty snapshot can quit directly and pending quit requests do not duplicate exit", async () => {
  useNativeRuntime.setState({ openTasks: 0, snapshotCurrent: true });
  let finish!: () => void;
  mocks.invoke.mockImplementation((name) =>
    name === "quit_app"
      ? new Promise<void>((resolve) => {
          finish = resolve;
        })
      : Promise.resolve(undefined),
  );
  render(<NativeMainController ready={false} />);
  await waitFor(() => expect(listeners.has("devbox://quit")).toBe(true));
  act(() => {
    listeners.get("devbox://quit")?.({ payload: null });
    listeners.get("devbox://quit")?.({ payload: null });
  });
  expect(mocks.invoke.mock.calls.filter(([name]) => name === "quit_app")).toHaveLength(1);
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  await act(async () => finish());
});
test("native palette events carry only a numeric trace ID into the intent queue", async () => {
  render(<NativeMainController ready={false} />);
  await waitFor(() => expect(listeners.has("devbox://palette")).toBe(true));
  act(() => listeners.get("devbox://palette")?.({ payload: { traceId: 5 } }));
  act(() => listeners.get("devbox://palette")?.({ payload: { traceId: null } }));
  act(() => listeners.get("devbox://palette")?.({ payload: null }));
  expect(useNativeRuntime.getState().intents).toEqual([
    { kind: "palette", traceId: 5 },
    { kind: "palette" },
    { kind: "palette" },
  ]);
});

type Saved = { autostartAsked: boolean; autostart: boolean; trayNoticeShown: boolean };
/** A native store that keeps what `set_autostart` saved, as settings.json does across app restarts. */
function nativeStore(initial: Partial<Saved> = {}, failSave = false) {
  const saved: Saved = { autostartAsked: false, autostart: false, trayNoticeShown: false, ...initial };
  mocks.invoke.mockReset().mockImplementation(async (name, args) => {
    if (name === "app_preferences") return { ...saved };
    if (name === "set_autostart") {
      if (failSave) throw new Error("owned fixture failure");
      saved.autostart = args.enabled;
      saved.autostartAsked ||= args.firstRun;
      return { ...saved };
    }
    return undefined;
  });
  return saved;
}
const reads = () => mocks.invoke.mock.calls.filter(([name]) => name === "app_preferences").length;
const firstRun = () => screen.queryByRole("dialog", { name: "devbox 시작하기" });

test("later_is_remembered_across_restarts", async () => {
  const saved = nativeStore();
  const first = render(<NativeMainController ready />);
  await userEvent.click(await screen.findByRole("button", { name: "나중에" }));
  expect(firstRun()).not.toBeInTheDocument();
  await waitFor(() => expect(saved.autostartAsked).toBe(true));
  // Asking later never turns autostart on.
  expect(saved.autostart).toBe(false);
  expect(mocks.invoke.mock.calls.filter(([name]) => name === "set_autostart")).toEqual([
    ["set_autostart", { enabled: false, firstRun: true }],
  ]);
  first.unmount();
  // The app starts again and reads the same native settings: nothing is asked.
  const before = reads();
  render(<NativeMainController ready />);
  await waitFor(() => expect(reads()).toBeGreaterThan(before));
  await act(async () => {});
  expect(firstRun()).not.toBeInTheDocument();
});

test("Escape closes the first-run dialog as 나중에 does", async () => {
  const saved = nativeStore();
  render(<NativeMainController ready />);
  await screen.findByRole("dialog", { name: "devbox 시작하기" });
  await userEvent.keyboard("{Escape}");
  await waitFor(() => expect(saved.autostartAsked).toBe(true));
  expect(saved.autostart).toBe(false);
});

test("나중에 leaves an autostart that is already on exactly as it is", async () => {
  const saved = nativeStore({ autostart: true });
  render(<NativeMainController ready />);
  await userEvent.click(await screen.findByRole("button", { name: "나중에" }));
  await waitFor(() => expect(saved.autostartAsked).toBe(true));
  expect(saved.autostart).toBe(true);
});

test("a failed save of 나중에 closes the dialog, says so, and 다시 시도 asks again", async () => {
  const saved = nativeStore({}, true);
  render(<NativeMainController ready />);
  await userEvent.click(await screen.findByRole("button", { name: "나중에" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("첫 실행 선택을 저장하지 못했습니다");
  expect(firstRun()).not.toBeInTheDocument();
  expect(saved.autostartAsked).toBe(false);
  await userEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  expect(await screen.findByRole("dialog", { name: "devbox 시작하기" })).toBeInTheDocument();
});
