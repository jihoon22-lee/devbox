import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { RpcError } from "../../../rpc/client";
import type { AuditJob, AuditProgress, DependencyInventory } from "../../../rpc/gen/rpc";
import { DependencyLens } from "./DependencyLens";

const rpc = vi.hoisted(() => ({
  queryError: null as RpcError | null,
  call: vi.fn(),
  audit: vi.fn(),
  refetch: vi.fn(),
  subscribe: vi.fn(),
  open: vi.fn(),
}));
vi.mock("../../../rpc/RpcProvider", () => ({ useRpc: () => rpc, useRpcSession: () => ({ state: "ready" }) }));
vi.mock("../../../rpc/query", () => ({
  useRpcQuery: () => ({ data: inventory, error: rpc.queryError, isLoading: false, refetch: rpc.refetch }),
  useRpcMutation: () => ({ mutateAsync: rpc.audit }),
}));
vi.mock("../../../shell/external", () => ({ openExternal: rpc.open }));
const inventory: DependencyInventory = {
  fingerprint: "lock-a",
  locks: ["pnpm"],
  dependencies: [
    {
      ecosystem: "npm",
      name: "@scope/pkg",
      direct: true,
      versions: [
        { version: "1.0.0", source: "registry", direct: true },
        { version: "2.0.0", source: "registry", direct: false },
      ],
    },
  ],
};
const report = {
  fingerprint: "lock-a",
  completedAtMs: 1000,
  skipped: 0,
  packages: [
    {
      ecosystem: "npm" as const,
      name: "@scope/pkg",
      version: "1.0.0",
      advisories: [{ id: "GHSA-owned", url: "https://osv.dev/vulnerability/GHSA-owned" }],
    },
    { ecosystem: "npm" as const, name: "@scope/pkg", version: "2.0.0", advisories: [] },
  ],
};
function job(state: AuditJob["state"] = "running"): AuditJob {
  return {
    id: "job",
    projectId: "p",
    fingerprint: "lock-a",
    state,
    completed: state === "running" ? 0 : 2,
    total: 2,
    cached: false,
    error: state === "failed" ? "network" : null,
    result: state === "succeeded" ? report : null,
    lastSuccess: state === "failed" ? report : null,
  };
}
let event: ((_rev: string, payload: AuditProgress) => void) | undefined;
beforeEach(() => {
  vi.clearAllMocks();
  rpc.queryError = null;
  rpc.subscribe.mockImplementation((_topic, cb) => {
    event = cb;
    return () => {
      event = undefined;
    };
  });
  rpc.audit.mockResolvedValue(job());
  rpc.call.mockResolvedValue(job());
});
afterEach(cleanup);
test("shows duplicate versions and direct relationships then safe advisory links", async () => {
  render(<DependencyLens projectId="p" />);
  expect(screen.getByText("1.0.0")).toBeInTheDocument();
  expect(screen.getByText("2.0.0")).toBeInTheDocument();
  expect(screen.getByText("직접 의존성")).toBeInTheDocument();
  expect(screen.getByText("간접 의존성")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "취약점 조회" }));
  await waitFor(() => expect(rpc.audit).toHaveBeenCalledWith({ projectId: "p" }));
  rpc.call.mockResolvedValue(job("succeeded"));
  act(() => event?.("2", { id: "job", projectId: "p", state: "succeeded", completed: 2, total: 2 }));
  const link = await screen.findByRole("link", { name: "GHSA-owned" });
  fireEvent.click(link);
  expect(rpc.open).toHaveBeenCalledWith("https://osv.dev/vulnerability/GHSA-owned");
  expect(screen.getByText(/마지막 성공/)).toBeInTheDocument();
});
test("failure retains timestamp and advisories with stale label and retry", async () => {
  rpc.audit.mockResolvedValue(job("failed"));
  render(<DependencyLens projectId="p" />);
  fireEvent.click(screen.getByRole("button", { name: "취약점 조회" }));
  await screen.findByText(/조회 실패/);
  expect(screen.getByRole("link", { name: "GHSA-owned" })).toBeInTheDocument();
  expect(screen.getByText(/이전 조회 결과/)).toBeInTheDocument();
  expect(screen.queryByText("취약점 없음")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "다시 시도" })).toBeInTheDocument();
});
test("late audit response from another project cannot populate current selection", async () => {
  let resolve!: (job: AuditJob) => void;
  rpc.audit.mockImplementation(
    () =>
      new Promise<AuditJob>((r) => {
        resolve = r;
      }),
  );
  const view = render(<DependencyLens projectId="p" />);
  fireEvent.click(screen.getByRole("button", { name: "취약점 조회" }));
  view.rerender(<DependencyLens projectId="other" />);
  await act(async () => resolve(job("succeeded")));
  expect(screen.queryByRole("link", { name: "GHSA-owned" })).not.toBeInTheDocument();
});

test("failed inventory refresh cannot show the previous result as currently clean", async () => {
  rpc.audit.mockResolvedValue({ ...job("succeeded"), result: { ...report, completedAtMs: Date.now() } });
  const view = render(<DependencyLens projectId="p" />);
  fireEvent.click(screen.getByRole("button", { name: "취약점 조회" }));
  await screen.findByText("취약점 없음");
  rpc.queryError = new RpcError("deps.malformed_lock", { lock: "pnpm" });
  view.rerender(<DependencyLens projectId="p" />);
  expect(screen.queryByText("취약점 없음")).not.toBeInTheDocument();
  expect(screen.getByText(/이전 조회 결과/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "취약점 조회" })).toBeDisabled();
});
test("progress events are scoped to the current project and job", async () => {
  render(<DependencyLens projectId="p" />);
  fireEvent.click(screen.getByRole("button", { name: "취약점 조회" }));
  await screen.findByText("조회 중 0 / 2");
  await act(async () => {});
  act(() => event?.("3", { id: "other", projectId: "other", state: "running", completed: 9, total: 10 }));
  expect(screen.getByText("조회 중 0 / 2")).toBeInTheDocument();
  act(() => event?.("4", { id: "job", projectId: "p", state: "running", completed: 1, total: 2 }));
  expect(screen.getByText("조회 중 1 / 2")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "취약점 조회" })).toBeDisabled();
});
