import { beforeEach, describe, expect, it, vi } from "vitest";
import { configureProductTransport, WorkspaceOperationError } from "../transport";
import { forgetReviewedControl, submitRuntimeControl, reconcileCompletedControls } from "./runtimeControls";
configureProductTransport(async <T>() => undefined as T, "fixture-installation");
beforeEach(() => localStorage.clear());
type MockCall = (method: string, args?: Record<string, unknown>) => Promise<unknown>;
const asCall =
  (call: MockCall) =>
  <T>(method: string, args?: Record<string, unknown>): Promise<T> =>
    call(method, args) as Promise<T>;
const withEmptyInventory =
  (call: MockCall) =>
  <T>(method: string, args?: Record<string, unknown>): Promise<T> =>
    method === "list_runtime_controls" ? Promise.resolve([] as T) : asCall(call)<T>(method, args);

describe("durable Runtime submissions", () => {
  it("reuses one operation after transport loss and clears only confirmed completion", async () => {
    const call = vi.fn().mockRejectedValueOnce(new Error("transport lost")).mockResolvedValueOnce({ id: "run-1" });
    await expect(submitRuntimeControl(withEmptyInventory(call), "run_job_now", { id: "job-1" })).rejects.toThrow();
    const first = call.mock.calls[0][1];
    await expect(submitRuntimeControl(withEmptyInventory(call), "run_job_now", { id: "job-1" })).resolves.toEqual({
      id: "run-1",
    });
    expect(call.mock.calls[1][1].operationId).toBe(first.operationId);
    expect(localStorage.length).toBe(0);
  });
  it("retains interrupted requests and rejects changed arguments until explicit review", async () => {
    const call = vi.fn().mockRejectedValue(new WorkspaceOperationError("review", "runtime_control_recovery_required"));
    await expect(
      submitRuntimeControl(withEmptyInventory(call), "run_workspace_task_operation", { id: "job-1", failFast: true }),
    ).rejects.toThrow();
    await expect(
      submitRuntimeControl(withEmptyInventory(call), "run_workspace_task_operation", { id: "job-1", failFast: false }),
    ).rejects.toThrow("이전 실행 요청");
    expect(call).toHaveBeenCalledTimes(1);
    forgetReviewedControl(call.mock.calls[0][1].operationId);
    expect(localStorage.length).toBe(0);
  });
  it("does not invoke native control when request persistence fails", async () => {
    const call = vi.fn();
    const save = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota");
    });
    try {
      await expect(submitRuntimeControl(withEmptyInventory(call), "run_job_now", { id: "job-1" })).rejects.toThrow(
        "quota",
      );
      expect(call).not.toHaveBeenCalled();
    } finally {
      save.mockRestore();
    }
  });
});

it("settles lost stop replies by read-only receipt lookup without stopping or launching again", async () => {
  const submit = vi.fn().mockRejectedValue(new Error("reply lost"));
  await expect(submitRuntimeControl(withEmptyInventory(submit), "stop_active_run", { id: "job-1" })).rejects.toThrow();
  const operationId = submit.mock.calls[0][1].operationId;
  const status = vi.fn().mockImplementation(async (method) => (method === "list_runtime_controls" ? [] : null)); // Native status replays the stored stop result.
  await expect(reconcileCompletedControls(status)).resolves.toEqual([
    { operationId, method: "stop_active_run", targetId: "job-1", state: "completed" },
  ]);
  expect(status.mock.calls).toEqual([
    ["list_runtime_controls", {}],
    ["runtime_control_status", { operationId }],
    ["review_runtime_control", { operationId }],
  ]);
  expect(submit).toHaveBeenCalledTimes(1);
  expect(localStorage.length).toBe(0);
});

it("retains unknown, pending and interrupted native status failures", async () => {
  for (const error of [
    new Error("reply lost"),
    new WorkspaceOperationError("pending", "runtime_control_in_progress"),
    new WorkspaceOperationError("interrupted", "runtime_control_recovery_required"),
  ]) {
    localStorage.clear();
    const submit = vi.fn().mockRejectedValue(new Error("reply lost"));
    await expect(
      submitRuntimeControl(withEmptyInventory(submit), "stop_active_run", { id: "job-1" }),
    ).rejects.toThrow();
    const reconciliation = reconcileCompletedControls(
      vi.fn().mockImplementation(async (method) => {
        if (method === "list_runtime_controls") return [];
        throw error;
      }),
    );
    if (error instanceof WorkspaceOperationError) await reconciliation;
    else await expect(reconciliation).rejects.toBe(error);
    expect(localStorage.length).toBe(1);
  }
});

it("retains a new operation replacing the old ID during reconciliation", async () => {
  const submit = vi.fn().mockRejectedValue(new Error("lost"));
  await expect(submitRuntimeControl(withEmptyInventory(submit), "stop_active_run", { id: "job-1" })).rejects.toThrow();
  const key = localStorage.key(0)!;
  const status = vi.fn().mockImplementation(async (method) => {
    if (method === "list_runtime_controls") return [];
    const entry = JSON.parse(localStorage.getItem(key)!);
    localStorage.setItem(key, JSON.stringify({ ...entry, operationId: "10000000-0000-4000-8000-000000000001" }));
    return null;
  });
  expect(await reconcileCompletedControls(status)).toEqual([]);
  expect(localStorage.length).toBe(1);
});

it("recovers the native completed receipt when crash lost all browser storage", async () => {
  const operationId = "10000000-0000-4000-8000-000000000001";
  const call = vi.fn(async (method: string) => {
    if (method === "list_runtime_controls")
      return [{ operationId, method: "run_job_now", targetId: "job-1", state: "completed", reviewed: false }];
    return null;
  });
  expect(await reconcileCompletedControls(asCall(call))).toEqual([
    { operationId, method: "run_job_now", targetId: "job-1", state: "completed" },
  ]);
  expect(call.mock.calls.map(([method]) => method)).toEqual([
    "list_runtime_controls",
    "runtime_control_status",
    "review_runtime_control",
  ]);
  expect(localStorage.length).toBe(0);
});

it("acknowledges the successful native reply and preserves recovery if acknowledgement fails", async () => {
  const call = vi.fn(async (method: string) => {
    if (method === "review_runtime_control") throw new Error("ack lost");
    return { id: "run-1" };
  });
  await expect(submitRuntimeControl(withEmptyInventory(call), "run_job_now", { id: "job-1" })).resolves.toEqual({
    id: "run-1",
  });
  expect(call.mock.calls.map(([method]) => method)).toEqual(["runtime_control", "review_runtime_control"]);
  expect(localStorage.length).toBe(1);
});

it("blocks a fresh operation while the same target has a native unacknowledged receipt", async () => {
  const call = vi.fn().mockResolvedValue([
    {
      operationId: "10000000-0000-4000-8000-000000000001",
      method: "run_job_now",
      targetId: "job-1",
      state: "completed",
      reviewed: false,
    },
  ]);
  await expect(submitRuntimeControl(asCall(call), "run_job_now", { id: "job-1" })).rejects.toThrow("이전 실행 요청");
  expect(call).toHaveBeenCalledExactlyOnceWith("list_runtime_controls", {});
  expect(localStorage.length).toBe(0);
});

it("shares the same operation when two submissions overlap the inventory read", async () => {
  const call = vi.fn(async (method: string, _args?: Record<string, unknown>) => {
    if (method === "list_runtime_controls") return [];
    if (method === "runtime_control") throw new Error("reply lost");
    return null;
  });
  await Promise.allSettled([
    submitRuntimeControl(asCall(call), "run_job_now", { id: "job-1" }),
    submitRuntimeControl(asCall(call), "run_job_now", { id: "job-1" }),
  ]);
  const controls = call.mock.calls.filter(([method]) => method === "runtime_control");
  expect(controls).toHaveLength(2);
  expect(controls[0][1]).toEqual(controls[1][1]);
});

it("publishes each acknowledged completion before a later receipt read can fail", async () => {
  const ids = ["10000000-0000-4000-8000-000000000001", "10000000-0000-4000-8000-000000000002"];
  const call = vi.fn(async (method: string, args?: Record<string, unknown>) => {
    if (method === "list_runtime_controls")
      return ids.map((operationId) => ({
        operationId,
        method: "run_job_now",
        targetId: "job",
        state: "completed",
        reviewed: false,
      }));
    if (method === "runtime_control_status" && args?.operationId === ids[1]) throw new Error("later receipt offline");
    return null;
  });
  const observed = vi.fn();
  await expect(reconcileCompletedControls(asCall(call), observed)).rejects.toThrow("later receipt offline");
  expect(observed).toHaveBeenCalledExactlyOnceWith({
    operationId: ids[0],
    method: "run_job_now",
    targetId: "job",
    state: "completed",
  });
});
