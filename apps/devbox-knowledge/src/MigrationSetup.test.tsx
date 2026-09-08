import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@devbox/knowledge-features/transport", () => ({ componentInvoke: () => invoke }));
vi.mock("./transport", () => ({ issueError: (issue: string) => Object.assign(new Error(issue), { name: issue }) }));
import MigrationSetup from "./MigrationSetup";
const plan = { id: "plan-1", phase: "prepared", preparedAtMs: 1, hasPrevious: true, vault: "C:/synthetic-vault", sources: [{ source: "notes", bytes: 100, report: { imported: 2, repeated: 1, conflicts: 1, retired: 0, reservedRootIds: 0 } }] };
beforeEach(() => {
  invoke.mockReset();
  invoke.mockImplementation(async (method: string, args?: { jobId: string }) => {
    if (method === "list_import_sources") return [{ source: "notes", available: true }, { source: "activity", available: false }];
    if (method === "list_imports") return [];
    if (method === "prepare_import") return { jobId: "prepare" };
    if (method === "activate_import") return { jobId: "activate" };
    if (method === "import_job") return { state: "succeeded", value: { plan, active: args?.jobId === "activate" } };
    return {};
  });
});
afterEach(cleanup);
describe("migration preparation and explicit activation", () => {
  it("shows source conflicts before a separate one-time apply action", async () => {
    const onActivated = vi.fn();
    render(<MigrationSetup onActivated={onActivated} onBack={() => {}}/>);
    const prepare = await screen.findByRole("button", { name: "가져오기 미리보기 준비" });
    await waitFor(() => expect((prepare as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(prepare);
    const apply = await screen.findByRole("button", { name: "미리보기를 확인하고 적용" });
    expect(screen.getByRole("table").textContent).toContain("충돌");
    expect(screen.getByText("C:/synthetic-vault")).toBeTruthy();
    expect(invoke).not.toHaveBeenCalledWith("activate_import", expect.anything());
    expect(onActivated).not.toHaveBeenCalled();
    fireEvent.click(apply); fireEvent.click(apply);
    await waitFor(() => expect(onActivated).toHaveBeenCalledTimes(1));
    expect(invoke.mock.calls.filter(([method]) => method === "activate_import")).toEqual([["activate_import", { planId: "plan-1" }]]);
  });
  it("cancels a prepare job returned after the screen unmounts without applying", async () => {
    let resolve: (value: { jobId: string }) => void = () => {};
    const defaultInvoke = invoke.getMockImplementation()!;
    invoke.mockImplementation((method: string, args?: unknown) => method === "prepare_import" ? new Promise(done => { resolve = done; }) : defaultInvoke(method, args));
    const onActivated = vi.fn();
    const view = render(<MigrationSetup onActivated={onActivated} onBack={() => {}}/>);
    const button = await screen.findByRole("button", { name: "가져오기 미리보기 준비" });
    await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(button); view.unmount();
    await act(async () => resolve({ jobId: "late-prepare" }));
    expect(invoke).toHaveBeenCalledWith("cancel_import_job", { jobId: "late-prepare" });
    expect(invoke).not.toHaveBeenCalledWith("activate_import", expect.anything());
    expect(onActivated).not.toHaveBeenCalled();
  });
  it("keeps failed preparations available for cleanup and never treats failure as activation", async () => {
    const defaultInvoke = invoke.getMockImplementation()!;
    invoke.mockImplementation((method: string, args?: unknown) => method === "import_job" ? Promise.resolve({ state: "failed", issue: "import_source_changed" }) : defaultInvoke(method, args));
    const onActivated = vi.fn();
    render(<MigrationSetup onActivated={onActivated} onBack={() => {}}/>);
    const button = await screen.findByRole("button", { name: "가져오기 미리보기 준비" });
    await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(button);
    expect((await screen.findByRole("alert")).textContent).toContain("import_source_changed");
    expect(onActivated).not.toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalledWith("activate_import", expect.anything());
    expect((button as HTMLButtonElement).disabled).toBe(false);
  });
});
