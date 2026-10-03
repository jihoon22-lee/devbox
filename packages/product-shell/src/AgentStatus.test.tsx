import { render, screen, act, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import AgentStatus from "./AgentStatus";
const state = vi.hoisted(() => ({ receive: (_: { payload: string }) => {}, unlisten: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_name, callback) => {
    state.receive = callback;
    return state.unlisten;
  }),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => "connected") }));
afterEach(cleanup);
it("reflects native connection changes without another describe and cleans up", async () => {
  const view = render(<AgentStatus initial="connected" native />);
  await act(async () => {});
  expect(screen.queryByText("백그라운드 서비스 연결 안 됨")).toBeNull();
  act(() => state.receive({ payload: "unavailable" }));
  expect(screen.getByRole("status").textContent).toContain("연결 안 됨");
  await assertNoA11yViolations(view.container);
  act(() => state.receive({ payload: "connected" }));
  expect(screen.queryByRole("status")).toBeNull();
  view.unmount();
  expect(state.unlisten).toHaveBeenCalledOnce();
});

it("does not overwrite a newer disconnect event with the initial status reply", async () => {
  const { invoke } = await import("@tauri-apps/api/core");
  let resolve!: (value: string) => void;
  vi.mocked(invoke).mockReturnValueOnce(
    new Promise<string>((done) => {
      resolve = done;
    }),
  );
  render(<AgentStatus initial="connected" native />);
  await act(async () => {});
  act(() => state.receive({ payload: "unavailable" }));
  await act(async () => resolve("connected"));
  expect(screen.getByRole("status").textContent).toContain("연결 안 됨");
});

it("announces a verified protocol restart until a fresh connection succeeds", async () => {
  const view = render(<AgentStatus initial="connected" native />);
  await act(async () => {});
  act(() => state.receive({ payload: "restarting" }));
  expect(screen.getByRole("status").textContent).toContain("다시 시작");
  await assertNoA11yViolations(view.container);
  act(() => state.receive({ payload: "connected" }));
  expect(screen.queryByRole("status")).toBeNull();
});

it("reconnects only the native agent and keeps a newer event over a late reconnect reply", async () => {
  const { invoke } = await import("@tauri-apps/api/core");
  let finish!: (value: string) => void;
  vi.mocked(invoke).mockImplementation((command) =>
    command === "plugin:product-shell|agent_reconnect"
      ? new Promise<string>((resolve) => {
          finish = resolve;
        })
      : Promise.resolve("connected"),
  );
  render(<AgentStatus initial="connected" native />);
  await act(async () => {});
  act(() => state.receive({ payload: "unavailable" }));
  fireEvent.click(screen.getByRole("button", { name: "백그라운드 서비스 다시 연결" }));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("plugin:product-shell|agent_reconnect"));
  act(() => state.receive({ payload: "connected" }));
  await act(async () => finish("unavailable"));
  expect(screen.queryByRole("status")).toBeNull();
  expect(
    vi
      .mocked(invoke)
      .mock.calls.every(([command]) =>
        ["plugin:product-shell|agent_status", "plugin:product-shell|agent_reconnect"].includes(command),
      ),
  ).toBe(true);
});

it("a manual reconnect supersedes an older initial status query", async () => {
  const { invoke } = await import("@tauri-apps/api/core");
  let initial!: (value: string) => void;
  vi.mocked(invoke).mockImplementation((command) =>
    command === "plugin:product-shell|agent_status"
      ? new Promise<string>((resolve) => {
          initial = resolve;
        })
      : Promise.resolve("connected"),
  );
  render(<AgentStatus initial="unavailable" native />);
  await act(async () => {});
  fireEvent.click(screen.getByRole("button", { name: "백그라운드 서비스 다시 연결" }));
  await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
  await act(async () => initial("unavailable"));
  expect(screen.queryByRole("status")).toBeNull();
});

it("pauses status calls and reconnect while setup is incomplete", async () => {
  const { invoke } = await import("@tauri-apps/api/core");
  vi.mocked(invoke).mockClear();
  render(<AgentStatus initial="unavailable" native paused />);
  await act(async () => {});
  expect(invoke).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: "백그라운드 서비스 다시 연결" })).toBeNull();
  expect(screen.getByRole("status").textContent).toContain("준비");
});
