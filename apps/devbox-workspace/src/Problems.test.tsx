import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { fixtureDescription } from "@devbox/product-shell/api";
import Problems from "./Problems";
import { componentCall } from "./native";

vi.mock("./native", () => ({ componentCall: vi.fn() }));
const call = vi.mocked(componentCall);
const context = { projectId: "project", worktreeId: "first", target: { kind: "windows" as const }, revision: 1 };
const description = { ...fixtureDescription("workspace"), context };
const problem = {
  id: "problem", source: "lsp", revision: "revision", severity: "error",
  message: "Synthetic diagnostic", stale: false, log: null,
  target: { kind: "file", relativePath: "src/a.ts", line: 3, column: null, documentVersion: 2 },
};
const snapshot = { context, initialized: true, runningTasks: 0, problems: [problem], sources: [] };
beforeEach(() => call.mockReset());
afterEach(cleanup);

it("revalidates a diagnostic before navigating and preserves an unknown column", async () => {
  call.mockImplementation(async (_description, _component, method) =>
    method === "snapshot" ? snapshot : { context, target: problem.target });
  const onFile = vi.fn();
  render(<Problems description={description} onFile={onFile} onLog={vi.fn()} navigate={vi.fn()}/>);
  await screen.findByText(/열 위치 미확인/);
  fireEvent.click(screen.getByRole("button", { name: "위치 열기" }));
  await waitFor(() => expect(onFile).toHaveBeenCalledWith(expect.objectContaining({relativePath: "src/a.ts", line: 3, column: null})));
  expect(call.mock.calls.find(([, , method]) => method === "resolve")?.[3]).toEqual({ id: "problem", revision: "revision", log: false });
});

it("retains unavailable evidence but prevents stale navigation", async () => {
  call.mockResolvedValue({...snapshot, problems: [{...problem, stale: true}], sources: [{source: "lsp", state: "unavailable", truncated: false}]});
  render(<Problems description={description} onFile={vi.fn()} onLog={vi.fn()} navigate={vi.fn()}/>);
  await screen.findByText("Synthetic diagnostic");
  expect((screen.getByRole("button", {name: "위치 열기"}) as HTMLButtonElement).disabled).toBe(true);
  expect(call.mock.calls.every(([, , method]) => method === "snapshot")).toBe(true);
});

it("discards a late snapshot and late navigation after switching worktrees", async () => {
  let finish: ((value: unknown) => void) | undefined;
  call.mockResolvedValue(snapshot);
  const onFile = vi.fn();
  const props = {onFile, onLog: vi.fn(), navigate: vi.fn()};
  const view = render(<Problems description={description} {...props}/>);
  await screen.findByText("Synthetic diagnostic");
  call.mockImplementation(async (_description, _component, method) => method === "resolve"
    ? new Promise(resolve => { finish = resolve; }) : snapshot);
  fireEvent.click(screen.getByRole("button", {name: "위치 열기"}));
  await waitFor(() => expect(finish).toBeDefined());
  view.rerender(<Problems description={{...description, context: {...context, worktreeId: "second"}}} {...props}/>);
  await act(async () => { finish?.({context, target: problem.target}); });
  expect(screen.queryByText("Synthetic diagnostic")).toBeNull();
  expect(onFile).not.toHaveBeenCalled();
});
