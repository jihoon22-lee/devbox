import type { TransitionGuard } from "../contextTransition";
import { currentDescription } from "@devbox/product-shell/api";
import type { AgentsCall } from "@devbox/workspace-features/generated/AgentsCall";
import type { AgentsResults } from "@devbox/workspace-features/generated/agents-results";
import { bindTypedCall } from "@devbox/workspace-features/typed";
import { nativeCall } from "../native";
import type { Registry } from "../RegistryGate";
import { AgentFlowError } from "./agentFlow";
import type { FlowPorts } from "./agentFlow";
export const agentsCall = bindTypedCall<AgentsCall, AgentsResults>((method, args) =>
  nativeCall("workspace.agents", method, args, "agents"),
);
export function call<T>(
  component: "source" | "registry" | "terminal",
  method: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  return nativeCall(`workspace.${component}`, method, args, "agents");
}
export function nativePorts(
  refreshContext: () => Promise<void>,
  installationId: string,
  transition?: TransitionGuard,
  refreshRegistry?: () => Promise<Registry>,
): FlowPorts {
  const creationKey = (id: string) => `${installationId}:workspace-agent-worktree:${id}`;
  return {
    agents: {
      recordWorktree: (taskId, revision, path) => agentsCall("record_worktree", { taskId, revision, path }),
      bindWorktree: (taskId, revision, worktreeId) => agentsCall("bind_worktree", { taskId, revision, worktreeId }),
    },
    source: {
      canRecoverWorktree: (id) => localStorage.getItem(creationKey(id)) === "attempted",
      markWorktreeAttempt: (id) => localStorage.setItem(creationKey(id), "attempted"),
      clearWorktreeAttempt: (id) => localStorage.removeItem(creationKey(id)),
      inspectWorktree: async (branch, targetDir) => {
        const context = (await currentDescription("workspace")).context;
        const registry = await call<Registry>("registry", "snapshot");
        const tree = registry.worktrees.find(
          (tree) =>
            tree.id === context?.worktreeId &&
            tree.projectId === context.projectId &&
            tree.revision === context.revision,
        );
        if (!tree) throw new AgentFlowError("agent_task_context_mismatch");
        return call("source", "inspect_agent_worktree", {
          request: { path: tree.binding.root, worktree: targetDir, branch },
        });
      },
      previewWorktree: (branch, targetDir) => call("source", "preview_worktree", { branch, targetDir }),
      createWorktree: (previewId, operationId) => call("source", "create_worktree", { previewId, operationId }),
    },
    registry: {
      previewWsl: (distroId, root) => call("registry", "preview_wsl", { distroId, root, startStopped: false }),
      cancel: (previewId) => call("registry", "cancel_registration", { previewId }),
      apply: (previewId, name) => {
        const apply = () =>
          call<{ context: import("@devbox/product-shell/api").ProjectContext }>("registry", "apply_registration", {
            previewId,
            name,
            action: "register",
          });
        return transition ? transition(apply) : apply();
      },
      select: (context) => {
        const select = () => call("registry", "select_project", { context });
        return transition ? transition(select) : select();
      },
    },
    terminal: {
      openAgentTerminal: (operationId, taskId) => call("terminal", "open_agent_terminal", { operationId, taskId }),
    },
    refreshContext,
    refreshRegistry,
    currentContext: async () => (await currentDescription("workspace")).context ?? null,
    operationId: (key) => {
      const storageKey = `${installationId}:${key}`;
      const stored = sessionStorage.getItem(storageKey);
      if (stored && /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(stored)) return stored;
      const id = crypto.randomUUID();
      sessionStorage.setItem(storageKey, id);
      return id;
    },
    settle: (key) => sessionStorage.removeItem(`${installationId}:${key}`),
  };
}
