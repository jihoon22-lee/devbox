import { beforeEach, describe, expect, it, vi } from "vitest";
import { configureProductTransport, WorkspaceOperationError } from "../transport";
import { forgetReviewedControl, submitRuntimeControl } from "./runtimeControls";
configureProductTransport(async <T>() => undefined as T, "fixture-installation");
beforeEach(() => localStorage.clear());
describe("durable Runtime submissions", () => {
  it("reuses one operation after transport loss and clears only confirmed completion", async () => {
    const call=vi.fn().mockRejectedValueOnce(new Error("transport lost")).mockResolvedValueOnce({id:"run-1"});
    await expect(submitRuntimeControl(call,"run_job_now",{id:"job-1"})).rejects.toThrow();
    const first=call.mock.calls[0][1];
    await expect(submitRuntimeControl(call,"run_job_now",{id:"job-1"})).resolves.toEqual({id:"run-1"});
    expect(call.mock.calls[1][1].operationId).toBe(first.operationId);
    expect(localStorage.length).toBe(0);
  });
  it("retains interrupted requests and rejects changed arguments until explicit review", async () => {
    const call=vi.fn().mockRejectedValue(new WorkspaceOperationError("review", "runtime_control_recovery_required"));
    await expect(submitRuntimeControl(call,"run_workspace_task_operation",{id:"job-1",failFast:true})).rejects.toThrow();
    await expect(submitRuntimeControl(call,"run_workspace_task_operation",{id:"job-1",failFast:false})).rejects.toThrow("이전 실행 요청");
    expect(call).toHaveBeenCalledTimes(1);
    forgetReviewedControl(call.mock.calls[0][1].operationId);
    expect(localStorage.length).toBe(0);
  });
  it("does not invoke native control when request persistence fails", async () => {
    const call=vi.fn();
    const save=vi.spyOn(Storage.prototype,"setItem").mockImplementation(() => {throw new Error("quota");});
    try {await expect(submitRuntimeControl(call,"run_job_now",{id:"job-1"})).rejects.toThrow("quota");expect(call).not.toHaveBeenCalled();}
    finally {save.mockRestore();}
  });
});
