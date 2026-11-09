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
  const exits = () => mocks.invoke.mock.calls.filter(([name]) => name === "quit_app");
  // Each request first asks native whether quick capture holds a draft.
  await waitFor(() => expect(exits()).toHaveLength(1));
  await act(async () => {});
  expect(exits()).toHaveLength(1);
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
/** A native store that keeps what the first-run commands saved, as settings.json does across app restarts. */
function nativeStore(initial: Partial<Saved> = {}, failAsked = false) {
  const saved: Saved = { autostartAsked: false, autostart: false, trayNoticeShown: false, ...initial };
  mocks.invoke.mockReset().mockImplementation(async (name, args) => {
    if (name === "app_preferences") return { ...saved };
    if (name === "set_autostart") {
      saved.autostart = args.enabled;
      saved.autostartAsked ||= args.firstRun;
      return { ...saved };
    }
    if (name === "mark_autostart_asked") {
      if (failAsked) throw new Error("owned fixture failure");
      saved.autostartAsked = true;
      return undefined;
    }
    return undefined;
  });
  return saved;
}
const called = (command: string) => mocks.invoke.mock.calls.filter(([name]) => name === command);
const reads = () => called("app_preferences").length;
const firstRun = () => screen.queryByRole("dialog", { name: "devbox 시작하기" });
const overlay = () => document.querySelector("[data-dialog-overlay]") as HTMLElement;

test("later_is_remembered_across_restarts", async () => {
  const saved = nativeStore();
  const first = render(<NativeMainController ready />);
  await userEvent.click(await screen.findByRole("button", { name: "나중에" }));
  expect(firstRun()).not.toBeInTheDocument();
  await waitFor(() => expect(saved.autostartAsked).toBe(true));
  // Asking later saves that the question was asked and nothing else: autostart is neither read nor set.
  expect(saved.autostart).toBe(false);
  expect(called("mark_autostart_asked")).toEqual([["mark_autostart_asked"]]);
  expect(called("set_autostart")).toEqual([]);
  expect(reads()).toBe(1);
  first.unmount();
  // The app starts again and reads the same native settings: nothing is asked.
  render(<NativeMainController ready />);
  await waitFor(() => expect(reads()).toBe(2));
  await act(async () => {});
  expect(firstRun()).not.toBeInTheDocument();
});

test.each([
  ["Escape", () => userEvent.keyboard("{Escape}")],
  ["a click on the overlay", () => userEvent.click(overlay())],
])("%s closes the first-run dialog as 나중에 does", async (_, dismiss) => {
  const saved = nativeStore();
  render(<NativeMainController ready />);
  await screen.findByRole("dialog", { name: "devbox 시작하기" });
  await dismiss();
  await waitFor(() => expect(saved.autostartAsked).toBe(true));
  expect(firstRun()).not.toBeInTheDocument();
  expect(saved.autostart).toBe(false);
  expect(called("mark_autostart_asked")).toHaveLength(1);
  expect(called("set_autostart")).toEqual([]);
});

test("나중에 leaves an autostart that is already on exactly as it is", async () => {
  const saved = nativeStore({ autostart: true });
  render(<NativeMainController ready />);
  await userEvent.click(await screen.findByRole("button", { name: "나중에" }));
  await waitFor(() => expect(saved.autostartAsked).toBe(true));
  expect(saved.autostart).toBe(true);
  expect(called("set_autostart")).toEqual([]);
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

test("시작하기 still saves the explicit choice and the question together", async () => {
  const saved = nativeStore();
  render(<NativeMainController ready />);
  await userEvent.click(await screen.findByRole("checkbox"));
  await userEvent.click(screen.getByRole("button", { name: "시작하기" }));
  await waitFor(() => expect(saved.autostartAsked).toBe(true));
  expect(called("set_autostart")).toEqual([["set_autostart", { enabled: false, firstRun: true }]]);
  expect(called("mark_autostart_asked")).toEqual([]);
});

/** Native says the hidden quick capture window holds a draft; every other command succeeds. */
function captureDraft(present: boolean | Error) {
  mocks.invoke.mockReset().mockImplementation(async (name) => {
    if (name !== "capture_has_draft") return undefined;
    if (present instanceof Error) throw present;
    return present;
  });
}
async function requestQuit() {
  render(<NativeMainController ready={false} />);
  await waitFor(() => expect(listeners.has("devbox://quit")).toBe(true));
  act(() => listeners.get("devbox://quit")?.({ payload: null }));
}

test("캡처 초안이 있으면 종료를 묻고 quit_app을 부르지 않는다", async () => {
  useNativeRuntime.setState({ openTasks: 0, snapshotCurrent: true });
  captureDraft(true);
  await requestQuit();
  const dialog = await screen.findByRole("dialog", { name: "devbox 종료" });
  expect(dialog).toHaveTextContent("빠른 캡처에 저장하지 않은 초안이 있습니다. 종료하면 사라집니다.");
  // No agent task runs, so neither the task nor the WSL sentence is a reason.
  expect(dialog).not.toHaveTextContent("에이전트");
  expect(dialog).not.toHaveTextContent("WSL");
  expect(screen.getByRole("button", { name: "취소" })).toHaveFocus();
  expect(screen.queryByRole("button", { name: "종료" })).toBeNull();
  expect(called("quit_app")).toEqual([]);
  await userEvent.click(screen.getByRole("button", { name: "캡처 열기" }));
  expect(called("capture_open")).toEqual([["capture_open"]]);
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(called("quit_app")).toEqual([]);
});

test("running tasks and a draft are both named, and 버리고 종료 quits", async () => {
  useNativeRuntime.setState({ openTasks: 2, snapshotCurrent: true });
  captureDraft(true);
  await requestQuit();
  expect(await screen.findByRole("dialog", { name: "devbox 종료" })).toHaveTextContent(
    "에이전트 2개가 실행 중입니다. 앱을 끄면 WSL이 약 15초 뒤 멈추면서 함께 종료될 수 있습니다. 빠른 캡처에 저장하지 않은 초안이 있습니다. 종료하면 사라집니다.",
  );
  await userEvent.click(screen.getByRole("button", { name: "버리고 종료" }));
  expect(called("quit_app")).toEqual([["quit_app"]]);
  expect(called("capture_open")).toEqual([]);
});

test("a draft check that fails counts as no draft, so a current empty snapshot still quits directly", async () => {
  useNativeRuntime.setState({ openTasks: 0, snapshotCurrent: true });
  captureDraft(new Error("owned fixture failure"));
  await requestQuit();
  await waitFor(() => expect(called("quit_app")).toEqual([["quit_app"]]));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});
