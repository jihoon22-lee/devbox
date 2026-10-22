// 이 파일은 `pnpm gen`이 만든다. 직접 고치지 않는다.
/* eslint-disable */

export type AddProject = { path: string, };
export type AgentChanges = { base: string, ahead: number, behind: number, headOid: string, baseOid: string, files: Array<FileChange>, };
export type AgentCommits = { items: Array<Commit>, };
export type AgentConfig = { copy: Array<string>, setup: Array<string>, teardown: Array<string>, test: string | null, };
export type AgentEventParams = { agentId: string, launchId?: string, source: HookSource, payload: string, code: number | null, };
export type AgentGitFailure = { id: string, code: string, };
export type AgentGitStat = { id: string, changedFiles: number, ahead: number, behind: number, };
export type AgentGitStats = { items: Array<AgentGitStat>, failures: Array<AgentGitFailure>, };
export type AgentPatch = { patch: string, truncated: boolean, };
export type AgentProfiles = { items: Array<ToolProfile>, };
export type AgentRebase = { result: RebaseResult, files: Array<string>, };
export type AgentRebaseStatus = { owned: boolean, files: Array<string>, };
export type AgentResource = { id: string, cpuPercent: number | null, memoryBytes: number | null, };
export type AgentResources = { items: Array<AgentResource>, };
export type AgentState = "preparing" | "running" | "waiting" | "idle" | "failed" | "review" | "closed";
export type AgentTask = { id: string, projectId: string, title: string, profile: string, baseBranch: string, branch: string | null, worktreePath: string | null, workdir: string, sessionId: string | null, terminalId: string | null, toolExited: boolean, state: AgentState, reason: string | null, attention: Attention | null, port: number, prompt: string, followUps: Array<FollowUp>, tokens: Tokens | null, tokensTruncated: boolean, activitySeq: number, prUrl: string | null, closedHow: CloseHow | null, createdMs: number, updatedMs: number, closedMs: number | null, closedCommits: number | null, closedHeadOid: string | null, cleanupError: string | null, };
export type AgentTest = { runId: string, };
export type AgentUsage = { tokens: Tokens | null, truncated: boolean, };
export type AgentsAttention = { id: string, state: AgentState, message: string, };
export type AgentsChanged = { rev: string, id: string, };
export type AgentsError = { "code": "agents.title_invalid" } | { "code": "agents.slug_exhausted" } | { "code": "agents.not_found", "detail": { id: string, } } | { "code": "agents.project_not_found", "detail": { id: string, } } | { "code": "agents.profile_not_found", "detail": { id: string, } } | { "code": "agents.bad_state", "detail": { state: string, } } | { "code": "agents.worktree_dirty", "detail": { files: Array<string>, } } | { "code": "agents.base_dirty", "detail": { files: Array<string>, } } | { "code": "agents.base_not_checked_out", "detail": { branch: string, } } | { "code": "agents.conflict", "detail": { files: Array<string>, } } | { "code": "agents.unmerged_commits", "detail": { count: number, } } | { "code": "agents.gh_missing" } | { "code": "agents.not_repo" } | { "code": "agents.state_changed", "detail": { head: string, base: string, unmerged: number, } } | { "code": "agents.operation_in_progress" } | { "code": "agents.unsafe_worktree" } | { "code": "agents.branch_exists", "detail": { branch: string, } } | { "code": "agents.worktree_add_partial", "detail": { branch: string, path: string, path_exists: boolean, branch_exists: boolean | null, worktree_registered: boolean | null, } } | { "code": "agents.invalid_input" } | { "code": "agents.session_missing" } | { "code": "agents.session_metadata", "detail": { diagnostic: Diagnostic, } } | { "code": "agents.input_too_large", "detail": { limit_bytes: number, } } | { "code": "agents.pr_title_too_long", "detail": { limit_bytes: number, } } | { "code": "agents.squash_message_too_long", "detail": { limit_bytes: number, } } | { "code": "agents.teardown_command_too_long", "detail": { limit_bytes: number, } } | { "code": "agents.teardown_command_invalid" } | { "code": "agents.delivery_unknown" } | { "code": "agents.resume_unavailable" } | { "code": "agents.review_unavailable" } | { "code": "agents.discard_snapshot_unavailable", "detail": { reason: string, } } | { "code": "agents.list_changed" } | { "code": "agents.project_trust_required", "detail": { id: string, } };
export type AgentsList = { rev: string, items: Array<AgentTask>, nextAfterId?: string, };
export type AttachImage = { notePath: string, name: string, 
/**
 * Standard base64 of the image bytes.
 */
data: string, };
export type AttachSession = { id: string, cols: number, rows: number, readOnly: boolean, };
export type Attached = { path: string, markdown: string, };
export type Attention = { kind: string, message: string, atMs: number, };
export type Backlink = { src: string, title: string, 
/**
 * 1-based line of the link in `src`.
 */
line: number, fromUtf16: number, toUtf16: number, };
export type Backlinks = { rev: string, items: Array<Backlink>, truncated: boolean, };
export type Candidate = { path: string, name: string, isGit: boolean, registered: boolean, };
export type ChangeStatus = "added" | "modified" | "deleted" | "renamed" | "unmerged";
export type Check = { id: string, status: Status, message: string, fix: string | null, };
export type ClaimDocument = { key: DocKey, policy: DocPolicy, windowId: string, };
export type Claimed = { key: DocKey, leaseId: string | null, rev: DiskRev | null, text: string | null, eol: Eol, bom: boolean, readOnly: ReadOnly | null, journal: JournalView | null, };
export type CleanupAgents = { projectId: string, worktrees: Array<string>, branches: Array<string>, };
export type CleanupCandidates = { projectId: string, };
export type CleanupOptions = { worktrees: Array<string>, branches: Array<string>, };
export type CloneProject = { url: string, dir: string | null, };
export type CloseHow = "merged" | "pullRequest" | "discarded";
export type CloseSession = { id: string, };
export type Commit = { sha: string, subject: string, author: string, timeMs: number, };
export type CommonError = { "code": "internal" } | { "code": "invalid_params", "detail": { reason: string, } } | { "code": "method_not_found", "detail": { method: string, } } | { "code": "hello_required" } | { "code": "cancelled" } | { "code": "daemon_restarting" } | { "code": "not_found" } | { "code": "secrets_locked" };
export type ContainerAction = "start" | "stop" | "restart";
export type ContainerActionParams = { id: string, action: ContainerAction, };
export type ContainerActionResult = { id: string, action: ContainerAction, undo: ContainerActionParams | null, };
export type ContainerError = { "code": "runtime.container_invalid_id" } | { "code": "runtime.container_unavailable" } | { "code": "runtime.container_failed" } | { "code": "runtime.container_timeout" } | { "code": "runtime.container_output_limit" } | { "code": "runtime.container_invalid_output" } | { "code": "runtime.container_cancelled" };
export type ContainerInfo = { id: string, name: string, image: string, state: string, status: string, ports: string, composeProject?: string, };
export type ContainerUnavailableReason = "cliMissing" | "daemonUnavailable";
export type ContainersQuery = Record<symbol, never>;
export type ContainersResult = { available: boolean, reason?: ContainerUnavailableReason, items: Array<ContainerInfo>, };
export type CreateAgentTask = { projectId: string, title: string, profile: string, baseBranch: string | null, worktree: boolean, prompt: string, };
export type CreateNote = { 
/**
 * Vault-relative folder, `""` for the vault root.
 */
folder: string, title: string, 
/**
 * Template name from `notes.templates`, or `null` for the default body.
 */
template: string | null, };
export type CreateSession = { projectId: string | null, cwd: string | null, title: string | null, command: string | null, attachZellij: string | null, };
export type CreatedNote = { path: string, rev: DiskRev, };
export type DaemonInfo = { version: string, instance: string, pid: number, startedMs: number, dataDir: string, configPath: string, hostname?: string, };
export type DebugError = { "code": "debug.unsupported" };
export type DefineResult = { rev: string, definition: Definition, };
export type DefineRun = { projectId: string, def: RunDef, saveTo: SaveTo, };
export type Definition = { source: DefinitionSource, def: RunDef, };
export type DefinitionCursor = { id: string, shadowed: boolean, };
export type DefinitionKind = "task" | "service";
export type DefinitionList = { rev: string, defs: Array<Definition>, shadowed: Array<Definition>, runs: Array<Run>, trust: TrustPreviewStatus, nextCursor: DefinitionCursor | null, };
export type DefinitionSource = "projectFile" | "thisPc";
export type DetectRuns = { projectId: string, };
export type Diagnostic = "missing" | "ambiguous" | "limitExceeded" | "invalid" | "unsafePath" | "unavailable";
export type DiscardAgent = { id: string, expect: ReviewExpect & { unmergedCommits: number }, };
export type DiscardCheck = { unmergedCommits: number, dirtyFiles: number, headOid: string, fingerprint: string | null, };
export type DiskDocument = { key: DocKey, rev: DiskRev | null, text: string | null, readOnly: ReadOnly | null, };
export type DiskRev = { hash: string, size: number, mtimeNs: string, ino: string, };
export type DocKey = { root: string, path: string, };
export type DocKind = "note" | "code";
export type DocPolicy = { kind: DocKind, };
export type DoctorReport = { checks: Array<Check>, };
export type DocumentChanged = { key: DocKey, rev: DiskRev | null, };
export type DocumentMoved = { from: DocKey, to: DocKey, };
export type DocumentsError = { "code": "documents.conflict", "detail": { rev: DiskRev | null, } } | { "code": "documents.lease_lost", "detail": { reason: LeaseLoss, } } | { "code": "documents.out_of_scope" } | { "code": "documents.not_found" } | { "code": "documents.exists" } | { "code": "documents.too_large", "detail": { limit: number, } } | { "code": "documents.stale_journal" } | { "code": "documents.unsupported" } | { "code": "documents.invalid_input" };
export type Done = { rev: string, };
export type DropJournal = { key: DocKey, uptoSeq: number, 
/**
 * The caller's lease. While another live lease holds the key the drop is refused, so a
 * read-only window can never remove the holder's journal.
 */
leaseId?: string, };
export type EchoParams = { bytes: number, };
export type Empty = Record<symbol, never>;
export type Eol = "lf" | "crlf";
export type ExitRecord = { attemptId: string, code: number | null, signal?: string, endedMs: number, by: ExitSource, serviceResult?: string, };
export type ExitSource = "wrapper" | "systemd";
export type Exited = { runId: string, attemptId: string, };
export type Expect = "present" | "absent";
export type FailedRewrite = { path: string, reason: RewriteFailure, };
export type FileChange = { path: string, oldPath: string | null, status: ChangeStatus, added: number, removed: number, };
export type FileLocation = { path: string, line: number, column: number, };
export type FilesystemInfo = { totalBytes: number, availableBytes: number, usedBytes: number, };
export type FollowUp = { atMs: number, text: string, instructionId?: string, delivery?: InstructionDelivery, };
export type GetAgent = { id: string, };
export type GetAgentPatch = { id: string, path: string, };
export type GetProjectConfig = { id: string, };
export type HookSource = "claude" | "codex" | "setup" | "exit";
export type Hooks = "claude" | "codex" | "none";
export type ImageProblem = "empty" | "too_large" | "unsupported" | "dimensions" | "encoding";
export type IncompleteReason = "failed" | "target_exists" | "source_missing" | "move_failed" | "unreadable";
export type InstructionDelivery = "pending" | "delivered" | "unknown";
export type JournalText = { seq: number, baseRev: DiskRev | null, text: string, };
export type JournalView = { seq: number, baseRev: DiskRev | null, text: string | null, 
/**
 * True when `text` did not fit the response budget; read it with `documents.journal_read`.
 */
deferred: boolean, };
export type KillProcess = { pid: number, signal: KillSignal, expect: ProcessExpectation, };
export type KillResult = { pid: number, };
export type KillSignal = "term" | "kill";
export type LaunchAgent = { tool: string, worktree: boolean, prompt: string | null, };
export type LaunchConfig = { services: Array<string>, terminals: Array<LaunchTerminal>, agents: Array<LaunchAgent>, };
export type LaunchProject = { id: string, };
export type LaunchResult = { services: number, runIds: Array<string>, terminals: number, agents: number, terminalIds: Array<string>, rev: string, };
export type LaunchTerminal = { name: string, command: string | null, };
export type LeaseLoss = "taken" | "expired";
export type LeaseLost = { key: DocKey, leaseId: string, };
export type LeaseRef = { leaseId: string, };
export type LinkKind = "wiki" | "md";
export type ListAgents = { includeClosedHours: number | null, afterId?: string, };
export type ListProjects = Record<symbol, never>;
export type ListRuns = { projectId: string, cursor: DefinitionCursor | null, };
export type ListSessions = { projectId: string | null, afterId?: string, };
export type LogFilter = { text?: string, regex?: string, streams?: Array<LogStream>, };
export type LogFrom = Tail | { offset: number, };
export type LogLine = { src: LogSource, ts: string, stream: LogStream, text: string, };
export type LogSource = { "kind": "run", runId: string, } | { "kind": "file", path: string, } | { "kind": "container", id: string, };
export type LogStream = "stdout" | "stderr" | "system";
export type MergeAgent = { id: string, strategy: MergeKind, message: string | null, expect: ReviewExpect & { baseOid: string }, };
export type MergeKind = "merge" | "squash" | "rebase";
export type MissedPolicy = "skip" | "runOnce";
export type Name = { name: string, };
export type NoParams = Record<symbol, never>;
export type NotePath = { path: string, };
export type NoteTree = { rev: string, items: Array<TreeItem>, nextCursor: string | null, truncated: boolean, };
export type NotesChanged = { rev: string, paths: Array<string>, };
export type NotesError = { "code": "notes.not_found" } | { "code": "notes.exists" } | { "code": "notes.out_of_vault" } | { "code": "notes.invalid_name" } | { "code": "notes.invalid_template", "detail": { problem: TemplateProblem, } } | { "code": "notes.invalid_image", "detail": { problem: ImageProblem, } } | { "code": "notes.invalid_input" } | { "code": "notes.busy" } | { "code": "notes.target_unsaved" } | { "code": "notes.stale_cursor" } | { "code": "notes.too_large", "detail": { limit: number, } };
export type Notification = { id: string, projectId: string, runId: string | null, task: string | null, reason: NotificationReason, };
export type NotificationReason = "startLimit" | "schedule_failed" | "schedule_trust_required";
export type ObserveError = { "code": "observe.invalid_input" } | { "code": "observe.state_changed" } | { "code": "observe.permission_denied" } | { "code": "observe.project_not_found" } | { "code": "observe.invalid_regex" } | { "code": "observe.file_outside_roots" } | { "code": "observe.source_unavailable" } | { "code": "observe.run_not_found" } | { "code": "observe.invalid_cursor" } | { "code": "observe.limit_exceeded" } | { "code": "observe.stream_unsupported" };
export type ObserveQuery = Record<symbol, never>;
export type OpenLogs = { sources: Array<LogSource>, from: LogFrom, filter?: LogFilter, };
export type PingParams = Record<symbol, never>;
export type Pong = { daemonId: string, version: string, uptimeMs: number, };
export type PortInfo = { port: number, proto: string, addr: string, pid: number | null, process: string | null, startTimeMs: number | null, runId: string | null, containerId: string | null, projectId: string | null, };
export type PrAgent = { id: string, title: string | null, body: string | null, expect: ReviewExpect & { baseOid: string }, };
export type Pressure = { some: PressureLine | null, full: PressureLine | null, };
export type PressureLine = { avg10: number, avg60: number, avg300: number, totalUs: number, };
export type Problem = { file: string, line: number, col: number | null, severity: ProblemSeverity, message: string, source: string, };
export type ProblemSeverity = "error" | "warning" | "info";
export type ProblemsChange = { rev: string, projectId: string, };
export type ProblemsCursor = { rev: string, offset: number, };
export type ProblemsQuery = { projectId: string, limit?: number, cursor?: ProblemsCursor, };
export type ProblemsResult = { rev: string, items: Array<Problem>, nextCursor: ProblemsCursor | null, truncated: boolean, };
export type ProcessExpectation = { startTimeMs: number, };
export type ProcessInfo = { pid: number, name: string, uid: number, cwd: string | null, startTimeMs: number, cpuPercent: number, rssBytes: number, unit: string | null, runId: string | null, projectId: string | null, };
export type ProcessesQuery = { projectId: string | null, };
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
export type ProjectsError = { "code": "projects.path_invalid", "detail": { path: string, } } | { "code": "projects.not_directory", "detail": { path: string, } } | { "code": "projects.already_exists", "detail": { path: string, } } | { "code": "projects.not_found", "detail": { id: string, } } | { "code": "projects.in_use", "detail": { id: string, } } | { "code": "projects.configuration_invalid", "detail": { message: string, } } | { "code": "projects.launch_profile_not_found", "detail": { id: string, } } | { "code": "projects.launch_failed", "detail": { terminals: number, agents: number, services: number, run_ids: Array<string>, message: string, } } | { "code": "projects.clone_invalid_url" } | { "code": "projects.clone_target_exists", "detail": { path: string, } } | { "code": "projects.clone_failed", "detail": { path: string, message: string, } } | { "code": "projects.unsafe_prompt_path", "detail": { path: string, } } | { "code": "projects.prompt_limit_exceeded", "detail": { max_files: number, max_bytes: number, } } | { "code": "projects.prompt_invalid_encoding", "detail": { name: string, } } | { "code": "projects.trust_required", "detail": { id: string, } } | { "code": "projects.trust_preview_stale", "detail": { id: string, } } | { "code": "projects.trust_context_invalid" };
export type PromptList = { items: Array<ProjectPrompt>, };
export type PublicKeyStatus = { locked: boolean, keyMismatch: boolean, };
export type ReadDocument = { key: DocKey, 
/**
 * Defaults to `note`.
 */
policy?: DocPolicy, };
export type ReadOnly = { reason: ReadOnlyReason, };
export type ReadOnlyReason = "binary" | "encoding" | "too_large" | "leased_elsewhere";
export type RebaseResult = "done" | "conflict";
export type Recovery = { recovery: string, };
export type RefreshEnvResult = { restarting: boolean, };
export type RemoveProject = { id: string, };
export type RenameDocument = { leaseId: string | null, key: DocKey, to: DocKey, };
export type RenameIncomplete = { opId: string, 
/**
 * Notes left as they were.
 */
count: number, reason: IncompleteReason, };
export type RenameNote = { path: string, newPath: string, 
/**
 * Rewrite links in other notes (and the note's own relative links) that would break.
 */
rewriteLinks: boolean, };
export type RenameOp = { opId: string, };
export type RenameSession = { id: string, title: string, };
export type RenameStarted = { opId: string, };
export type RenameState = "running" | "done" | "incomplete";
export type RenameStatus = { state: RenameState, failed: Array<FailedRewrite>, truncated: boolean, 
/**
 * Why an `incomplete` rename stopped; `null` while running or when done.
 */
reason: IncompleteReason | null, };
export type Renamed = { key: DocKey, };
export type RenderNote = { 
/**
 * Note whose folder anchors relative images; read from disk when `text` is `null`.
 */
path: string | null, text: string | null, };
export type RenderedNote = { html: string, mermaid: Array<string>, };
export type RenewLeases = { leaseIds: Array<string>, };
export type Renewed = { lost: Array<string>, };
export type ResizeStream = { streamId: number, cols: number, rows: number, };
export type ResolveFileLocation = { projectId: string, source: LogSource, path: string, line: number, column: number | null, };
export type ResolveLink = { 
/**
 * `[[target]]` text (`#heading`, `|alias` allowed) or a Markdown link destination.
 */
target: string, kind: LinkKind, 
/**
 * The note that contains the link; nearer candidates win ties. `null` = vault root.
 */
from: string | null, };
export type ResolvedLink = { paths: Array<string>, ambiguous: boolean, };
export type RestartPolicy = "no" | "onFailure" | "always";
export type RestoreNote = { trashPath: string, path: string, };
export type ReviewExpect = { headOid: string, baseOid?: string, unmergedCommits?: number, discardFingerprint?: string, };
export type RewriteFailure = "conflict" | "open" | "missing" | "unsupported";
export type RewriteReply = { opId: string, key: DocKey, 
/**
 * True after the window applied the edits and saved the note.
 */
applied: boolean, };
export type RewriteRequest = { opId: string, key: DocKey, leaseId: string, journalSeq: number, edits: Array<TextEdit>, };
export type Run = { id: string, projectId: string, defId: string, kind: RunKind, state: RunState, exit: ExitRecord | null, startedMs: number, endedMs: number | null, restarts: number, unit: string, ready: boolean, pid: number | null, port: number | null, attemptId: string | null, windows?: WindowsRun, };
export type RunDef = { "kind": "task" } & TaskDef | { "kind": "service" } & ServiceDef | { "kind": "schedule" } & ScheduleDef;
export type RunDetail = { rev: string, run: Run, };
export type RunId = { runId: string, };
export type RunKind = "task" | "service" | "scheduled" | "agentTest";
export type RunState = "starting" | "running" | "restarting" | "exited" | "failed" | "stopped";
export type RunSummary = { rev: string, active: number, };
export type RunSummaryQuery = Record<symbol, never>;
export type RunTarget = "linux" | "windows";
export type RunsQuery = { projectId: string | null, limit: number, };
export type RunsResult = { rev: string, items: Array<Run>, };
export type RuntimeChange = { rev: string, projectId: string, };
export type RuntimeError = { "code": "runtime.def_invalid", "detail": { id: string, reason: string, } } | { "code": "runtime.project_not_found", "detail": { id: string, } } | { "code": "projects.trust_required", "detail": { id: string, } } | { "code": "projects.trust_preview_stale", "detail": { id: string, } } | { "code": "runtime.project_config_invalid", "detail": { message: string, } } | { "code": "runtime.run_not_found", "detail": { id: string, } } | { "code": "runtime.not_ready", "detail": { id: string, } } | { "code": "runtime.dependency_failed", "detail": { id: string, } } | { "code": "runtime.detect_parse", "detail": { file: string, message: string, } } | { "code": "runtime.detect_limit", "detail": { subject: string, limit: number, } } | { "code": "runtime.detect_io", "detail": { file: string, message: string, } } | { "code": "runtime.invalid_cursor" } | { "code": "secrets_locked" } | { "code": "secrets.key_mismatch" } | { "code": "runtime.secret_missing", "detail": { names: Array<string>, } };
export type SaveDocument = { leaseId: string, seq: number, baseRev: DiskRev | null, expect: Expect, text: string, };
export type SaveTo = "project" | "thisPc";
export type Saved = { rev: DiskRev, };
export type ScanProjects = Record<symbol, never>;
export type ScanResult = { candidates: Array<Candidate>, };
export type ScheduleDef = { id: string, task: string, cron: string, tz: string | null, missed: MissedPolicy, };
export type ScheduleLast = { at: string, result: ScheduleOutcome, };
export type ScheduleOutcome = "claimed" | "running" | "succeeded" | "failed" | "stopped" | "skippedMissed" | "skippedTrust";
export type ScheduleStatus = { id: string, cron: string, next: string | null, last: ScheduleLast | null, };
export type SchedulesQuery = { projectId: string, };
export type SchedulesResult = { items: Array<ScheduleStatus>, };
export type SearchCursor = { sources: Array<SourceCursor>, };
export type SearchLogs = { sources: Array<LogSource>, query: string, regex: boolean, before?: SearchCursor, limit: number, };
export type SearchResult = { items: Array<LogLine>, nextCursor: SearchCursor | null, };
export type SecretInfo = { name: string, hasValue: boolean, };
export type SecretsError = { "code": "secrets_locked" } | { "code": "secrets.key_mismatch" } | { "code": "secrets.invalid_input" } | { "code": "secrets.missing", "detail": { names: Array<string>, } } | { "code": "secrets.unavailable" };
export type SendToAgent = { id: string, text: string, };
export type ServiceDef = { id: string, command: string, cwd: string | null, env: { [key in string]: string }, target: RunTarget, dependsOn: Array<string>, worktree: string | null, secrets: Array<string>, restart: RestartPolicy, restartSec: number, startLimit: StartLimit, portsHint: Array<number>, readyTimeoutSec: number, };
export type SessionKind = "shell" | "agent";
export type SessionList = { rev: string, items: Array<TerminalSession>, nextAfterId?: string, };
export type SetFavorite = { id: string, favorite: boolean, };
export type SetSecret = { name: string, value: string, };
export type SourceCursor = { source: LogSource, identity: string, stream: LogStream, offset: number, };
export type StartLimit = { burst: number, intervalSec: number, };
export type StartRun = { projectId: string, id: string, worktree: string | null, };
export type Status = "ok" | "warn" | "fail";
export type StreamOpened = { streamId: number, };
export type Suggestion = { id: string, command: string, kind: DefinitionKind, from: SuggestionSource, };
export type SuggestionSource = "package.json" | "Makefile" | "justfile" | "Cargo.toml" | "compose.yaml" | "docker-compose.yml";
export type SummaryProjects = Record<symbol, never>;
export type SystemError = { "code": "system.unavailable" } | { "code": "system.env_refresh_failed" };
export type SystemInfo = { memoryTotalBytes: number, memoryAvailableBytes: number, swapTotalBytes: number, swapUsedBytes: number, cpuPressure: Pressure | null, memoryPressure: Pressure | null, ioPressure: Pressure | null, homeFilesystem: FilesystemInfo | null, };
export type TagCount = { tag: string, count: number, };
export type Tags = { rev: string, items: Array<TagCount>, truncated: boolean, };
export type Tail = "tail";
export type TakeDocument = { key: DocKey, windowId: string, 
/**
 * Defaults to `note`.
 */
policy?: DocPolicy, };
export type TaskDef = { id: string, command: string, cwd: string | null, env: { [key in string]: string }, target: RunTarget, dependsOn: Array<string>, worktree: string | null, secrets: Array<string>, };
export type TemplateItem = { name: string, path: string, };
export type TemplateProblem = "not_found" | "unknown_placeholder" | "invalid_content" | "invalid_target" | "invalid_title" | "invalid_date" | "invalid_time" | "output_too_large";
export type Templates = { items: Array<TemplateItem>, };
export type TerminalAttached = { streamId: number, cols: number, rows: number, };
export type TerminalChanged = { rev: string, };
export type TerminalError = { "code": "terminal.command_too_long", "detail": { limit_bytes: number, } } | { "code": "terminal.command_invalid" } | { "code": "terminal.invalid_cursor" } | { "code": "terminal.list_changed" } | { "code": "terminal.title_too_long", "detail": { limit: number, } } | { "code": "terminal.invalid_size", "detail": { cols: number, rows: number, } } | { "code": "terminal.stream_unavailable", "detail": Record<symbol, never> } | { "code": "terminal.not_found", "detail": { id: string, } } | { "code": "terminal.project_not_found", "detail": { id: string, } } | { "code": "terminal.cwd_invalid", "detail": { path: string, } } | { "code": "terminal.agent_owned" };
export type TerminalProfile = { name: string, command: string, };
export type TerminalProfiles = { items: Array<TerminalProfile>, };
export type TerminalSession = { id: string, projectId: string | null, title: string, cwd: string, kind: SessionKind, alive: boolean, command: string, createdMs: number, };
export type TextEdit = { fromUtf16: number, toUtf16: number, text: string, };
export type Tokens = { input: number, output: number, cacheRead: number, cacheWrite: number, };
export type ToolProfile = { id: string, label: string, command: string, hooks: Hooks, waitingPatterns: Array<string>, };
export type Trashed = { trashPath: string, };
export type TreeItem = { 
/**
 * Vault-relative `/` path.
 */
path: string, dir: boolean, title: string, };
export type TreeParams = { 
/**
 * `null` starts a new listing; otherwise the previous page's `nextCursor`.
 */
cursor: string | null, };
export type TrustApprove = { id: string, expectedCanonicalPath: string, expectedDigest: string, expectedScopeVersion: number, };
export type TrustOperation = "create" | "run" | "test" | "launch" | "discard" | "merge" | "pr" | "cleanup";
export type TrustPreview = { status: TrustPreviewStatus, projectId: string, projectName: string, canonicalPath: string, configPath: string, digest: string | null, scopeVersion: number, approved: boolean, requiresApproval: boolean, config: ProjectConfig | null, executionDirectories: Array<string>, runDefinitions: Array<RunDef>, };
export type TrustPreviewParams = { id: string, operation: TrustOperation, agentId: string | null, worktrees: Array<string>, worktree: boolean | null, };
export type TrustPreviewStatus = "not_required" | "approval_required" | "approved";
export type UndefineRun = { projectId: string, id: string, };
export type WindowsRun = { directory: string, notice: string, };
export type WriteJournal = { leaseId: string, seq: number, baseRev: DiskRev | null, text: string, };
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
  "agents.rebase_status": { kind: "query"; confirm: "none"; params: GetAgent; result: AgentRebaseStatus; error: AgentsError };
  "agents.resources": { kind: "query"; confirm: "none"; params: PingParams; result: AgentResources; error: AgentsError };
  "agents.resume": { kind: "mutation"; confirm: "none"; params: GetAgent; result: AgentTask; error: AgentsError };
  "agents.review": { kind: "mutation"; confirm: "none"; params: GetAgent; result: AgentTask; error: AgentsError };
  "agents.send": { kind: "mutation"; confirm: "none"; params: SendToAgent; result: AgentTask; error: AgentsError };
  "agents.test": { kind: "mutation"; confirm: "none"; params: GetAgent; result: AgentTest; error: AgentsError };
  "agents.usage": { kind: "query"; confirm: "none"; params: GetAgent; result: AgentUsage; error: AgentsError };
  "debug.echo": { kind: "stream"; confirm: "none"; params: EchoParams; result: StreamOpened; error: DebugError };
  "documents.claim": { kind: "mutation"; confirm: "none"; params: ClaimDocument; result: Claimed; error: DocumentsError };
  "documents.journal": { kind: "mutation"; confirm: "none"; params: WriteJournal; result: Done; error: DocumentsError };
  "documents.journal_drop": { kind: "mutation"; confirm: "none"; params: DropJournal; result: Done; error: DocumentsError };
  "documents.journal_read": { kind: "query"; confirm: "none"; params: LeaseRef; result: JournalText; error: DocumentsError };
  "documents.read": { kind: "query"; confirm: "none"; params: ReadDocument; result: DiskDocument; error: DocumentsError };
  "documents.release": { kind: "mutation"; confirm: "none"; params: LeaseRef; result: Done; error: DocumentsError };
  "documents.rename": { kind: "mutation"; confirm: "none"; params: RenameDocument; result: Renamed; error: DocumentsError };
  "documents.renew": { kind: "mutation"; confirm: "none"; params: RenewLeases; result: Renewed; error: DocumentsError };
  "documents.save": { kind: "mutation"; confirm: "none"; params: SaveDocument; result: Saved; error: DocumentsError };
  "documents.take": { kind: "mutation"; confirm: "none"; params: TakeDocument; result: Claimed; error: DocumentsError };
  "logs.open": { kind: "stream"; confirm: "none"; params: OpenLogs; result: StreamOpened; error: ObserveError };
  "logs.search": { kind: "query"; confirm: "none"; params: SearchLogs; result: SearchResult; error: ObserveError };
  "notes.attach": { kind: "mutation"; confirm: "none"; params: AttachImage; result: Attached; error: NotesError };
  "notes.backlinks": { kind: "query"; confirm: "none"; params: NotePath; result: Backlinks; error: NotesError };
  "notes.create": { kind: "mutation"; confirm: "none"; params: CreateNote; result: CreatedNote; error: NotesError };
  "notes.delete": { kind: "mutation"; confirm: "reversible"; params: NotePath; result: Trashed; error: NotesError };
  "notes.rename": { kind: "mutation"; confirm: "none"; params: RenameNote; result: RenameStarted; error: NotesError };
  "notes.rename_status": { kind: "query"; confirm: "none"; params: RenameOp; result: RenameStatus; error: NotesError };
  "notes.render": { kind: "query"; confirm: "none"; params: RenderNote; result: RenderedNote; error: NotesError };
  "notes.resolve": { kind: "query"; confirm: "none"; params: ResolveLink; result: ResolvedLink; error: NotesError };
  "notes.restore": { kind: "mutation"; confirm: "none"; params: RestoreNote; result: Done; error: NotesError };
  "notes.rewrite_reply": { kind: "mutation"; confirm: "none"; params: RewriteReply; result: Done; error: NotesError };
  "notes.tags": { kind: "query"; confirm: "none"; params: NoParams; result: Tags; error: NotesError };
  "notes.templates": { kind: "query"; confirm: "none"; params: NoParams; result: Templates; error: NotesError };
  "notes.tree": { kind: "query"; confirm: "none"; params: TreeParams; result: NoteTree; error: NotesError };
  "observe.kill": { kind: "mutation"; confirm: "always"; params: KillProcess; result: KillResult; error: ObserveError };
  "observe.ports": { kind: "query"; confirm: "none"; params: ObserveQuery; result: Array<PortInfo>; error: ObserveError };
  "observe.problems": { kind: "query"; confirm: "none"; params: ProblemsQuery; result: ProblemsResult; error: ObserveError };
  "observe.processes": { kind: "query"; confirm: "none"; params: ProcessesQuery; result: Array<ProcessInfo>; error: ObserveError };
  "observe.resolve_location": { kind: "query"; confirm: "none"; params: ResolveFileLocation; result: FileLocation; error: ObserveError };
  "observe.system": { kind: "query"; confirm: "none"; params: ObserveQuery; result: SystemInfo; error: ObserveError };
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
  "projects.trust_approve": { kind: "mutation"; confirm: "none"; params: TrustApprove; result: Done; error: ProjectsError };
  "projects.trust_preview": { kind: "query"; confirm: "none"; params: TrustPreviewParams; result: TrustPreview; error: ProjectsError };
  "runtime.container_action": { kind: "mutation"; confirm: "reversible"; params: ContainerActionParams; result: ContainerActionResult; error: ContainerError };
  "runtime.containers": { kind: "query"; confirm: "none"; params: ContainersQuery; result: ContainersResult; error: ContainerError };
  "runtime.define": { kind: "mutation"; confirm: "none"; params: DefineRun; result: DefineResult; error: RuntimeError };
  "runtime.detect": { kind: "query"; confirm: "none"; params: DetectRuns; result: Array<Suggestion>; error: RuntimeError };
  "runtime.exited": { kind: "mutation"; confirm: "always"; params: Exited; result: Done; error: RuntimeError };
  "runtime.get": { kind: "query"; confirm: "none"; params: RunId; result: RunDetail; error: RuntimeError };
  "runtime.list": { kind: "query"; confirm: "none"; params: ListRuns; result: DefinitionList; error: RuntimeError };
  "runtime.restart": { kind: "mutation"; confirm: "none"; params: RunId; result: Run; error: RuntimeError };
  "runtime.runs": { kind: "query"; confirm: "none"; params: RunsQuery; result: RunsResult; error: RuntimeError };
  "runtime.schedules": { kind: "query"; confirm: "none"; params: SchedulesQuery; result: SchedulesResult; error: RuntimeError };
  "runtime.start": { kind: "mutation"; confirm: "none"; params: StartRun; result: Run; error: RuntimeError };
  "runtime.stop": { kind: "mutation"; confirm: "reversible"; params: RunId; result: Run; error: RuntimeError };
  "runtime.summary": { kind: "query"; confirm: "none"; params: RunSummaryQuery; result: RunSummary; error: RuntimeError };
  "runtime.undefine": { kind: "mutation"; confirm: "always"; params: UndefineRun; result: Done; error: RuntimeError };
  "secrets.delete": { kind: "mutation"; confirm: "always"; params: Name; result: Done; error: SecretsError };
  "secrets.export_key": { kind: "query"; confirm: "none"; params: Empty; result: Recovery; error: SecretsError };
  "secrets.import_key": { kind: "mutation"; confirm: "none"; params: Recovery; result: Done; error: SecretsError };
  "secrets.list": { kind: "query"; confirm: "none"; params: Empty; result: Array<SecretInfo>; error: SecretsError };
  "secrets.reset": { kind: "mutation"; confirm: "always"; params: Empty; result: Done; error: SecretsError };
  "secrets.set": { kind: "mutation"; confirm: "none"; params: SetSecret; result: Done; error: SecretsError };
  "secrets.status": { kind: "query"; confirm: "none"; params: Empty; result: PublicKeyStatus; error: SecretsError };
  "system.doctor": { kind: "query"; confirm: "none"; params: PingParams; result: DoctorReport; error: SystemError };
  "system.info": { kind: "query"; confirm: "none"; params: PingParams; result: DaemonInfo; error: SystemError };
  "system.ping": { kind: "query"; confirm: "none"; params: PingParams; result: Pong; error: SystemError };
  "system.refresh_env": { kind: "mutation"; confirm: "none"; params: PingParams; result: RefreshEnvResult; error: SystemError };
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
  "documents.changed": DocumentChanged;
  "documents.lease_lost": LeaseLost;
  "documents.moved": DocumentMoved;
  "notes.changed": NotesChanged;
  "notes.rename_incomplete": RenameIncomplete;
  "notes.rewrite_request": RewriteRequest;
  "notify.raised": Notification;
  "observe.problems.changed": ProblemsChange;
  "projects.changed": ProjectsChanged;
  "runtime.changed": RuntimeChange;
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
  "agents.rebase_status": { kind: "query", confirm: "none" },
  "agents.resources": { kind: "query", confirm: "none" },
  "agents.resume": { kind: "mutation", confirm: "none" },
  "agents.review": { kind: "mutation", confirm: "none" },
  "agents.send": { kind: "mutation", confirm: "none" },
  "agents.test": { kind: "mutation", confirm: "none" },
  "agents.usage": { kind: "query", confirm: "none" },
  "debug.echo": { kind: "stream", confirm: "none" },
  "documents.claim": { kind: "mutation", confirm: "none" },
  "documents.journal": { kind: "mutation", confirm: "none" },
  "documents.journal_drop": { kind: "mutation", confirm: "none" },
  "documents.journal_read": { kind: "query", confirm: "none" },
  "documents.read": { kind: "query", confirm: "none" },
  "documents.release": { kind: "mutation", confirm: "none" },
  "documents.rename": { kind: "mutation", confirm: "none" },
  "documents.renew": { kind: "mutation", confirm: "none" },
  "documents.save": { kind: "mutation", confirm: "none" },
  "documents.take": { kind: "mutation", confirm: "none" },
  "logs.open": { kind: "stream", confirm: "none" },
  "logs.search": { kind: "query", confirm: "none" },
  "notes.attach": { kind: "mutation", confirm: "none" },
  "notes.backlinks": { kind: "query", confirm: "none" },
  "notes.create": { kind: "mutation", confirm: "none" },
  "notes.delete": { kind: "mutation", confirm: "reversible" },
  "notes.rename": { kind: "mutation", confirm: "none" },
  "notes.rename_status": { kind: "query", confirm: "none" },
  "notes.render": { kind: "query", confirm: "none" },
  "notes.resolve": { kind: "query", confirm: "none" },
  "notes.restore": { kind: "mutation", confirm: "none" },
  "notes.rewrite_reply": { kind: "mutation", confirm: "none" },
  "notes.tags": { kind: "query", confirm: "none" },
  "notes.templates": { kind: "query", confirm: "none" },
  "notes.tree": { kind: "query", confirm: "none" },
  "observe.kill": { kind: "mutation", confirm: "always" },
  "observe.ports": { kind: "query", confirm: "none" },
  "observe.problems": { kind: "query", confirm: "none" },
  "observe.processes": { kind: "query", confirm: "none" },
  "observe.resolve_location": { kind: "query", confirm: "none" },
  "observe.system": { kind: "query", confirm: "none" },
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
  "projects.trust_approve": { kind: "mutation", confirm: "none" },
  "projects.trust_preview": { kind: "query", confirm: "none" },
  "runtime.container_action": { kind: "mutation", confirm: "reversible" },
  "runtime.containers": { kind: "query", confirm: "none" },
  "runtime.define": { kind: "mutation", confirm: "none" },
  "runtime.detect": { kind: "query", confirm: "none" },
  "runtime.exited": { kind: "mutation", confirm: "always" },
  "runtime.get": { kind: "query", confirm: "none" },
  "runtime.list": { kind: "query", confirm: "none" },
  "runtime.restart": { kind: "mutation", confirm: "none" },
  "runtime.runs": { kind: "query", confirm: "none" },
  "runtime.schedules": { kind: "query", confirm: "none" },
  "runtime.start": { kind: "mutation", confirm: "none" },
  "runtime.stop": { kind: "mutation", confirm: "reversible" },
  "runtime.summary": { kind: "query", confirm: "none" },
  "runtime.undefine": { kind: "mutation", confirm: "always" },
  "secrets.delete": { kind: "mutation", confirm: "always" },
  "secrets.export_key": { kind: "query", confirm: "none" },
  "secrets.import_key": { kind: "mutation", confirm: "none" },
  "secrets.list": { kind: "query", confirm: "none" },
  "secrets.reset": { kind: "mutation", confirm: "always" },
  "secrets.set": { kind: "mutation", confirm: "none" },
  "secrets.status": { kind: "query", confirm: "none" },
  "system.doctor": { kind: "query", confirm: "none" },
  "system.info": { kind: "query", confirm: "none" },
  "system.ping": { kind: "query", confirm: "none" },
  "system.refresh_env": { kind: "mutation", confirm: "none" },
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
  "agents.delivery_unknown",
  "agents.discard_snapshot_unavailable",
  "agents.gh_missing",
  "agents.input_too_large",
  "agents.invalid_input",
  "agents.list_changed",
  "agents.not_found",
  "agents.not_repo",
  "agents.operation_in_progress",
  "agents.pr_title_too_long",
  "agents.profile_not_found",
  "agents.project_not_found",
  "agents.project_trust_required",
  "agents.resume_unavailable",
  "agents.review_unavailable",
  "agents.session_metadata",
  "agents.session_missing",
  "agents.slug_exhausted",
  "agents.squash_message_too_long",
  "agents.state_changed",
  "agents.teardown_command_invalid",
  "agents.teardown_command_too_long",
  "agents.title_invalid",
  "agents.unmerged_commits",
  "agents.unsafe_worktree",
  "agents.worktree_add_partial",
  "agents.worktree_dirty",
  "cancelled",
  "daemon_restarting",
  "debug.unsupported",
  "documents.conflict",
  "documents.exists",
  "documents.invalid_input",
  "documents.lease_lost",
  "documents.not_found",
  "documents.out_of_scope",
  "documents.stale_journal",
  "documents.too_large",
  "documents.unsupported",
  "hello_required",
  "internal",
  "invalid_params",
  "method_not_found",
  "not_found",
  "notes.busy",
  "notes.exists",
  "notes.invalid_image",
  "notes.invalid_input",
  "notes.invalid_name",
  "notes.invalid_template",
  "notes.not_found",
  "notes.out_of_vault",
  "notes.stale_cursor",
  "notes.target_unsaved",
  "notes.too_large",
  "observe.file_outside_roots",
  "observe.invalid_cursor",
  "observe.invalid_input",
  "observe.invalid_regex",
  "observe.limit_exceeded",
  "observe.permission_denied",
  "observe.project_not_found",
  "observe.run_not_found",
  "observe.source_unavailable",
  "observe.state_changed",
  "observe.stream_unsupported",
  "projects.already_exists",
  "projects.clone_failed",
  "projects.clone_invalid_url",
  "projects.clone_target_exists",
  "projects.configuration_invalid",
  "projects.in_use",
  "projects.launch_failed",
  "projects.launch_profile_not_found",
  "projects.not_directory",
  "projects.not_found",
  "projects.path_invalid",
  "projects.prompt_invalid_encoding",
  "projects.prompt_limit_exceeded",
  "projects.trust_context_invalid",
  "projects.trust_preview_stale",
  "projects.trust_required",
  "projects.unsafe_prompt_path",
  "runtime.container_cancelled",
  "runtime.container_failed",
  "runtime.container_invalid_id",
  "runtime.container_invalid_output",
  "runtime.container_output_limit",
  "runtime.container_timeout",
  "runtime.container_unavailable",
  "runtime.def_invalid",
  "runtime.dependency_failed",
  "runtime.detect_io",
  "runtime.detect_limit",
  "runtime.detect_parse",
  "runtime.invalid_cursor",
  "runtime.not_ready",
  "runtime.project_config_invalid",
  "runtime.project_not_found",
  "runtime.run_not_found",
  "runtime.secret_missing",
  "secrets.invalid_input",
  "secrets.key_mismatch",
  "secrets.missing",
  "secrets.unavailable",
  "secrets_locked",
  "system.env_refresh_failed",
  "system.unavailable",
  "terminal.agent_owned",
  "terminal.command_invalid",
  "terminal.command_too_long",
  "terminal.cwd_invalid",
  "terminal.invalid_cursor",
  "terminal.invalid_size",
  "terminal.list_changed",
  "terminal.not_found",
  "terminal.project_not_found",
  "terminal.stream_unavailable",
  "terminal.title_too_long",
] as const;
export type RpcErrorCode = (typeof RPC_ERROR_CODES)[number];
