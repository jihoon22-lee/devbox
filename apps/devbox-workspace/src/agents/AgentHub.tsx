import { useRef, useState } from "react";
import { useOperation, usePolling } from "@devbox/hooks";
import type { Description, ProjectContext } from "@devbox/product-shell/api";
import type { AgentResources } from "@devbox/workspace-features/generated/AgentResources";
import type { UsageReport } from "@devbox/workspace-features/generated/UsageReport";
import type { AgentTask } from "@devbox/workspace-features/generated/AgentTask";
import type { TerminalRecord } from "@devbox/workspace-features/generated/TerminalRecord";
import type { RepoSnapshot } from "@devbox/workspace-features/generated/RepoSnapshot";
import type { MergeResult } from "@devbox/workspace-features/generated/MergeResult";
import type { Registry, Worktree } from "../RegistryGate";
import { agentsCall, call, nativePorts } from "./api";
import { advance, AgentFlowError, selectContext } from "./agentFlow";
import { errorCode, errorMessage } from "./messages";
import NewAgentTask from "./NewAgentTask";
import AgentTaskRow, { type Confirmation, type RowAction } from "./AgentTaskRow";
function contextOf(tree: Worktree): ProjectContext {
  return { projectId: tree.projectId, worktreeId: tree.id, revision: tree.revision, target: tree.binding.target };
}
export default function AgentHub({
  description,
  active = true,
  registry,
  navigate,
  refreshContext,
}: {
  description: Description;
  active?: boolean;
  registry: Registry | null;
  navigate(route: string): void;
  refreshContext(): Promise<void>;
}) {
  const [resources, setResources] = useState<AgentResources[]>([]);
  const [usage, setUsage] = useState<Record<string, UsageReport>>({});
  const [tasks, setTasks] = useState<AgentTask[]>([]);
  const [terminals, setTerminals] = useState<TerminalRecord[]>([]);
  const [confirmation, setConfirmation] = useState<Confirmation | null>(null);
  const [issue, setIssue] = useState<string | null>(null);
  const [reviewRequired, setReviewRequired] = useState(false);
  const operation = useOperation();
  const acting = useRef(false);
  const projectId = description.context?.projectId;
  const selectedProject = useRef(projectId);
  selectedProject.current = projectId;
  const ports = nativePorts(refreshContext, description.handshake.installationId);
  const wsl = description.context?.target.kind === "wsl";
  async function load() {
    const project = projectId;
    const [next, sessions] = await Promise.all([
      agentsCall("list", {}),
      call<TerminalRecord[]>("terminal", "terminal_sessions"),
    ]);
    if (selectedProject.current === project) {
      setTasks(next.filter((task) => task.projectId === project));
      setTerminals(sessions);
    }
  }
  usePolling(
    async () => {
      if (acting.current) return;
      try {
        await load();
      } catch (error) {
        setIssue(errorMessage(error));
      }
    },
    { intervalMs: 3000, active: active && wsl },
  );
  const hasRunning = tasks.some((task) => task.projectId === projectId && task.state === "running");
  usePolling(
    async () => {
      if (acting.current) return;
      const project = projectId;
      try {
        const samples = await agentsCall("resources", {});
        if (selectedProject.current === project) setResources(samples);
      } catch (error) {
        setIssue((prior) => prior ?? errorMessage(error));
      }
    },
    { intervalMs: 10_000, active: active && wsl && hasRunning },
  );
  const worktreeContext = (id: string) => {
    const tree = registry?.worktrees.find((tree) => tree.id === id && tree.projectId === projectId);
    return tree ? contextOf(tree) : null;
  };
  async function run(action: () => Promise<void>) {
    if (acting.current) return;
    acting.current = true;
    setIssue(null);
    setReviewRequired(false);
    await operation.run(async () => {
      try {
        await action();
      } catch (error) {
        setIssue(errorMessage(error));
        setReviewRequired(errorCode(error) === "source_review_required");
      } finally {
        try {
          await load();
        } catch (error) {
          setIssue((prior) => prior ?? errorMessage(error));
        }
        acting.current = false;
      }
    });
  }
  async function resume(task: AgentTask) {
    const target = description.context?.target;
    if (target?.kind !== "wsl") throw new AgentFlowError("agent_wsl_required");
    await advance(task, ports, {
      distroId: target.distroId,
      projectName: registry?.projects.find((project) => project.id === task.projectId)?.name ?? "",
      worktreeContext,
    });
  }
  async function baseOf(task: AgentTask) {
    const snapshot = await call<Registry>("registry", "snapshot");
    const tree = snapshot.worktrees.find(
      (tree) => tree.id === task.baseWorktreeId && tree.projectId === task.projectId,
    );
    if (!tree) throw new AgentFlowError("agent_task_context_mismatch");
    await selectContext(ports, contextOf(tree));
    return tree;
  }
  async function cleanupTask(task: AgentTask, force: boolean) {
    if (task.terminalId) await call("terminal", "stop_terminal", { id: task.terminalId });
    const base = await baseOf(task);
    const presence = await ports.source.inspectWorktree(task.branch, task.targetDir);
    if (presence === "present")
      await call("source", "remove_agent_worktree", {
        request: {
          path: base.binding.root,
          worktree: task.targetDir,
          branch: task.branch,
          force,
          operationId: crypto.randomUUID(),
        },
      });
    const snapshot = await call<Registry>("registry", "snapshot");
    const tree = snapshot.worktrees.find((tree) => tree.id === task.worktreeId && tree.projectId === task.projectId);
    if (tree) await call("registry", "remove", { revision: snapshot.revision, context: contextOf(tree) });
    await agentsCall("finish", { taskId: task.id, revision: task.revision, outcome: force ? "discarded" : "merged" });
    setConfirmation(null);
    await refreshContext();
  }
  async function act(task: AgentTask, action: RowAction) {
    if (action === "usage") {
      const report = await agentsCall("usage", { taskId: task.id });
      if (selectedProject.current === task.projectId) setUsage((prior) => ({ ...prior, [task.id]: report }));
    }
    if (action === "resume") await resume(task);
    if (action === "focus") await call("terminal", "focus_terminal", { id: task.terminalId });
    if (action === "reopen" || action === "review" || action === "pr") {
      const context = task.worktreeId ? worktreeContext(task.worktreeId) : null;
      if (!context) throw new AgentFlowError("agent_task_context_mismatch");
      await selectContext(ports, context);
      if (action === "review" || action === "pr") navigate("source");
      else {
        const key = `workspace-agent-terminal:${task.id}`;
        await ports.terminal.openAgentTerminal(ports.operationId(key), task.id);
        ports.settle(key);
      }
    }
    if (action === "resolve") {
      const base = await baseOf(task);
      const result = await call<MergeResult>("source", "repo_merge", {
        request: {
          path: base.binding.root,
          branch: task.branch,
          operationId: crypto.randomUUID(),
          keepConflicts: true,
        },
      });
      setConfirmation(result.merged ? { taskId: task.id, kind: "cleanup" } : null);
      navigate("source");
    }
    if (action === "forget") {
      await agentsCall("forget", { taskId: task.id, revision: task.revision });
      ports.source.clearWorktreeAttempt(task.id);
    }
    if (action === "discard") setConfirmation({ taskId: task.id, kind: "discard" });
    if (action === "merge") {
      const base = await baseOf(task);
      const status = await call<RepoSnapshot>("source", "repo_status", { path: base.binding.root });
      if (status.branch.detached) throw new AgentFlowError("source_merge_failed");
      setConfirmation({ taskId: task.id, kind: "merge", branch: status.branch.current });
    }
  }
  async function confirm(task: AgentTask) {
    if (confirmation?.taskId !== task.id) return;
    if (confirmation.kind === "cleanup" || confirmation.kind === "discard")
      return cleanupTask(task, confirmation.kind === "discard");
    if (confirmation.kind === "merge") {
      const base = await baseOf(task);
      const status = await call<RepoSnapshot>("source", "repo_status", { path: base.binding.root });
      if (status.branch.detached || status.branch.current !== confirmation.branch)
        throw new AgentFlowError("agent_context_changed");
      const result = await call<MergeResult>("source", "repo_merge", {
        request: {
          path: base.binding.root,
          branch: task.branch,
          operationId: crypto.randomUUID(),
          keepConflicts: false,
        },
      });
      setConfirmation(
        result.merged
          ? { taskId: task.id, kind: "cleanup" }
          : { taskId: task.id, kind: "conflicts", paths: result.conflicts },
      );
    }
  }
  if (!wsl) return <p>에이전트 작업은 WSL 프로젝트에서 사용할 수 있습니다.</p>;
  return (
    <section aria-label="에이전트 작업">
      <h2>에이전트</h2>
      <NewAgentTask
        busy={operation.busy}
        submit={(input) => {
          void run(async () => {
            const task = await agentsCall("plan", input);
            await resume(task);
          });
        }}
      />
      {issue && <p role="alert">{issue}</p>}
      {reviewRequired && (
        <button disabled={operation.busy} onClick={() => navigate("source")}>
          소스 화면 열기
        </button>
      )}
      <ul>
        {tasks
          .filter((task) => task.projectId === projectId)
          .map((task) => (
            <AgentTaskRow
              key={task.id}
              task={task}
              resources={resources.find((sample) => sample.taskId === task.id)}
              usage={usage[task.id]}
              stopped={!terminals.some((terminal) => terminal.id === task.terminalId && terminal.state === "active")}
              busy={operation.busy}
              confirmation={confirmation}
              act={(action) => {
                void run(() => act(task, action));
              }}
              confirm={() => {
                void run(() => confirm(task));
              }}
              cancel={() => setConfirmation(null)}
            />
          ))}
      </ul>
    </section>
  );
}
