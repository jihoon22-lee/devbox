import { render, screen, act, cleanup } from "@testing-library/react";
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
