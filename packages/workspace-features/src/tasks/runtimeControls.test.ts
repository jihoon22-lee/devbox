import { beforeEach, describe, expect, it, vi } from "vitest";
import { configureProductTransport, WorkspaceOperationError } from "../transport";
import { forgetReviewedControl, submitRuntimeControl, reconcileCompletedControls } from "./runtimeControls";
configureProductTransport(async <T>() => undefined as T, "fixture-installation");
beforeEach(() => localStorage.clear());
describe("durable Runtime submissions", () => {
  it("reuses one operation after transport loss and clears only confirmed completion", async () => {
    const call = vi.fn().mockRejectedValueOnce(new Error("transport lost")).mockResolvedValueOnce({ id: "run-1" });
    await expect(submitRuntimeControl(call, "run_job_now", { id: "job-1" })).rejects.toThrow();
    const first = call.mock.calls[0][1];
    await expect(submitRuntimeControl(call, "run_job_now", { id: "job-1" })).resolves.toEqual({ id: "run-1" });
    expect(call.mock.calls[1][1].operationId).toBe(first.operationId);
    expect(localStorage.length).toBe(0);
  });
  it("retains interrupted requests and rejects changed arguments until explicit review", async () => {
    const call = vi.fn().mockRejectedValue(new WorkspaceOperationError("review", "runtime_control_recovery_required"));
    await expect(
      submitRuntimeControl(call, "run_workspace_task_operation", { id: "job-1", failFast: true }),
    ).rejects.toThrow();
    await expect(
      submitRuntimeControl(call, "run_workspace_task_operation", { id: "job-1", failFast: false }),
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
      await expect(submitRuntimeControl(call, "run_job_now", { id: "job-1" })).rejects.toThrow("quota");
      expect(call).not.toHaveBeenCalled();
    } finally {
      save.mockRestore();
    }
  });
});

it("settles lost stop replies by read-only receipt lookup without stopping or launching again", async () => {
  const submit = vi.fn().mockRejectedValue(new Error("reply lost"));
  await expect(submitRuntimeControl(submit, "stop_active_run", { id: "job-1" })).rejects.toThrow();
  const operationId = submit.mock.calls[0][1].operationId;
  const status = vi.fn().mockResolvedValue(null); // Native status replays the stored stop result.
  await expect(reconcileCompletedControls(status)).resolves.toEqual([
    { operationId, method: "stop_active_run", targetId: "job-1", state: "completed" },
  ]);
  expect(status).toHaveBeenCalledExactlyOnceWith("runtime_control_status", { operationId });
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
    await expect(submitRuntimeControl(submit, "stop_active_run", { id: "job-1" })).rejects.toThrow();
    const reconciliation = reconcileCompletedControls(vi.fn().mockRejectedValue(error));
    if (error instanceof WorkspaceOperationError) await reconciliation;
    else await expect(reconciliation).rejects.toBe(error);
    expect(localStorage.length).toBe(1);
  }
});

it("retains a new operation replacing the old ID during reconciliation", async () => {
  const submit = vi.fn().mockRejectedValue(new Error("lost"));
  await expect(submitRuntimeControl(submit, "stop_active_run", { id: "job-1" })).rejects.toThrow();
  const key = localStorage.key(0)!;
  const status = vi.fn().mockImplementation(async () => {
    const entry = JSON.parse(localStorage.getItem(key)!);
    localStorage.setItem(key, JSON.stringify({ ...entry, operationId: "10000000-0000-4000-8000-000000000001" }));
    return null;
  });
  expect(await reconcileCompletedControls(status)).toEqual([]);
  expect(localStorage.length).toBe(1);
});
