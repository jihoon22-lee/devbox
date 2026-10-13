// 이 파일은 `pnpm gen`이 만든다. 직접 고치지 않는다.
/* eslint-disable */

export type AddProject = { path: string, };
export type AgentEventParams = { agentId: string, source: HookSource, payload: string, code: number | null, };
export type AgentProfiles = { items: Array<ToolProfile>, };
export type AgentState = "preparing" | "running" | "waiting" | "idle" | "failed" | "review" | "closed";
export type AgentTask = { id: string, projectId: string, title: string, profile: string, baseBranch: string, branch: string | null, worktreePath: string | null, workdir: string, sessionId: string | null, terminalId: string | null, state: AgentState, reason: string | null, attention: Attention | null, port: number, prompt: string, followUps: Array<FollowUp>, tokens: Tokens | null, prUrl: string | null, closedHow: CloseHow | null, createdMs: number, updatedMs: number, closedMs: number | null, };
export type AgentsAttention = { id: string, state: AgentState, message: string, };
export type AgentsChanged = { rev: string, id: string, };
export type AgentsError = { "code": "agents.title_invalid" } | { "code": "agents.slug_exhausted" } | { "code": "agents.not_found", "detail": { id: string, } } | { "code": "agents.project_not_found", "detail": { id: string, } } | { "code": "agents.profile_not_found", "detail": { id: string, } } | { "code": "agents.bad_state", "detail": { state: string, } } | { "code": "agents.worktree_dirty", "detail": { files: Array<string>, } } | { "code": "agents.base_dirty", "detail": { files: Array<string>, } } | { "code": "agents.base_not_checked_out", "detail": { branch: string, } } | { "code": "agents.conflict", "detail": { files: Array<string>, } } | { "code": "agents.unmerged_commits", "detail": { count: number, } } | { "code": "agents.gh_missing" } | { "code": "agents.not_repo" } | { "code": "agents.state_changed" } | { "code": "agents.operation_in_progress" } | { "code": "agents.unsafe_worktree" } | { "code": "agents.branch_exists", "detail": { branch: string, } } | { "code": "agents.worktree_add_partial", "detail": { branch: string, path: string, path_exists: boolean, branch_exists: boolean | null, worktree_registered: boolean | null, } } | { "code": "agents.invalid_input" } | { "code": "agents.session_missing" } | { "code": "agents.session_metadata", "detail": { diagnostic: Diagnostic, } };
export type AgentsList = { rev: string, items: Array<AgentTask>, };
export type AttachSession = { id: string, cols: number, rows: number, readOnly: boolean, };
export type Attention = { kind: string, message: string, atMs: number, };
export type Candidate = { path: string, name: string, isGit: boolean, registered: boolean, };
export type Check = { id: string, status: Status, message: string, fix: string | null, };
export type CloseHow = "merged" | "pullRequest" | "discarded";
export type CloseSession = { id: string, };
export type CommonError = { "code": "internal" } | { "code": "invalid_params", "detail": { reason: string, } } | { "code": "method_not_found", "detail": { method: string, } } | { "code": "hello_required" } | { "code": "cancelled" } | { "code": "daemon_restarting" } | { "code": "not_found" } | { "code": "secrets_locked" };
export type CreateAgentTask = { projectId: string, title: string, profile: string, baseBranch: string | null, worktree: boolean, prompt: string, };
export type CreateSession = { projectId: string | null, cwd: string | null, title: string | null, command: string | null, attachZellij: string | null, };
export type DaemonInfo = { version: string, instance: string, pid: number, startedMs: number, dataDir: string, };
export type DebugError = { "code": "debug.unsupported" };
export type Diagnostic = "missing" | "ambiguous" | "limitExceeded" | "invalid" | "unsafePath" | "unavailable";
export type DoctorReport = { checks: Array<Check>, };
export type Done = { rev: string, };
export type EchoParams = { bytes: number, };
export type FollowUp = { atMs: number, text: string, };
export type GetAgent = { id: string, };
export type HookSource = "claude" | "codex" | "setup" | "exit";
export type Hooks = "claude" | "codex" | "none";
export type ListAgents = { includeClosedHours: number | null, };
export type ListProjects = Record<symbol, never>;
export type ListSessions = { projectId: string | null, };
export type PingParams = Record<symbol, never>;
export type Pong = { daemonId: string, version: string, uptimeMs: number, };
export type Project = { id: string, name: string, path: string, favorite: boolean, addedMs: number, };
export type ProjectList = { rev: string, items: Array<Project>, };
export type ProjectsChanged = { rev: string, };
export type ProjectsError = { "code": "projects.path_invalid", "detail": { path: string, } } | { "code": "projects.not_directory", "detail": { path: string, } } | { "code": "projects.already_exists", "detail": { path: string, } } | { "code": "projects.not_found", "detail": { id: string, } };
export type RemoveProject = { id: string, };
export type RenameSession = { id: string, title: string, };
export type ResizeStream = { streamId: number, cols: number, rows: number, };
export type ScanProjects = Record<symbol, never>;
export type ScanResult = { candidates: Array<Candidate>, };
export type SessionKind = "shell" | "agent";
export type SessionList = { rev: string, items: Array<TerminalSession>, };
export type Status = "ok" | "warn" | "fail";
export type StreamOpened = { streamId: number, };
export type SystemError = { "code": "system.unavailable" };
export type TerminalChanged = { rev: string, };
export type TerminalError = { "code": "terminal.invalid_size", "detail": { cols: number, rows: number, } } | { "code": "terminal.stream_unavailable", "detail": Record<symbol, never> } | { "code": "terminal.not_found", "detail": { id: string, } } | { "code": "terminal.project_not_found", "detail": { id: string, } } | { "code": "terminal.cwd_invalid", "detail": { path: string, } };
export type TerminalProfile = { name: string, command: string, };
export type TerminalProfiles = { items: Array<TerminalProfile>, };
export type TerminalSession = { id: string, projectId: string | null, title: string, cwd: string, kind: SessionKind, alive: boolean, command: string, createdMs: number, };
export type Tokens = { input: number, output: number, cacheRead: number, cacheWrite: number, };
export type ToolProfile = { id: string, label: string, command: string, hooks: Hooks, waitingPatterns: Array<string>, };
export type ZellijSessions = { items: Array<string>, };

export interface Methods {
  "agents.create": { kind: "mutation"; confirm: "none"; params: CreateAgentTask; result: AgentTask; error: AgentsError };
  "agents.event": { kind: "mutation"; confirm: "none"; params: AgentEventParams; result: Done; error: AgentsError };
  "agents.get": { kind: "query"; confirm: "none"; params: GetAgent; result: AgentTask; error: AgentsError };
  "agents.list": { kind: "query"; confirm: "none"; params: ListAgents; result: AgentsList; error: AgentsError };
  "agents.profiles": { kind: "query"; confirm: "none"; params: PingParams; result: AgentProfiles; error: AgentsError };
  "debug.echo": { kind: "stream"; confirm: "none"; params: EchoParams; result: StreamOpened; error: DebugError };
  "projects.add": { kind: "mutation"; confirm: "none"; params: AddProject; result: Project; error: ProjectsError };
  "projects.list": { kind: "query"; confirm: "none"; params: ListProjects; result: ProjectList; error: ProjectsError };
  "projects.remove": { kind: "mutation"; confirm: "always"; params: RemoveProject; result: Done; error: ProjectsError };
  "projects.scan": { kind: "query"; confirm: "none"; params: ScanProjects; result: ScanResult; error: ProjectsError };
  "system.doctor": { kind: "query"; confirm: "none"; params: PingParams; result: DoctorReport; error: SystemError };
  "system.info": { kind: "query"; confirm: "none"; params: PingParams; result: DaemonInfo; error: SystemError };
  "system.ping": { kind: "query"; confirm: "none"; params: PingParams; result: Pong; error: SystemError };
  "terminal.attach": { kind: "stream"; confirm: "none"; params: AttachSession; result: StreamOpened; error: TerminalError };
  "terminal.close": { kind: "mutation"; confirm: "always"; params: CloseSession; result: Done; error: TerminalError };
  "terminal.create": { kind: "mutation"; confirm: "none"; params: CreateSession; result: TerminalSession; error: TerminalError };
  "terminal.list": { kind: "query"; confirm: "none"; params: ListSessions; result: SessionList; error: TerminalError };
  "terminal.profiles": { kind: "query"; confirm: "none"; params: PingParams; result: TerminalProfiles; error: TerminalError };
  "terminal.rename": { kind: "mutation"; confirm: "none"; params: RenameSession; result: Done; error: TerminalError };
  "terminal.resize": { kind: "mutation"; confirm: "none"; params: ResizeStream; result: Done; error: TerminalError };
  "terminal.zellij_list": { kind: "query"; confirm: "none"; params: PingParams; result: ZellijSessions; error: TerminalError };
}

export interface Topics {
  "agents.attention": AgentsAttention;
  "agents.changed": AgentsChanged;
  "projects.changed": ProjectsChanged;
  "terminal.changed": TerminalChanged;
}

export const METHOD_INFO = {
  "agents.create": { kind: "mutation", confirm: "none" },
  "agents.event": { kind: "mutation", confirm: "none" },
  "agents.get": { kind: "query", confirm: "none" },
  "agents.list": { kind: "query", confirm: "none" },
  "agents.profiles": { kind: "query", confirm: "none" },
  "debug.echo": { kind: "stream", confirm: "none" },
  "projects.add": { kind: "mutation", confirm: "none" },
  "projects.list": { kind: "query", confirm: "none" },
  "projects.remove": { kind: "mutation", confirm: "always" },
  "projects.scan": { kind: "query", confirm: "none" },
  "system.doctor": { kind: "query", confirm: "none" },
  "system.info": { kind: "query", confirm: "none" },
  "system.ping": { kind: "query", confirm: "none" },
  "terminal.attach": { kind: "stream", confirm: "none" },
  "terminal.close": { kind: "mutation", confirm: "always" },
  "terminal.create": { kind: "mutation", confirm: "none" },
  "terminal.list": { kind: "query", confirm: "none" },
  "terminal.profiles": { kind: "query", confirm: "none" },
  "terminal.rename": { kind: "mutation", confirm: "none" },
  "terminal.resize": { kind: "mutation", confirm: "none" },
  "terminal.zellij_list": { kind: "query", confirm: "none" },
} as const;

export const RPC_ERROR_CODES = [
  "agents.bad_state",
  "agents.base_dirty",
  "agents.base_not_checked_out",
  "agents.branch_exists",
  "agents.conflict",
  "agents.gh_missing",
  "agents.invalid_input",
  "agents.not_found",
  "agents.not_repo",
  "agents.operation_in_progress",
  "agents.profile_not_found",
  "agents.project_not_found",
  "agents.session_metadata",
  "agents.session_missing",
  "agents.slug_exhausted",
  "agents.state_changed",
  "agents.title_invalid",
  "agents.unmerged_commits",
  "agents.unsafe_worktree",
  "agents.worktree_add_partial",
  "agents.worktree_dirty",
  "cancelled",
  "daemon_restarting",
  "debug.unsupported",
  "hello_required",
  "internal",
  "invalid_params",
  "method_not_found",
  "not_found",
  "projects.already_exists",
  "projects.not_directory",
  "projects.not_found",
  "projects.path_invalid",
  "secrets_locked",
  "system.unavailable",
  "terminal.cwd_invalid",
  "terminal.invalid_size",
  "terminal.not_found",
  "terminal.project_not_found",
  "terminal.stream_unavailable",
] as const;
export type RpcErrorCode = (typeof RPC_ERROR_CODES)[number];
