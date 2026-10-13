// 이 파일은 `pnpm gen`이 만든다. 직접 고치지 않는다.
/* eslint-disable */

export type AddProject = { path: string, };
export type AgentChanges = { base: string, ahead: number, behind: number, headOid: string, baseOid: string, files: Array<FileChange>, };
export type AgentCommits = { items: Array<Commit>, };
export type AgentConfig = { copy: Array<string>, setup: Array<string>, teardown: Array<string>, test: string | null, };
export type AgentEventParams = { agentId: string, source: HookSource, payload: string, code: number | null, };
export type AgentGitStat = { id: string, changedFiles: number, ahead: number, behind: number, };
export type AgentGitStats = { items: Array<AgentGitStat>, };
export type AgentPatch = { patch: string, truncated: boolean, };
export type AgentProfiles = { items: Array<ToolProfile>, };
export type AgentRebase = { result: RebaseResult, files: Array<string>, };
export type AgentResource = { id: string, cpuPercent: number | null, memoryBytes: number | null, };
export type AgentResources = { items: Array<AgentResource>, };
export type AgentState = "preparing" | "running" | "waiting" | "idle" | "failed" | "review" | "closed";
export type AgentTask = { id: string, projectId: string, title: string, profile: string, baseBranch: string, branch: string | null, worktreePath: string | null, workdir: string, sessionId: string | null, terminalId: string | null, state: AgentState, reason: string | null, attention: Attention | null, port: number, prompt: string, followUps: Array<FollowUp>, tokens: Tokens | null, tokensTruncated: boolean, activitySeq: number, prUrl: string | null, closedHow: CloseHow | null, createdMs: number, updatedMs: number, closedMs: number | null, closedCommits: number | null, closedHeadOid: string | null, cleanupError: string | null, };
export type AgentTest = { terminalId: string, };
export type AgentUsage = { tokens: Tokens | null, truncated: boolean, };
export type AgentsAttention = { id: string, state: AgentState, message: string, };
export type AgentsChanged = { rev: string, id: string, };
export type AgentsError = { "code": "agents.title_invalid" } | { "code": "agents.slug_exhausted" } | { "code": "agents.not_found", "detail": { id: string, } } | { "code": "agents.project_not_found", "detail": { id: string, } } | { "code": "agents.profile_not_found", "detail": { id: string, } } | { "code": "agents.bad_state", "detail": { state: string, } } | { "code": "agents.worktree_dirty", "detail": { files: Array<string>, } } | { "code": "agents.base_dirty", "detail": { files: Array<string>, } } | { "code": "agents.base_not_checked_out", "detail": { branch: string, } } | { "code": "agents.conflict", "detail": { files: Array<string>, } } | { "code": "agents.unmerged_commits", "detail": { count: number, } } | { "code": "agents.gh_missing" } | { "code": "agents.not_repo" } | { "code": "agents.state_changed", "detail": { head: string, base: string, unmerged: number, } } | { "code": "agents.operation_in_progress" } | { "code": "agents.unsafe_worktree" } | { "code": "agents.branch_exists", "detail": { branch: string, } } | { "code": "agents.worktree_add_partial", "detail": { branch: string, path: string, path_exists: boolean, branch_exists: boolean | null, worktree_registered: boolean | null, } } | { "code": "agents.invalid_input" } | { "code": "agents.session_missing" } | { "code": "agents.session_metadata", "detail": { diagnostic: Diagnostic, } };
export type AgentsList = { rev: string, items: Array<AgentTask>, };
export type AttachSession = { id: string, cols: number, rows: number, readOnly: boolean, };
export type Attention = { kind: string, message: string, atMs: number, };
export type Candidate = { path: string, name: string, isGit: boolean, registered: boolean, };
export type ChangeStatus = "added" | "modified" | "deleted" | "renamed" | "unmerged";
export type Check = { id: string, status: Status, message: string, fix: string | null, };
export type CleanupAgents = { projectId: string, worktrees: Array<string>, branches: Array<string>, };
export type CleanupCandidates = { projectId: string, };
export type CleanupOptions = { worktrees: Array<string>, branches: Array<string>, };
export type CloneProject = { url: string, dir: string | null, };
export type CloseHow = "merged" | "pullRequest" | "discarded";
export type CloseSession = { id: string, };
export type Commit = { sha: string, subject: string, author: string, timeMs: number, };
export type CommonError = { "code": "internal" } | { "code": "invalid_params", "detail": { reason: string, } } | { "code": "method_not_found", "detail": { method: string, } } | { "code": "hello_required" } | { "code": "cancelled" } | { "code": "daemon_restarting" } | { "code": "not_found" } | { "code": "secrets_locked" };
export type CreateAgentTask = { projectId: string, title: string, profile: string, baseBranch: string | null, worktree: boolean, prompt: string, };
export type CreateSession = { projectId: string | null, cwd: string | null, title: string | null, command: string | null, attachZellij: string | null, };
export type DaemonInfo = { version: string, instance: string, pid: number, startedMs: number, dataDir: string, };
export type DebugError = { "code": "debug.unsupported" };
export type Diagnostic = "missing" | "ambiguous" | "limitExceeded" | "invalid" | "unsafePath" | "unavailable";
export type DiscardAgent = { id: string, expect: ReviewExpect & { unmergedCommits: number }, };
export type DiscardCheck = { unmergedCommits: number, dirtyFiles: number, headOid: string, };
export type DoctorReport = { checks: Array<Check>, };
export type Done = { rev: string, };
export type EchoParams = { bytes: number, };
export type FileChange = { path: string, oldPath: string | null, status: ChangeStatus, added: number, removed: number, };
export type FollowUp = { atMs: number, text: string, };
export type GetAgent = { id: string, };
export type GetAgentPatch = { id: string, path: string, };
export type GetProjectConfig = { id: string, };
export type HookSource = "claude" | "codex" | "setup" | "exit";
export type Hooks = "claude" | "codex" | "none";
export type LaunchAgent = { tool: string, worktree: boolean, prompt: string | null, };
export type LaunchConfig = { services: Array<string>, terminals: Array<LaunchTerminal>, agents: Array<LaunchAgent>, };
export type LaunchProject = { id: string, };
export type LaunchResult = { terminals: number, agents: number, };
export type LaunchTerminal = { name: string, command: string | null, };
export type ListAgents = { includeClosedHours: number | null, };
export type ListProjects = Record<symbol, never>;
export type ListSessions = { projectId: string | null, };
export type MergeAgent = { id: string, strategy: MergeKind, message: string | null, expect: ReviewExpect & { baseOid: string }, };
export type MergeKind = "merge" | "squash" | "rebase";
export type PingParams = Record<symbol, never>;
export type Pong = { daemonId: string, version: string, uptimeMs: number, };
export type PrAgent = { id: string, title: string | null, body: string | null, expect: ReviewExpect & { baseOid: string }, };
export type Project = { id: string, name: string, path: string, favorite: boolean, addedMs: number, };
export type ProjectAgentCounts = { open: number, attention: number, };
export type ProjectConfig = { agent: AgentConfig, launch: LaunchConfig, };
export type ProjectConfigIssue = { message: string, };
export type ProjectConfigResult = { config: ProjectConfig, error?: ProjectConfigIssue, };
export type ProjectList = { rev: string, items: Array<Project>, };
export type ProjectPrompt = { name: string, body: string, };
export type ProjectPrompts = { id: string, };
export type ProjectSummary = { id: string, branch?: string, dirty: boolean | null, ahead: number | null, behind: number | null, terminals: number, agents: ProjectAgentCounts, error?: ProjectSummaryIssue, };
export type ProjectSummaryIssue = { message: string, };
export type ProjectSummaryList = { items: Array<ProjectSummary>, };
export type ProjectsChanged = { rev: string, };
export type ProjectsError = { "code": "projects.path_invalid", "detail": { path: string, } } | { "code": "projects.not_directory", "detail": { path: string, } } | { "code": "projects.already_exists", "detail": { path: string, } } | { "code": "projects.not_found", "detail": { id: string, } } | { "code": "projects.configuration_invalid", "detail": { message: string, } } | { "code": "projects.launch_profile_not_found", "detail": { id: string, } } | { "code": "projects.launch_failed", "detail": { terminals: number, agents: number, message: string, } } | { "code": "projects.clone_invalid_url" } | { "code": "projects.clone_target_exists", "detail": { path: string, } } | { "code": "projects.clone_failed", "detail": { path: string, message: string, } } | { "code": "projects.unsafe_prompt_path", "detail": { path: string, } } | { "code": "projects.prompt_limit_exceeded", "detail": { max_files: number, max_bytes: number, } } | { "code": "projects.prompt_invalid_encoding", "detail": { name: string, } };
export type PromptList = { items: Array<ProjectPrompt>, };
export type RebaseResult = "done" | "conflict";
export type RemoveProject = { id: string, };
export type RenameSession = { id: string, title: string, };
export type ResizeStream = { streamId: number, cols: number, rows: number, };
export type ReviewExpect = { headOid: string, baseOid?: string, unmergedCommits?: number, };
export type ScanProjects = Record<symbol, never>;
export type ScanResult = { candidates: Array<Candidate>, };
export type SendToAgent = { id: string, text: string, };
export type SessionKind = "shell" | "agent";
export type SessionList = { rev: string, items: Array<TerminalSession>, };
export type SetFavorite = { id: string, favorite: boolean, };
export type Status = "ok" | "warn" | "fail";
export type StreamOpened = { streamId: number, };
export type SummaryProjects = Record<symbol, never>;
export type SystemError = { "code": "system.unavailable" };
export type TerminalAttached = { streamId: number, cols: number, rows: number, };
export type TerminalChanged = { rev: string, };
export type TerminalError = { "code": "terminal.invalid_size", "detail": { cols: number, rows: number, } } | { "code": "terminal.stream_unavailable", "detail": Record<symbol, never> } | { "code": "terminal.not_found", "detail": { id: string, } } | { "code": "terminal.project_not_found", "detail": { id: string, } } | { "code": "terminal.cwd_invalid", "detail": { path: string, } };
export type TerminalProfile = { name: string, command: string, };
export type TerminalProfiles = { items: Array<TerminalProfile>, };
export type TerminalSession = { id: string, projectId: string | null, title: string, cwd: string, kind: SessionKind, alive: boolean, command: string, createdMs: number, };
export type Tokens = { input: number, output: number, cacheRead: number, cacheWrite: number, };
export type ToolProfile = { id: string, label: string, command: string, hooks: Hooks, waitingPatterns: Array<string>, };
export type ZellijSessions = { items: Array<string>, };

export interface Methods {
  "agents.changes": { kind: "query"; confirm: "none"; params: GetAgent; result: AgentChanges; error: AgentsError };
  "agents.cleanup": { kind: "mutation"; confirm: "always"; params: CleanupAgents; result: Done; error: AgentsError };
  "agents.cleanup_candidates": { kind: "query"; confirm: "none"; params: CleanupCandidates; result: CleanupOptions; error: AgentsError };
  "agents.commits": { kind: "query"; confirm: "none"; params: GetAgent; result: AgentCommits; error: AgentsError };
  "agents.create": { kind: "mutation"; confirm: "none"; params: CreateAgentTask; result: AgentTask; error: AgentsError };
  "agents.discard": { kind: "mutation"; confirm: "always"; params: DiscardAgent; result: AgentTask; error: AgentsError };
  "agents.discard_check": { kind: "query"; confirm: "none"; params: GetAgent; result: DiscardCheck; error: AgentsError };
  "agents.event": { kind: "mutation"; confirm: "none"; params: AgentEventParams; result: Done; error: AgentsError };
  "agents.get": { kind: "query"; confirm: "none"; params: GetAgent; result: AgentTask; error: AgentsError };
  "agents.git_stats": { kind: "query"; confirm: "none"; params: PingParams; result: AgentGitStats; error: AgentsError };
  "agents.list": { kind: "query"; confirm: "none"; params: ListAgents; result: AgentsList; error: AgentsError };
  "agents.merge": { kind: "mutation"; confirm: "always"; params: MergeAgent; result: AgentTask; error: AgentsError };
  "agents.patch": { kind: "query"; confirm: "none"; params: GetAgentPatch; result: AgentPatch; error: AgentsError };
  "agents.pr": { kind: "mutation"; confirm: "always"; params: PrAgent; result: AgentTask; error: AgentsError };
  "agents.profiles": { kind: "query"; confirm: "none"; params: PingParams; result: AgentProfiles; error: AgentsError };
  "agents.rebase": { kind: "mutation"; confirm: "none"; params: GetAgent; result: AgentRebase; error: AgentsError };
  "agents.rebase_abort": { kind: "mutation"; confirm: "none"; params: GetAgent; result: AgentTask; error: AgentsError };
  "agents.resources": { kind: "query"; confirm: "none"; params: PingParams; result: AgentResources; error: AgentsError };
  "agents.resume": { kind: "mutation"; confirm: "none"; params: GetAgent; result: AgentTask; error: AgentsError };
  "agents.review": { kind: "mutation"; confirm: "none"; params: GetAgent; result: AgentTask; error: AgentsError };
  "agents.send": { kind: "mutation"; confirm: "none"; params: SendToAgent; result: AgentTask; error: AgentsError };
  "agents.test": { kind: "mutation"; confirm: "none"; params: GetAgent; result: AgentTest; error: AgentsError };
  "agents.usage": { kind: "query"; confirm: "none"; params: GetAgent; result: AgentUsage; error: AgentsError };
  "debug.echo": { kind: "stream"; confirm: "none"; params: EchoParams; result: StreamOpened; error: DebugError };
  "projects.add": { kind: "mutation"; confirm: "none"; params: AddProject; result: Project; error: ProjectsError };
  "projects.clone": { kind: "mutation"; confirm: "none"; params: CloneProject; result: Project; error: ProjectsError };
  "projects.config": { kind: "query"; confirm: "none"; params: GetProjectConfig; result: ProjectConfigResult; error: ProjectsError };
  "projects.launch": { kind: "mutation"; confirm: "none"; params: LaunchProject; result: LaunchResult; error: ProjectsError };
  "projects.list": { kind: "query"; confirm: "none"; params: ListProjects; result: ProjectList; error: ProjectsError };
  "projects.prompts": { kind: "query"; confirm: "none"; params: ProjectPrompts; result: PromptList; error: ProjectsError };
  "projects.remove": { kind: "mutation"; confirm: "always"; params: RemoveProject; result: Done; error: ProjectsError };
  "projects.scan": { kind: "query"; confirm: "none"; params: ScanProjects; result: ScanResult; error: ProjectsError };
  "projects.set_favorite": { kind: "mutation"; confirm: "none"; params: SetFavorite; result: Done; error: ProjectsError };
  "projects.summary": { kind: "query"; confirm: "none"; params: SummaryProjects; result: ProjectSummaryList; error: ProjectsError };
  "system.doctor": { kind: "query"; confirm: "none"; params: PingParams; result: DoctorReport; error: SystemError };
  "system.info": { kind: "query"; confirm: "none"; params: PingParams; result: DaemonInfo; error: SystemError };
  "system.ping": { kind: "query"; confirm: "none"; params: PingParams; result: Pong; error: SystemError };
  "terminal.attach": { kind: "stream"; confirm: "none"; params: AttachSession; result: TerminalAttached; error: TerminalError };
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
  "agents.changes": { kind: "query", confirm: "none" },
  "agents.cleanup": { kind: "mutation", confirm: "always" },
  "agents.cleanup_candidates": { kind: "query", confirm: "none" },
  "agents.commits": { kind: "query", confirm: "none" },
  "agents.create": { kind: "mutation", confirm: "none" },
  "agents.discard": { kind: "mutation", confirm: "always" },
  "agents.discard_check": { kind: "query", confirm: "none" },
  "agents.event": { kind: "mutation", confirm: "none" },
  "agents.get": { kind: "query", confirm: "none" },
  "agents.git_stats": { kind: "query", confirm: "none" },
  "agents.list": { kind: "query", confirm: "none" },
  "agents.merge": { kind: "mutation", confirm: "always" },
  "agents.patch": { kind: "query", confirm: "none" },
  "agents.pr": { kind: "mutation", confirm: "always" },
  "agents.profiles": { kind: "query", confirm: "none" },
  "agents.rebase": { kind: "mutation", confirm: "none" },
  "agents.rebase_abort": { kind: "mutation", confirm: "none" },
  "agents.resources": { kind: "query", confirm: "none" },
  "agents.resume": { kind: "mutation", confirm: "none" },
  "agents.review": { kind: "mutation", confirm: "none" },
  "agents.send": { kind: "mutation", confirm: "none" },
  "agents.test": { kind: "mutation", confirm: "none" },
  "agents.usage": { kind: "query", confirm: "none" },
  "debug.echo": { kind: "stream", confirm: "none" },
  "projects.add": { kind: "mutation", confirm: "none" },
  "projects.clone": { kind: "mutation", confirm: "none" },
  "projects.config": { kind: "query", confirm: "none" },
  "projects.launch": { kind: "mutation", confirm: "none" },
  "projects.list": { kind: "query", confirm: "none" },
  "projects.prompts": { kind: "query", confirm: "none" },
  "projects.remove": { kind: "mutation", confirm: "always" },
  "projects.scan": { kind: "query", confirm: "none" },
  "projects.set_favorite": { kind: "mutation", confirm: "none" },
  "projects.summary": { kind: "query", confirm: "none" },
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
  "projects.clone_failed",
  "projects.clone_invalid_url",
  "projects.clone_target_exists",
  "projects.configuration_invalid",
  "projects.launch_failed",
  "projects.launch_profile_not_found",
  "projects.not_directory",
  "projects.not_found",
  "projects.path_invalid",
  "projects.prompt_invalid_encoding",
  "projects.prompt_limit_exceeded",
  "projects.unsafe_prompt_path",
  "secrets_locked",
  "system.unavailable",
  "terminal.cwd_invalid",
  "terminal.invalid_size",
  "terminal.not_found",
  "terminal.project_not_found",
  "terminal.stream_unavailable",
] as const;
export type RpcErrorCode = (typeof RPC_ERROR_CODES)[number];
