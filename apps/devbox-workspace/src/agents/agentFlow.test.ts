import { describe, expect, it, vi } from "vitest";
import { advance, AgentFlowError, type FlowPorts } from "./agentFlow";
import type { AgentTask } from "@devbox/workspace-features/generated/AgentTask";

const base = { projectId: "p1", worktreeId: "w1", revision: 1, target: { kind: "wsl" as const, distroId: "d1" } };
const agentContext = { ...base, worktreeId: "w2" };
function task(state: AgentTask["state"], extra: Partial<AgentTask> = {}): AgentTask {
  return {
    id: "t1",
    revision: 1,
    projectId: "p1",
    baseWorktreeId: "w1",
    title: "Fix login",
    tool: "claudeCode",
    command: "claude",
    branch: "agent/fix-login",
    targetDir: "/home/me/projects/devbox-fix-login",
    worktreeId: null,
    terminalId: null,
    state,
    createdAtMs: 1,
    updatedAtMs: 1,
    ...extra,
  };
}
function ports(overrides: Partial<Record<string, unknown>> = {}) {
  let selected: typeof base | null = base;
  const p = {
    agents: {
      recordWorktree: vi.fn(async (_id, revision, path) =>
        task("created", { revision: revision + 1, targetDir: path }),
      ),
      bindWorktree: vi.fn(async (_id, revision, worktreeId) => task("ready", { revision: revision + 1, worktreeId })),
    },
    source: {
      canRecoverWorktree: vi.fn(() => false),
      markWorktreeAttempt: vi.fn(),
      clearWorktreeAttempt: vi.fn(),
      inspectWorktree: vi.fn(async () => "absent" as "absent" | "present"),
      previewWorktree: vi.fn(async () => ({ previewId: "pv1" })),
      createWorktree: vi.fn(async () => ({ path: "/home/me/projects/devbox-fix-login" })),
    },
    registry: {
      previewWsl: vi.fn(async () => ({ previewId: "rg1", discovery: { kind: "linkedWorktree" } })),
      cancel: vi.fn(async () => {}),
      apply: vi.fn(async () => ({ context: agentContext })),
      select: vi.fn(async (context) => {
        selected = context;
      }),
    },
    terminal: { openAgentTerminal: vi.fn(async () => {}) },
    refreshContext: vi.fn(async () => {}),
    currentContext: vi.fn(async () => selected),
    operationId: vi.fn(() => "00000000-0000-4000-8000-000000000001"),
    settle: vi.fn(),
    ...overrides,
  };
  return p as unknown as FlowPorts & typeof p;
}
const env = {
  distroId: "d1",
  projectName: "devbox",
  worktreeContext: (id: string) => (id === "w2" ? agentContext : id === "w1" ? base : null),
};

describe("agent flow", () => {
  it("runs every step once from a planned task", async () => {
    const p = ports();
    await advance(task("planned"), p, env);
    expect(p.source.createWorktree).toHaveBeenCalledWith("pv1", "t1");
    expect(p.registry.apply).toHaveBeenCalledWith("rg1", "devbox");
    expect(p.registry.select).toHaveBeenCalledWith(agentContext);
    expect(p.terminal.openAgentTerminal).toHaveBeenCalledWith("00000000-0000-4000-8000-000000000001", "t1");
    expect(p.settle).toHaveBeenCalledTimes(1);
  });

  it("recovers a created worktree after its creation reply was lost", async () => {
    const p = ports();
    p.source.canRecoverWorktree.mockReturnValue(true);
    p.source.inspectWorktree.mockResolvedValue("present");
    await advance(task("planned"), p, env);
    expect(p.source.previewWorktree).not.toHaveBeenCalled();
    expect(p.source.createWorktree).not.toHaveBeenCalled();
    expect(p.agents.recordWorktree).toHaveBeenCalledWith("t1", 1, "/home/me/projects/devbox-fix-login");
  });

  it("does not adopt an existing matching worktree on the first creation attempt", async () => {
    const p = ports();
    p.source.inspectWorktree.mockResolvedValue("present");
    await expect(advance(task("planned"), p, env)).rejects.toEqual(new AgentFlowError("agent_worktree_unexpected"));
    expect(p.agents.recordWorktree).not.toHaveBeenCalled();
    expect(p.terminal.openAgentTerminal).not.toHaveBeenCalled();
  });

  it("resumes a created task at registration without creating another worktree", async () => {
    const p = ports();
    await advance(task("created"), p, env);
    expect(p.source.previewWorktree).not.toHaveBeenCalled();
    expect(p.registry.previewWsl).toHaveBeenCalledWith("d1", "/home/me/projects/devbox-fix-login");
  });

  it("accepts an already registered folder when a retry follows a lost reply", async () => {
    const p = ports();
    p.registry.previewWsl.mockResolvedValueOnce({ previewId: "rg2", discovery: { kind: "known" } });
    await advance(task("created"), p, env);
    expect(p.agents.bindWorktree).toHaveBeenCalledWith("t1", 1, "w2");
  });

  it("cancels an unexpected registration and stops", async () => {
    const p = ports();
    p.registry.previewWsl.mockResolvedValueOnce({ previewId: "rg3", discovery: { kind: "newProject" } });
    await expect(advance(task("created"), p, env)).rejects.toEqual(new AgentFlowError("agent_worktree_unexpected"));
    expect(p.registry.cancel).toHaveBeenCalledWith("rg3");
    expect(p.agents.bindWorktree).not.toHaveBeenCalled();
  });

  it("keeps the terminal operation id when opening fails so a retry is idempotent", async () => {
    const p = ports();
    p.terminal.openAgentTerminal.mockRejectedValueOnce(new Error("busy"));
    await expect(advance(task("ready", { worktreeId: "w2" }), p, env)).rejects.toThrow("busy");
    expect(p.settle).not.toHaveBeenCalled();
    await advance(task("ready", { worktreeId: "w2" }), p, env);
    expect(p.terminal.openAgentTerminal).toHaveBeenNthCalledWith(2, "00000000-0000-4000-8000-000000000001", "t1");
  });

  it("refuses to open a terminal when the selection did not switch", async () => {
    const p = ports({ currentContext: vi.fn(async () => base) });
    await expect(advance(task("ready", { worktreeId: "w2" }), p, env)).rejects.toEqual(
      new AgentFlowError("agent_context_changed"),
    );
  });
});

it("uses the newly registered context before the registry prop refreshes", async () => {
  const p = ports();
  await advance(task("created"), p, { ...env, worktreeContext: (id) => (id === "w1" ? base : null) });
  expect(p.terminal.openAgentTerminal).toHaveBeenCalledTimes(1);
});
it("rejects the same worktree id with a changed revision or project", async () => {
  for (const selected of [
    { ...agentContext, revision: 2 },
    { ...agentContext, projectId: "p2" },
  ]) {
    const p = ports({ currentContext: vi.fn(async () => selected) });
    await expect(advance(task("ready", { worktreeId: "w2" }), p, env)).rejects.toEqual(
      new AgentFlowError("agent_context_changed"),
    );
    expect(p.terminal.openAgentTerminal).not.toHaveBeenCalled();
  }
});
