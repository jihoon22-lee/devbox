import type { Registry } from "../RegistryGate";
import type { ProjectContext } from "@devbox/product-shell/api";
import type { AgentTask } from "@devbox/workspace-features/generated/AgentTask";

export interface FlowPorts {
  agents: {
    recordWorktree(id: string, revision: number, path: string): Promise<AgentTask>;
    bindWorktree(id: string, revision: number, worktreeId: string): Promise<AgentTask>;
  };
  source: {
    canRecoverWorktree(taskId: string): boolean;
    markWorktreeAttempt(taskId: string): void;
    clearWorktreeAttempt(taskId: string): void;
    inspectWorktree(branch: string, targetDir: string): Promise<"present" | "absent">;
    previewWorktree(branch: string, targetDir: string): Promise<{ previewId: string }>;
    createWorktree(previewId: string, operationId: string): Promise<{ path: string }>;
  };
  registry: {
    previewWsl(distroId: string, root: string): Promise<{ previewId: string; discovery: { kind: string } }>;
    cancel(previewId: string): Promise<unknown>;
    apply(previewId: string, name: string): Promise<{ context: ProjectContext }>;
    select(context: ProjectContext): Promise<unknown>;
  };
  terminal: { openAgentTerminal(operationId: string, taskId: string): Promise<unknown> };
  refreshContext(): Promise<void>;
  refreshRegistry?(): Promise<Registry>;
  currentContext(): Promise<ProjectContext | null>;
  operationId(key: string): string;
  settle(key: string): void;
}
export interface FlowEnv {
  distroId: string;
  projectName: string;
  worktreeContext(worktreeId: string): ProjectContext | null;
}
export class AgentFlowError extends Error {
  constructor(readonly code: string) {
    super(code);
    this.name = "AgentFlowError";
  }
}
export function sameContext(a: ProjectContext | null, b: ProjectContext): boolean {
  return (
    a?.projectId === b.projectId &&
    a.worktreeId === b.worktreeId &&
    a.revision === b.revision &&
    a.target.kind === "wsl" &&
    b.target.kind === "wsl" &&
    a.target.distroId === b.target.distroId
  );
}
export async function selectContext(ports: FlowPorts, context: ProjectContext): Promise<void> {
  await ports.registry.select(context);
  await ports.refreshRegistry?.();
  await ports.refreshContext();
  if (!sameContext(await ports.currentContext(), context)) throw new AgentFlowError("agent_context_changed");
}
export async function advance(task: AgentTask, ports: FlowPorts, env: FlowEnv): Promise<AgentTask> {
  let current = task;
  let registered: ProjectContext | null = null;
  if (current.state === "planned" || current.state === "created") {
    const base = env.worktreeContext(current.baseWorktreeId);
    if (
      !base ||
      base.projectId !== task.projectId ||
      base.target.kind !== "wsl" ||
      base.target.distroId !== env.distroId
    )
      throw new AgentFlowError("agent_task_context_mismatch");
    if (!sameContext(await ports.currentContext(), base)) await selectContext(ports, base);
  }
  if (current.state === "planned") {
    let path = current.targetDir;
    const presence = await ports.source.inspectWorktree(current.branch, current.targetDir);
    if (presence === "present" && !ports.source.canRecoverWorktree(current.id))
      throw new AgentFlowError("agent_worktree_unexpected");
    if (presence === "absent") {
      const preview = await ports.source.previewWorktree(current.branch, current.targetDir);
      ports.source.markWorktreeAttempt(current.id);
      path = (await ports.source.createWorktree(preview.previewId, current.id)).path;
    }
    current = await ports.agents.recordWorktree(current.id, current.revision, path);
  }
  if (current.state === "created") {
    ports.source.clearWorktreeAttempt(current.id);
    const preview = await ports.registry.previewWsl(env.distroId, current.targetDir);
    if (preview.discovery.kind !== "linkedWorktree" && preview.discovery.kind !== "known") {
      await ports.registry.cancel(preview.previewId);
      throw new AgentFlowError("agent_worktree_unexpected");
    }
    const { context } = await ports.registry.apply(preview.previewId, env.projectName);
    if (
      context.projectId !== task.projectId ||
      context.worktreeId === task.baseWorktreeId ||
      context.target.kind !== "wsl" ||
      context.target.distroId !== env.distroId
    )
      throw new AgentFlowError("agent_task_context_mismatch");
    current = await ports.agents.bindWorktree(current.id, current.revision, context.worktreeId);
    const snapshot = await ports.refreshRegistry?.();
    const tree = snapshot?.worktrees.find(
      (tree) => tree.id === context.worktreeId && tree.projectId === context.projectId,
    );
    if (snapshot && !tree) throw new AgentFlowError("agent_task_context_mismatch");
    registered = tree
      ? { projectId: tree.projectId, worktreeId: tree.id, revision: tree.revision, target: tree.binding.target }
      : context;
  }
  if (current.state === "ready" || current.state === "running") {
    const snapshot = registered ? null : await ports.refreshRegistry?.();
    const tree = snapshot?.worktrees.find(
      (tree) => tree.id === current.worktreeId && tree.projectId === current.projectId,
    );
    const target =
      registered ??
      (tree
        ? { projectId: tree.projectId, worktreeId: tree.id, revision: tree.revision, target: tree.binding.target }
        : snapshot
          ? null
          : current.worktreeId
            ? env.worktreeContext(current.worktreeId)
            : null);
    if (
      !target ||
      target.projectId !== task.projectId ||
      target.target.kind !== "wsl" ||
      target.target.distroId !== env.distroId
    )
      throw new AgentFlowError("agent_task_context_mismatch");
    await selectContext(ports, target);
    if (current.state === "ready") {
      const key = `workspace-agent-terminal:${current.id}`;
      await ports.terminal.openAgentTerminal(ports.operationId(key), current.id);
      ports.settle(key);
    }
  }
  return current;
}
