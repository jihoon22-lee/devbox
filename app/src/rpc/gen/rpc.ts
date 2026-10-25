// 이 파일은 `pnpm gen`이 만든다. 직접 고치지 않는다.
/* eslint-disable */

export type ActivityChanged = { rev: string, };
export type ActivityError = { "code": "activity.invalid_rules", "detail": { problems: Array<InvalidRule>, } } | { "code": "activity.invalid_range" } | { "code": "activity.invalid_input" } | { "code": "activity.config_unavailable" };
export type ActivityStatus = { rev: string, state: StorageState, 
/**
 * Closed sessions waiting to be written.
 */
pending: number, 
/**
 * Lost before storage: retry-queue overflow plus the app's offline drops.
 */
dropped: number, lastError: StorageProblem | null, };
export type AddProject = { path: string, };
export type Advisory = { id: string, url: string, };
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
export type ApiError = { "code": "api.invalid_header", "detail": { index: number, field: string, reason: string, } } | { "code": "api.invalid_cookie", "detail": { index: number, reason: string, } } | { "code": "api.invalid_url", "detail": { reason: string, } } | { "code": "api.invalid_method" } | { "code": "api.invalid_body", "detail": { reason: string, } } | { "code": "api.invalid_input", "detail": { field: string, reason: string, } } | { "code": "api.variable_missing", "detail": { names: Array<string>, } } | { "code": "api.secret_missing", "detail": { names: Array<string>, } } | { "code": "secrets_locked" } | { "code": "secrets.key_mismatch" } | { "code": "api.file_missing", "detail": { part: number, } } | { "code": "api.too_large", "detail": { limit: number, } } | { "code": "api.redaction_failed", "detail": { reason: string, } } | { "code": "api.tls_invalid", "detail": { reason: string, } } | { "code": "api.oauth_failed", "detail": { stage: string, } } | { "code": "api.network", "detail": { kind: string, } } | { "code": "api.stream_unsupported" } | { "code": "api.stream_rejected", "detail": { status: number, reason: string, } } | { "code": "api.too_many_streams", "detail": { limit: number, } } | { "code": "api.history_not_found" } | { "code": "api.response_export_failed", "detail": { reason: string, } } | { "code": "api.collection_path_invalid" } | { "code": "api.collection_conflict", "detail": { digest: string | null, } } | { "code": "api.collection_too_large", "detail": { limit: number, } } | { "code": "api.collection_not_found" } | { "code": "api.storage_sensitive_body" } | { "code": "api.project_not_found" } | { "code": "api.grpc", "detail": { reason: string, } } | { "code": "api.review_required" } | { "code": "api.mcp", "detail": { reason: string, } };
export type AppMs = { app: string, ms: number, };
export type AppendFormat = "plain" | "capture";
export type AppendNote = { target: AppendTarget, text: string, format: AppendFormat, };
export type AppendResponseNote = { historyId: string, selection: string, operationId: string, };
export type AppendTarget = "inbox" | "daily";
export type Appended = { path: string, rev: DiskRev, };
export type AttachImage = { notePath: string, name: string, 
/**
 * Standard base64 of the image bytes.
 */
data: string, };
export type AttachSession = { id: string, cols: number, rows: number, readOnly: boolean, };
export type Attached = { path: string, markdown: string, };
export type Attention = { kind: string, message: string, atMs: number, };
export type AuditFailure = "network" | "http" | "invalidResponse" | "incomplete" | "limitExceeded" | "inputChanged";
export type AuditJob = { id: string, projectId: string, fingerprint: string, state: AuditState, completed: number, total: number, cached: boolean, error: AuditFailure | null, result: AuditReport | null, lastSuccess: AuditReport | null, };
export type AuditPackage = { ecosystem: Ecosystem, name: string, version: string, advisories: Array<Advisory>, };
export type AuditProgress = { id: string, projectId: string, state: AuditState, completed: number, total: number, };
export type AuditReport = { fingerprint: string, completedAtMs: number, packages: Array<AuditPackage>, skipped: number, };
export type AuditState = "running" | "succeeded" | "failed";
export type Auth = { "kind": "none" } | { "kind": "basic", username: string, password: string, } | { "kind": "bearer", token: string, } | { "kind": "apiKey", name: string, value: string, placement: KeyPlacement, } | { "kind": "oauth2ClientCredentials", tokenUrl: string, clientId: string, clientSecret: string, scope: string | null, } | { "kind": "oauth2AuthCode", authorizationUrl: string, tokenUrl: string, clientId: string, clientSecret: string | null, scope: string | null, };
export type Backlink = { src: string, title: string, 
/**
 * 1-based line of the link in `src`.
 */
line: number, fromUtf16: number, toUtf16: number, };
export type Backlinks = { rev: string, items: Array<Backlink>, truncated: boolean, };
export type BlameLine = { oid: string, originalLine: number, line: number, author: string, timeMs: number, text: string, };
export type Body = { "kind": "none" } | { "kind": "raw", text: string, encoding?: RawEncoding, contentType: string | null, } | { "kind": "json", text: string, } | { "kind": "form", fields: Array<Pair>, } | { "kind": "multipart", parts: Array<Part>, } | { "kind": "binary", path: string, } | { "kind": "graphql", query: string, variables: string, operationName: string | null, method: GraphqlMethod, };
export type Candidate = { path: string, name: string, isGit: boolean, registered: boolean, };
export type CatalogSource = "reflection" | "protoFiles";
export type ChangeStatus = "added" | "modified" | "deleted" | "renamed" | "unmerged";
export type Check = { id: string, status: Status, message: string, fix: string | null, };
export type CheckRef = { id: string, };
export type ClaimDocument = { key: DocKey, policy: DocPolicy, windowId: string, };
export type Claimed = { key: DocKey, leaseId: string | null, rev: DiskRev | null, text: string | null, eol: Eol, bom: boolean, readOnly: ReadOnly | null, journal: JournalView | null, };
export type CleanupAgents = { projectId: string, worktrees: Array<string>, branches: Array<string>, };
export type CleanupCandidate = { path: string | null, branch: string, oid: string, };
export type CleanupCandidates = { projectId: string, };
export type CleanupOptions = { worktrees: Array<string>, branches: Array<string>, };
export type CloneProject = { url: string, dir: string | null, };
export type CloseHow = "merged" | "pullRequest" | "discarded";
export type CloseSession = { id: string, };
export type CollectionExpect = { "kind": "digest", digest: string, } | { "kind": "absent" };
export type CollectionFile = { path: string, size: number, digest: string | null, 
/**
 * 프로젝트 파일이 비밀 참조·파일 부분·stdio 명령을 쓰면 이 PC에서 그 digest를 검토했을 때만 참.
 */
reviewed: boolean, 
/**
 * 권한 등으로 읽지 못한 파일. 목록 전체를 실패시키지 않고 이 항목만 표시한다.
 */
unreadable: boolean, };
export type CollectionList = { root: CollectionRoot, items: Array<CollectionFile>, 
/**
 * 루트당 2,000개를 넘으면 참.
 */
truncated: boolean, };
export type CollectionRoot = { "kind": "personal" } | { "kind": "project", projectId: string, };
export type CollectionsChange = { root: CollectionRoot, rev: string, };
export type CollectionsQuery = { root: CollectionRoot, };
export type Commit = { sha: string, subject: string, author: string, timeMs: number, };
export type CommonError = { "code": "internal" } | { "code": "invalid_params", "detail": { reason: string, } } | { "code": "method_not_found", "detail": { method: string, } } | { "code": "hello_required" } | { "code": "cancelled" } | { "code": "daemon_restarting" } | { "code": "not_found" } | { "code": "secrets_locked" };
export type ConflictApply = { target: SourceTarget, path: string, resolution: string | null, token: ConflictToken, };
export type ConflictEntry = { path: string, kind: ConflictKind, };
export type ConflictKind = "modifyModify" | "deleteModify" | "addAdd" | "other";
export type ConflictLabels = { base: string, ours: string, theirs: string, rebase: boolean, };
export type ConflictPreview = { base: string | null, ours: string | null, theirs: string | null, merged: string | null, labels: ConflictLabels, token: ConflictToken, textSupported: boolean, };
export type ConflictToken = { target: SourceTarget, path: string, worktreeHash: string, stageOids: [string | null, string | null, string | null], identity: string, };
export type ContainerAction = "start" | "stop" | "restart";
export type ContainerActionParams = { id: string, action: ContainerAction, };
export type ContainerActionResult = { id: string, action: ContainerAction, undo: ContainerActionParams | null, };
export type ContainerError = { "code": "runtime.container_invalid_id" } | { "code": "runtime.container_unavailable" } | { "code": "runtime.container_failed" } | { "code": "runtime.container_timeout" } | { "code": "runtime.container_output_limit" } | { "code": "runtime.container_invalid_output" } | { "code": "runtime.container_cancelled" };
export type ContainerInfo = { id: string, name: string, image: string, state: string, status: string, ports: string, composeProject?: string, };
export type ContainerUnavailableReason = "cliMissing" | "daemonUnavailable";
export type ContainersQuery = Record<symbol, never>;
export type ContainersResult = { available: boolean, reason?: ContainerUnavailableReason, items: Array<ContainerInfo>, };
export type CookieInfo = { name: string, value: string, domain: string, path: string, secure: boolean, httpOnly: boolean, expiresMs: number | null, };
export type CookieJarRef = { jar: string, };
export type CookieList = { cookies: Array<CookieInfo>, };
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
export type DailyNote = { path: string, created: boolean, };
export type DailyParams = { 
/**
 * `YYYY-MM-DD`; `null` is today (local).
 */
date: string | null, };
export type DayActivity = { rev: string, date: string, totalMs: number, apps: Array<AppMs>, projects: Array<ProjectMs>, 
/**
 * Top 50 non-empty titles.
 */
titles: Array<TitleMs>, 
/**
 * 24 local hours.
 */
hours: Array<HourMs>, };
export type DayQuery = { 
/**
 * Local date `YYYY-MM-DD`.
 */
date: string, };
export type DayTotal = { date: string, totalMs: number, };
export type DebugError = { "code": "debug.unsupported" };
export type DefineResult = { rev: string, definition: Definition, };
export type DefineRun = { projectId: string, def: RunDef, saveTo: SaveTo, };
export type Definition = { source: DefinitionSource, def: RunDef, };
export type DefinitionCursor = { id: string, shadowed: boolean, };
export type DefinitionKind = "task" | "service";
export type DefinitionList = { rev: string, defs: Array<Definition>, shadowed: Array<Definition>, runs: Array<Run>, trust: TrustPreviewStatus, nextCursor: DefinitionCursor | null, };
export type DefinitionSource = "projectFile" | "thisPc";
export type DeleteFile = { root: CollectionRoot, path: string, digest: string, };
export type DeleteSourceFile = { key: DocKey, expect: DiskRev, };
export type Dependency = { ecosystem: Ecosystem, name: string, versions: Array<DependencyVersion>, direct: boolean, };
export type DependencyInventory = { fingerprint: string, locks: Array<LockKind>, dependencies: Array<Dependency>, };
export type DependencySource = "registry" | "git" | "local" | "unknown";
export type DependencyVersion = { version: string, source: DependencySource, direct: boolean, };
export type DepsError = { "code": "deps.invalid_project" } | { "code": "deps.no_lockfile" } | { "code": "deps.unsupported_lock", "detail": { lock: LockKind, } } | { "code": "deps.malformed_lock", "detail": { lock: LockKind, } } | { "code": "deps.missing_manifest", "detail": { lock: LockKind, } } | { "code": "deps.limit_exceeded" } | { "code": "deps.unsafe_path" } | { "code": "deps.read_failed" } | { "code": "deps.busy" } | { "code": "deps.job_not_found" };
export type DetectRuns = { projectId: string, };
export type Diagnostic = "missing" | "ambiguous" | "limitExceeded" | "invalid" | "unsafePath" | "unavailable";
export type DiffSnapshot = { path: string, oldPath: string | null, staged: boolean, patch: string, hunks: Array<Hunk>, fileOnly: boolean, expect: FileExpect, };
export type Direction = "sent" | "received";
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
export type DocumentsError = { "code": "documents.conflict", "detail": { rev: DiskRev | null, } } | { "code": "documents.lease_lost", "detail": { reason: LeaseLoss, } } | { "code": "documents.out_of_scope" } | { "code": "documents.not_found" } | { "code": "documents.not_empty" } | { "code": "documents.exists" } | { "code": "documents.too_large", "detail": { limit: number, } } | { "code": "documents.stale_journal" } | { "code": "documents.unsupported" } | { "code": "documents.invalid_input" };
export type Done = { rev: string, };
export type DropJournal = { key: DocKey, uptoSeq: number, 
/**
 * The caller's lease. While another live lease holds the key the drop is refused, so a
 * read-only window can never remove the holder's journal.
 */
leaseId?: string, };
export type EchoParams = { bytes: number, };
export type Ecosystem = "npm" | "cargo" | "go" | "python";
export type Empty = Record<symbol, never>;
export type EntryInfo = { key: DocKey, kind: EntryKind, rev: DiskRev, readOnly: ReadOnly | null, };
export type EntryKind = "file" | "directory" | "symlink";
export type EnvSnapshot = { variables: Array<EnvVar>, };
export type EnvVar = { name: string, value: string, sensitive: boolean, origin: VarOrigin, };
export type Eol = "lf" | "crlf";
export type ExitRecord = { attemptId: string, code: number | null, signal?: string, endedMs: number, by: ExitSource, serviceResult?: string, };
export type ExitSource = "wrapper" | "systemd";
export type Exited = { runId: string, attemptId: string, };
export type Expect = "present" | "absent";
export type ExportFormat = "csv" | "json";
export type ExportQuery = { 
/**
 * Inclusive local dates, at most 366 days.
 */
from: string, to: string, format: ExportFormat, };
export type Exported = { path: string, };
export type FailedRewrite = { path: string, reason: RewriteFailure, };
export type FileChange = { path: string, oldPath: string | null, status: ChangeStatus, added: number, removed: number, };
export type FileEntry = { key: DocKey, name: string, kind: EntryKind, };
export type FileExpect = { fingerprint: string, };
export type FileKey = { key: DocKey, };
export type FileLocation = { path: string, line: number, column: number, };
export type FileRef = { root: CollectionRoot, path: string, };
export type FileText = { text: string, digest: string, reviewed: boolean, };
export type FileTree = { entries: Array<FileEntry>, nextCursor: FileTreeCursor | null, };
export type FileTreeCursor = { after: string, rev: string, };
export type FileTreeQuery = { projectId: string, dir: string, showHidden: boolean, cursor: FileTreeCursor | null, };
export type FilesError = { "code": "files.invalid_input" } | { "code": "files.out_of_scope" } | { "code": "files.not_found" } | { "code": "files.exists" } | { "code": "files.state_changed" } | { "code": "files.not_empty" } | { "code": "files.limit_exceeded" } | { "code": "files.editor_unavailable" };
export type FilesystemInfo = { totalBytes: number, availableBytes: number, usedBytes: number, };
export type FollowUp = { atMs: number, text: string, instructionId?: string, delivery?: InstructionDelivery, };
export type GetAgent = { id: string, };
export type GetAgentPatch = { id: string, path: string, };
export type GetProjectConfig = { id: string, };
export type GitBranchDeleteParams = { target: SourceTarget, branch: string, oid: string, force: boolean, expect: SourceExpect, };
export type GitChanged = { projectId: string, worktreeId: string | null, };
export type GitCheckoutParams = { target: SourceTarget, branch: string, create?: boolean, expect: SourceExpect, };
export type GitCleanupParams = { target: SourceTarget, candidates: Array<CleanupCandidate>, };
export type GitCommitParams = { target: SourceTarget, message: string, amend: boolean, expect: SourceExpect, };
export type GitConfigWarning = { key: string, };
export type GitDiffQuery = { target: SourceTarget, path: string, staged: boolean, };
export type GitFileChange = { target: SourceTarget, path: string, hunk?: string, expect: FileExpect, };
export type GitLogQuery = { target: SourceTarget, path?: string, limit: number, skip: number, };
export type GitOperation = { id: string, target: SourceTarget, state: GitOperationState, error: SourceGitError | null, current: RemoteExpect | null, remoteOid: string | null, completionUnknown: boolean, };
export type GitOperationQuery = { target: SourceTarget, operationId: string, };
export type GitOperationState = "running" | "succeeded" | "failed" | "cancelled";
export type GitPathQuery = { target: SourceTarget, path: string, };
export type GitPullParams = { target: SourceTarget, rebase: boolean, expect: RemoteExpect, };
export type GitPushParams = { target: SourceTarget, forceWithLease: boolean, expect: RemoteExpect, };
export type GitStashAction = { "action": "push", message: string, includeUntracked: boolean, } | { "action": "apply", oid: string, } | { "action": "pop", oid: string, };
export type GitStashDropParams = { target: SourceTarget, oid: string, expect: SourceExpect, };
export type GitStashParams = { target: SourceTarget, payload: GitStashAction, expect: SourceExpect, };
export type GitTargetQuery = { target: SourceTarget, };
export type GraphqlMethod = "post" | "get";
export type GrpcCall = { target: string, 
/**
 * `package.Service/Method`(목록의 `fullName`).
 */
method: string, 
/**
 * 요청 메시지 JSON 텍스트(변수·비밀 참조 가능).
 */
json: string, metadata: Array<HeaderRow>, tls: TlsOptions, env: EnvSnapshot, timeoutMs: number, protoFiles: Array<string> | null, origin: RequestOrigin | null, };
export type GrpcCatalog = { source: CatalogSource, services: Array<GrpcService>, };
export type GrpcEvent = { "type": "headers", metadata: Array<Pair>, } | { "type": "message", json: string, } | { "type": "trailers", code: number, message: string, metadata: Array<Pair>, } | { "type": "failed", error: WireError, };
export type GrpcMethod = { name: string, 
/**
 * `package.Service/Method`.
 */
fullName: string, clientStreaming: boolean, serverStreaming: boolean, 
/**
 * 입력 메시지 기본값(JSON 텍스트).
 */
inputDefault: string, };
export type GrpcService = { name: string, methods: Array<GrpcMethod>, };
export type GrpcTarget = { 
/**
 * `http://host:port`(평문 h2c) 또는 `https://host:port`(요청 TLS 설정). 변수·비밀 참조 가능.
 */
target: string, metadata: Array<HeaderRow>, tls: TlsOptions, env: EnvSnapshot, 
/**
 * 있으면 reflection 대신 이 `.proto` 파일들(WSL 절대 경로)을 protox로 해석한다.
 */
protoFiles: Array<string> | null, };
export type HeaderRow = { row: number, name: string, value: string, };
export type HistoryEntry = { id: string, sentAtMs: number, method: string, url: string, snapshot: RequestSnapshot, tlsVerify: boolean, runId: string | null, origin: RequestOrigin | null, outcome: HistoryOutcome, error: WireError | null, response: HistoryResponse | null, bodyBase64: string, bodyTruncated: boolean, };
export type HistoryId = { id: string, };
export type HistoryOutcome = "ok" | "failed" | "cancelled";
export type HistoryPage = { items: Array<HistorySummary>, next: string | null, };
export type HistoryQuery = { before: string | null, limit: number, origin: RequestOrigin | null, };
export type HistoryResponse = { status: number, reason: string, httpVersion: string, headers: Array<Pair>, redirects: Array<Redirect>, elapsedMs: number, sizeBytes: number, binary: boolean, };
export type HistorySummary = { id: string, sentAtMs: number, method: string, url: string, status: number | null, elapsedMs: number, sizeBytes: number, outcome: HistoryOutcome, runId: string | null, origin: RequestOrigin | null, };
export type HookSource = "claude" | "codex" | "setup" | "exit";
export type Hooks = "claude" | "codex" | "none";
export type HourMs = { 
/**
 * Local hour `0..=23`.
 */
hour: number, ms: number, };
export type Hunk = { id: string, patch: string, };
export type ImageProblem = "empty" | "too_large" | "unsupported" | "dimensions" | "encoding";
export type IncompleteReason = "failed" | "target_exists" | "source_missing" | "move_failed" | "unreadable";
export type Ingest = { 
/**
 * `null` outside the app's batch path: session ids are then random.
 */
batch: IngestBatch | null, 
/**
 * At most 1,000.
 */
events: Array<Observation>, 
/**
 * With `batch`: the run's cumulative count of observations its offline
 * buffer dropped (resends repeat it; the daemon adds only the increase).
 * Without: dropped since the previous call.
 */
dropped: number, };
export type IngestBatch = { runId: string, upto: number, };
export type Ingested = { 
/**
 * Events applied; `0` for a batch at or below the run's last `upto`.
 */
accepted: number, };
export type InotifyInfo = { maxUserInstances: number | null, maxUserWatches: number | null, ownedInstances: number, ownedWatches: number, usageComplete: boolean, };
export type InsertSummary = { 
/**
 * `YYYY-MM-DD`; `null` is today (local).
 */
date: string | null, };
export type InstallLsp = { projectId: string, language: Language, expectedCommand: string, };
export type InstallPlan = { "kind": "wsl", command: string, title: string, shellCommand: string, } | { "kind": "windows", package: WindowsPackage, } | { "kind": "manual", text: string, } | { "kind": "none" };
export type Installed = { terminalId: string, };
export type InstructionDelivery = "pending" | "delivered" | "unknown";
export type InvalidRule = { field: RuleField, 
/**
 * Zero-based position in the submitted list. `TooMany` reports the first
 * entry beyond the limit.
 */
index: number, problem: RuleProblem, };
export type JobQuery = { projectId: string, id: string, };
export type JournalText = { seq: number, baseRev: DiskRev | null, text: string, };
export type JournalView = { seq: number, baseRev: DiskRev | null, text: string | null, 
/**
 * True when `text` did not fit the response budget; read it with `documents.journal_read`.
 */
deferred: boolean, };
export type KeyPlacement = "header" | "query";
export type KillProcess = { pid: number, signal: KillSignal, expect: ProcessExpectation, };
export type KillResult = { pid: number, };
export type KillSignal = "term" | "kill";
export type Language = "rust" | "typescript" | "python" | "go" | "bash" | "yaml" | "toml" | "json";
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
export type LockKind = "pnpm" | "npm" | "yarn" | "cargo" | "go" | "poetry" | "uv";
export type LogFilter = { text?: string, regex?: string, streams?: Array<LogStream>, };
export type LogFrom = Tail | { offset: number, };
export type LogLine = { src: LogSource, ts: string, stream: LogStream, text: string, };
export type LogSource = { "kind": "run", runId: string, } | { "kind": "file", path: string, } | { "kind": "container", id: string, };
export type LogStream = "stdout" | "stderr" | "system";
export type LspError = { "code": "lsp.invalid_input" } | { "code": "lsp.project_not_found" } | { "code": "lsp.missing_server" } | { "code": "lsp.failed" } | { "code": "lsp.limit_exceeded" } | { "code": "lsp.lease_lost" } | { "code": "lsp.outside_project" } | { "code": "lsp.protocol" } | { "code": "lsp.stream_unavailable" } | { "code": "lsp.install_unavailable" };
export type McpConnect = { transport: McpTransport, env: EnvSnapshot, source: McpSource, };
export type McpEvent = { "type": "authorize", url: string, } | { "type": "ready", serverName: string, serverVersion: string, protocolVersion: string, capabilitiesJson: string, } | { "type": "result", id: string, json: string, } | { "type": "error", id: string, code: number, message: string, } | { "type": "failed", error: WireError, } | { "type": "closed" };
export type McpInput = { "type": "list", id: string, what: McpList, cursor: string | null, } | { "type": "call", id: string, name: string, argumentsJson: string, } | { "type": "read", id: string, uri: string, } | { "type": "getPrompt", id: string, name: string, argumentsJson: string, };
export type McpList = "tools" | "resources" | "prompts";
export type McpSource = { "kind": "draft" } | { "kind": "file", root: CollectionRoot, path: string, digest: string, };
export type McpTransport = { "kind": "stdio", command: string, args: Array<string>, cwd: string | null, env: Array<Pair>, } | { "kind": "http", url: string, headers: Array<HeaderRow>, tls: TlsOptions, };
export type MergeAgent = { id: string, strategy: MergeKind, message: string | null, expect: ReviewExpect & { baseOid: string }, };
export type MergeKind = "merge" | "squash" | "rebase";
export type MessageKind = "text" | "binary";
export type MissedPolicy = "skip" | "runOnce";
export type Name = { name: string, };
export type NoParams = Record<symbol, never>;
export type NoteContent = { path: string, text: string, rev: DiskRev, };
export type NoteHit = { path: string, title: string, snippet: string, rangesUtf16: Array<[number, number]>, };
export type NoteHits = { rev: string, items: Array<NoteHit>, truncated: boolean, };
export type NotePath = { path: string, };
export type NoteTree = { rev: string, items: Array<TreeItem>, nextCursor: string | null, truncated: boolean, };
export type NotesChanged = { rev: string, paths: Array<string>, };
export type NotesError = { "code": "notes.not_found" } | { "code": "notes.exists" } | { "code": "notes.out_of_vault" } | { "code": "notes.invalid_name" } | { "code": "notes.invalid_template", "detail": { problem: TemplateProblem, } } | { "code": "notes.invalid_image", "detail": { problem: ImageProblem, } } | { "code": "notes.invalid_input" } | { "code": "notes.response_unavailable", "detail": { reason: string, } } | { "code": "notes.response_target_changed" } | { "code": "notes.append_uncertain" } | { "code": "notes.append_operation_mismatch" } | { "code": "notes.busy" } | { "code": "notes.target_unsaved" } | { "code": "notes.stale_cursor" } | { "code": "notes.too_large", "detail": { limit: number, } } | { "code": "notes.open_unsaved" } | { "code": "notes.summary_broken" } | { "code": "notes.conflict" };
export type NotesQuery = { text: string, 
/**
 * `1..=50`.
 */
limit: number, };
export type Notification = { id: string, projectId: string, runId: string | null, task: string | null, reason: NotificationReason, };
export type NotificationReason = "startLimit" | "schedule_failed" | "schedule_trust_required";
export type Observation = { 
/**
 * Per `runId` observation number, from 1.
 */
seq: number, atMs: number, kind: ObservationKind, app: string | null, 
/**
 * Already filtered by the app's privacy rules; the daemon applies them again.
 */
title: string | null, 
/**
 * Epoch ms of the latest user input.
 */
lastInputMs: number, };
export type ObservationKind = "foreground" | "input" | "idle" | "pause";
export type ObserveError = { "code": "observe.invalid_input" } | { "code": "observe.state_changed" } | { "code": "observe.permission_denied" } | { "code": "observe.project_not_found" } | { "code": "observe.invalid_regex" } | { "code": "observe.file_outside_roots" } | { "code": "observe.source_unavailable" } | { "code": "observe.run_not_found" } | { "code": "observe.invalid_cursor" } | { "code": "observe.limit_exceeded" } | { "code": "observe.stream_unsupported" };
export type ObserveQuery = Record<symbol, never>;
export type OpenLogs = { sources: Array<LogSource>, from: LogFrom, filter?: LogFilter, };
export type OpenLsp = { projectId: string, language: Language, };
export type OpenSourceEditor = { key: DocKey, editor: SourceEditor, };
export type Pair = { name: string, value: string, };
export type Part = { "kind": "text", name: string, value: string, contentType: string | null, } | { "kind": "file", name: string, path: string, fileName: string | null, contentType: string | null, };
export type PingParams = Record<symbol, never>;
export type Pong = { daemonId: string, version: string, uptimeMs: number, };
export type PortInfo = { port: number, proto: string, addr: string, pid: number | null, process: string | null, startTimeMs: number | null, runId: string | null, containerId: string | null, projectId: string | null, };
export type PrAgent = { id: string, title: string | null, body: string | null, expect: ReviewExpect & { baseOid: string }, };
export type Pressure = { some: PressureLine | null, full: PressureLine | null, };
export type PressureLine = { avg10: number, avg60: number, avg300: number, totalUs: number, };
export type PreviewFile = { key: DocKey, text?: string, };
export type PrivacyRules = { 
/**
 * Exact process names (case-insensitive). A match drops the session.
 */
excludedProcesses: Array<string>, 
/**
 * A match keeps the session but stores an empty title.
 */
excludedTitlePatterns: Array<string>, 
/**
 * Every match is replaced with `[redacted]`.
 */
redactTitlePatterns: Array<string>, 
/**
 * Never store any title.
 */
maskAllTitles: boolean, };
export type Problem = { file: string, line: number, col: number | null, severity: ProblemSeverity, message: string, source: string, };
export type ProblemSeverity = "error" | "warning" | "info";
export type ProblemsChange = { rev: string, projectId: string, };
export type ProblemsCursor = { rev: string, offset: number, };
export type ProblemsQuery = { projectId: string, limit?: number, cursor?: ProblemsCursor, };
export type ProblemsResult = { rev: string, items: Array<Problem>, nextCursor: ProblemsCursor | null, truncated: boolean, };
export type ProcessExpectation = { startTimeMs: number, };
export type ProcessInfo = { pid: number, name: string, uid: number, cwd: string | null, startTimeMs: number, cpuPercent: number, rssBytes: number, unit: string | null, runId: string | null, projectId: string | null, };
export type ProcessesQuery = { projectId: string | null, };
export type Project = { id: string, name: string, path: string, favorite: boolean, addedMs: number, gitWarnings?: Array<GitConfigWarning>, };
export type ProjectAgentCounts = { open: number, attention: number, };
export type ProjectConfig = { agent: AgentConfig, launch: LaunchConfig, };
export type ProjectConfigIssue = { message: string, };
export type ProjectConfigResult = { config: ProjectConfig, error?: ProjectConfigIssue, };
export type ProjectList = { rev: string, items: Array<Project>, };
export type ProjectMs = { projectId: string, ms: number, };
export type ProjectPrompt = { name: string, body: string, };
export type ProjectPrompts = { id: string, };
export type ProjectQuery = { projectId: string, };
export type ProjectSummary = { id: string, branch?: string, dirty: boolean | null, ahead: number | null, behind: number | null, terminals: number, agents: ProjectAgentCounts, error?: ProjectSummaryIssue, };
export type ProjectSummaryIssue = { message: string, };
export type ProjectSummaryList = { items: Array<ProjectSummary>, };
export type ProjectsChanged = { rev: string, };
export type ProjectsError = { "code": "projects.path_invalid", "detail": { path: string, } } | { "code": "projects.not_directory", "detail": { path: string, } } | { "code": "projects.already_exists", "detail": { path: string, } } | { "code": "projects.not_found", "detail": { id: string, } } | { "code": "projects.in_use", "detail": { id: string, } } | { "code": "projects.configuration_invalid", "detail": { message: string, } } | { "code": "projects.launch_profile_not_found", "detail": { id: string, } } | { "code": "projects.launch_failed", "detail": { terminals: number, agents: number, services: number, run_ids: Array<string>, message: string, } } | { "code": "projects.clone_invalid_url" } | { "code": "projects.clone_target_exists", "detail": { path: string, } } | { "code": "projects.clone_failed", "detail": { path: string, message: string, } } | { "code": "projects.unsafe_prompt_path", "detail": { path: string, } } | { "code": "projects.prompt_limit_exceeded", "detail": { max_files: number, max_bytes: number, } } | { "code": "projects.prompt_invalid_encoding", "detail": { name: string, } } | { "code": "projects.trust_required", "detail": { id: string, } } | { "code": "projects.trust_preview_stale", "detail": { id: string, } } | { "code": "projects.trust_context_invalid" };
export type PromptList = { items: Array<ProjectPrompt>, };
export type PublicKeyStatus = { locked: boolean, keyMismatch: boolean, };
export type RangeActivity = { rev: string, days: Array<DayTotal>, apps: Array<AppMs>, projects: Array<ProjectMs>, };
export type RangeQuery = { 
/**
 * Inclusive local dates, at most 31 days.
 */
from: string, to: string, };
export type RawEncoding = "base64";
export type ReadDocument = { key: DocKey, 
/**
 * Defaults to `note`.
 */
policy?: DocPolicy, };
export type ReadOnly = { reason: ReadOnlyReason, };
export type ReadOnlyReason = "binary" | "encoding" | "too_large" | "leased_elsewhere";
export type RealtimeEvent = { "type": "open", protocol: string | null, } | { "type": "message", direction: Direction, kind: MessageKind, data: string | null, dataBase64: string | null, atMs: number, } | { "type": "sseEvent", id: string | null, event: string | null, data: string, retryMs: number | null, atMs: number, } | { "type": "reconnecting", attempt: number, lastEventId: string | null, } | { "type": "dropped", count: number, } | { "type": "closed", code: number | null, reason: string | null, } | { "type": "failed", error: WireError, };
export type RebaseResult = "done" | "conflict";
export type Recovery = { recovery: string, };
export type Redirect = { status: number, location: string, };
export type RefreshEnvResult = { restarting: boolean, };
export type RelayMessage = { json: string, leaseId?: string, };
export type RemoteExpect = { branch: string | null, headOid: string | null, upstream: Upstream | null, upstreamOid: string | null, };
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
export type RequestOrigin = { root: CollectionRoot, path: string, };
export type RequestSnapshot = { method: string, url: string, query: Array<Pair>, headers: Array<HeaderRow>, cookies: Array<HeaderRow>, body: Body, auth: Auth, tls: TlsOptions, timeoutMs: number, followRedirects: boolean, };
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
export type ResponseNoteAppended = { path: string, rev: DiskRev, operationId: string, };
export type ResponseNoteSelection = { selection: string, path: string, target: ResponseNoteTarget, };
export type ResponseNoteTarget = { "kind": "daily" } | { "kind": "existing", path: string, };
export type ResponseRule = { id: string, priority: number, method: string | null, path: string, status: number, headers: Array<[string, string]>, body: string, delayMs: number, sequence?: Array<ResponseSequenceStep>, };
export type ResponseSequenceStep = { status: number, headers: Array<[string, string]>, body: string, delayMs: number, };
export type RestartPolicy = "no" | "onFailure" | "always";
export type Restarted = { projectId: string, language: Language, failed: boolean, generation: number, };
export type RestoreNote = { trashPath: string, path: string, };
export type ReviewExpect = { headOid: string, baseOid?: string, unmergedCommits?: number, discardFingerprint?: string, };
export type ReviewFile = { root: CollectionRoot, path: string, digest: string, };
export type RewriteFailure = "conflict" | "open" | "missing" | "unsupported" | "too_large";
export type RewriteReply = { opId: string, key: DocKey, 
/**
 * True after the window applied the edits and saved the note.
 */
applied: boolean, };
export type RewriteRequest = { opId: string, key: DocKey, leaseId: string, journalSeq: number, edits: Array<TextEdit>, };
export type RuleField = "excludedProcesses" | "excludedTitlePatterns" | "redactTitlePatterns";
export type RuleProblem = "empty" | "too_long" | "too_many" | "syntax";
export type RulesRev = { rev: string, };
export type RulesView = { rev: string, rules: PrivacyRules, idleThresholdMs: number, };
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
export type RuntimeError = { "code": "runtime.def_invalid", "detail": { id: string, reason: string, } } | { "code": "runtime.project_not_found", "detail": { id: string, } } | { "code": "projects.trust_required", "detail": { id: string, } } | { "code": "projects.trust_preview_stale", "detail": { id: string, } } | { "code": "runtime.project_config_invalid", "detail": { message: string, } } | { "code": "runtime.run_not_found", "detail": { id: string, } } | { "code": "runtime.not_ready", "detail": { id: string, } } | { "code": "runtime.dependency_failed", "detail": { id: string, } } | { "code": "runtime.detect_parse", "detail": { file: string, message: string, } } | { "code": "runtime.detect_limit", "detail": { subject: string, limit: number, } } | { "code": "runtime.detect_io", "detail": { file: string, message: string, } } | { "code": "runtime.invalid_cursor" } | { "code": "runtime.windows_copy_busy" } | { "code": "runtime.windows_copy_unavailable" } | { "code": "secrets_locked" } | { "code": "secrets.key_mismatch" } | { "code": "runtime.secret_missing", "detail": { names: Array<string>, } };
export type SaveDocument = { leaseId: string, seq: number, baseRev: DiskRev | null, expect: Expect, text: string, };
export type SaveSearch = { name: string, query: SearchQuery, };
export type SaveTo = "project" | "thisPc";
export type Saved = { rev: DiskRev, };
export type SavedId = { id: string, };
export type SavedRef = { id: string, rev: string, };
export type SavedSearch = { id: string, name: string, query: SearchQuery, };
export type SavedSearches = { rev: string, items: Array<SavedSearch>, };
export type ScanProjects = Record<symbol, never>;
export type ScanResult = { candidates: Array<Candidate>, };
export type ScheduleDef = { id: string, task: string, cron: string, tz: string | null, missed: MissedPolicy, };
export type ScheduleLast = { at: string, result: ScheduleOutcome, };
export type ScheduleOutcome = "claimed" | "running" | "succeeded" | "failed" | "stopped" | "skippedMissed" | "skippedTrust";
export type ScheduleStatus = { id: string, cron: string, next: string | null, last: ScheduleLast | null, };
export type SchedulesQuery = { projectId: string, };
export type SchedulesResult = { items: Array<ScheduleStatus>, };
export type Scope = "notes" | "files" | "code";
export type SearchChanged = { rev: string, };
export type SearchChunk = { scope: Scope, 
/**
 * Search index revision when the query started.
 */
rev: string, items: Array<SearchHit>, 
/**
 * The last chunk of the stream.
 */
done: boolean, 
/**
 * This scope stopped at `limit` (or a size cap).
 */
truncated: boolean, };
export type SearchCursor = { sources: Array<SourceCursor>, };
export type SearchError = { "code": "search.invalid_regex", "detail": { position: number, } } | { "code": "search.invalid_input" } | { "code": "search.project_not_found" } | { "code": "search.not_found" } | { "code": "search.stream_unsupported" };
export type SearchHit = { root: string, path: string, 
/**
 * Note title (notes scope).
 */
title: string | null, 
/**
 * 1-based line (code scope).
 */
line: number | null, 
/**
 * A single-line excerpt (notes, code) or the path (files).
 */
snippet: string, 
/**
 * `[from, to)` UTF-16 offsets into `snippet`.
 */
rangesUtf16: Array<[number, number]>, };
export type SearchLogs = { sources: Array<LogSource>, query: string, regex: boolean, before?: SearchCursor, limit: number, };
export type SearchQuery = { text: string, 
/**
 * Results always arrive notes → files → code, whatever the order here.
 */
scopes: Array<Scope>, 
/**
 * `null`: every registered project (code and file names).
 */
projectIds: Array<string> | null, regex: boolean, caseSensitive: boolean, 
/**
 * Per scope, `1..=500`.
 */
limit: number, };
export type SearchResult = { items: Array<LogLine>, nextCursor: SearchCursor | null, };
export type SecretInfo = { name: string, hasValue: boolean, };
export type SecretsError = { "code": "secrets_locked" } | { "code": "secrets.key_mismatch" } | { "code": "secrets.invalid_input" } | { "code": "secrets.missing", "detail": { names: Array<string>, } } | { "code": "secrets.unavailable" };
export type SelectResponseNote = { target: ResponseNoteTarget, };
export type SendEvent = { "type": "started", historyId: string, sentAtMs: number, } | { "type": "authorize", url: string, } | { "type": "head", status: number, reason: string, httpVersion: string, headers: Array<Pair>, elapsedMs: number, redirects: Array<Redirect>, } | { "type": "body", dataBase64: string, } | { "type": "end", sizeBytes: number, elapsedMs: number, binary: boolean, historyTruncated: boolean, } | { "type": "failed", error: WireError, };
export type SendRequest = { sourceRoot?: CollectionRoot, snapshot: RequestSnapshot, env: EnvSnapshot, runId: string | null, origin: RequestOrigin | null, cookieJar: string | null, };
export type SendToAgent = { id: string, text: string, };
export type ServerInfo = { language: Language, command: string, args: Array<string>, install: string | null, state: ServerState, generation: number, pid: number | null, memoryBytes: number | null, clients: number, };
export type ServerState = "missing" | "starting" | "ready" | "failed" | "idle_stopped";
export type ServersQuery = { projectId: string, };
export type ServersResult = { items: Array<ServerInfo>, inotify: InotifyInfo, };
export type ServiceDef = { id: string, command: string, cwd: string | null, env: { [key in string]: string }, target: RunTarget, dependsOn: Array<string>, worktree: string | null, secrets: Array<string>, restart: RestartPolicy, restartSec: number, startLimit: StartLimit, portsHint: Array<number>, readyTimeoutSec: number, };
export type SessionKind = "shell" | "agent";
export type SessionList = { rev: string, items: Array<TerminalSession>, nextAfterId?: string, };
export type SetFavorite = { id: string, favorite: boolean, };
export type SetRules = { rules: PrivacyRules, 
/**
 * `60_000..=86_400_000`.
 */
idleThresholdMs: number, };
export type SetSecret = { name: string, value: string, };
export type SourceBranch = { name: string, oid: string, current: boolean, remote: boolean, upstream: string | null, };
export type SourceCursor = { source: LogSource, identity: string, stream: LogStream, offset: number, };
export type SourceEditor = "code" | "cursor" | "zed";
export type SourceEditorOpened = { opened: boolean, path: string, };
export type SourceExpect = { state: RemoteExpect, indexHash: string, worktreeHash: string, };
export type SourceFile = { path: string, oldPath: string | null, indexStatus: string, worktreeStatus: string, conflicted: boolean, };
export type SourceGitError = { "code": "git.conflict_stale", "detail": { target: SourceTarget, path: string, } } | { "code": "git.conflict_partial", "detail": { target: SourceTarget, path: string, } } | { "code": "git.invalid_target" } | { "code": "git.source_invalid_input" } | { "code": "git.source_failed" } | { "code": "git.source_limit_exceeded" } | { "code": "git.source_timed_out" } | { "code": "git.state_changed", "detail": { current: RemoteExpect, } } | { "code": "git.source_busy" } | { "code": "git.unsafe_source" };
export type SourcePreview = { "kind": "markdown", rendered: RenderedNote, } | { "kind": "svg", text: string, };
export type SourceStatus = { expect: SourceExpect, summary: StatusSummary, files: Array<SourceFile>, warnings: Array<GitConfigWarning>, };
export type SourceTarget = { projectId: string, worktreeId?: string, };
export type SourceWorktree = { path: string, headOid: string | null, branch: string | null, locked: boolean, prunable: boolean, main: boolean, };
export type SseOpen = { 
/**
 * `http://`·`https://` 절대 URL.
 */
url: string, headers: Array<HeaderRow>, tls: TlsOptions, env: EnvSnapshot, 
/**
 * 첫 연결부터 보낼 `Last-Event-ID`(재연결은 마지막으로 받은 id를 쓴다).
 */
lastEventId: string | null, origin: RequestOrigin | null, };
export type StartLimit = { burst: number, intervalSec: number, };
export type StartRun = { projectId: string, id: string, worktree: string | null, };
export type StashEntry = { oid: string, reference: string, subject: string, };
export type Status = "ok" | "warn" | "fail";
export type StatusSummary = { branch: string | null, ahead: number, behind: number, staged: number, unstaged: number, untracked: number, conflicted: Array<string>, clean: boolean, };
export type StorageProblem = "disk_full" | "database";
export type StorageState = "ok" | "storage_failed";
export type StreamOpened = { streamId: number, };
export type Suggestion = { id: string, command: string, kind: DefinitionKind, from: SuggestionSource, };
export type SuggestionSource = "package.json" | "Makefile" | "justfile" | "Cargo.toml" | "compose.yaml" | "docker-compose.yml";
export type SummaryInserted = { path: string, rev: DiskRev, };
export type SummaryProjects = Record<symbol, never>;
export type SupportBundle = { path: string, sizeBytes: number, logsOmitted?: string, };
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
export type TitleMs = { app: string, title: string, ms: number, };
export type TlsOptions = { verify: boolean, caPath: string | null, clientCertPath: string | null, clientKeyPath: string | null, };
export type Tokens = { input: number, output: number, cacheRead: number, cacheWrite: number, };
export type ToolProfile = { id: string, label: string, command: string, hooks: Hooks, waitingPatterns: Array<string>, };
export type ToolsDiagnosticsInfo = { logDir: string, };
export type ToolsEmpty = Record<symbol, never>;
export type ToolsError = { "code": "tools.invalid_input" } | { "code": "tools.support_failed" } | { "code": "tools.support_too_large" };
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
export type Upstream = { remote: string, destinationRef: string, };
export type VarOrigin = "environment" | "capture";
export type WebhookEmpty = Record<symbol, never>;
export type WebhookFixture = { id: string, name: string, request: WebhookReceivedDetail, };
export type WebhookHistoryQuery = { listenerId: string, before: string | null, limit: number, };
export type WebhookListener = { id: string, port: number, lan: boolean, addresses: Array<string>, running: boolean, };
export type WebhookListenerId = { id: string, };
export type WebhookListenerList = { items: Array<WebhookListener>, };
export type WebhookReceived = { listenerId: string, id: string, method: string, path: string, sizeBytes: number, atMs: number, truncated: boolean, };
export type WebhookReceivedDetail = { received: WebhookReceived, query: string, headers: Array<[string, string]>, bodyBase64: string, previewTruncated: boolean, };
export type WebhookReceivedId = { id: string, };
export type WebhookReceivedPage = { items: Array<WebhookReceived>, };
export type WebhookRuleList = { items: Array<ResponseRule>, };
export type WebhookRuleRef = { listenerId: string, id: string, };
export type WebhookSaveFixture = { receivedId: string, name: string, };
export type WebhookStartListener = { port: number, lan: boolean, };
export type WebhookUpsertRule = { listenerId: string, rule: ResponseRule, };
export type WebhooksError = { "code": "webhooks.invalid_input" } | { "code": "webhooks.not_found" } | { "code": "webhooks.bind_failed" } | { "code": "webhooks.limit_reached" } | { "code": "webhooks.storage" };
export type WindowsPackage = "dockerDesktop";
export type WindowsRun = { directory: string, notice: string, };
export type WireError = { code: string, detail?: unknown, };
export type WorkspaceBaseline = { uri: string, diskRev: DiskRev | null, textHash: string, leaseId?: string, version?: number, };
export type WorkspaceIdentity = { token: string, requestId: string, rootUri: string, generation: string, };
export type WorkspaceProof = { token: string, requestId: string, rootUri: string, generation: string, targets: Array<WorkspaceBaseline>, };
export type WorkspaceValidation = { targets: Array<WorkspaceBaseline>, token: string, requestId: string, rootUri: string, generation: string, };
export type WriteFile = { root: CollectionRoot, path: string, text: string, expect: CollectionExpect, };
export type WriteJournal = { leaseId: string, seq: number, baseRev: DiskRev | null, text: string, };
export type Written = { digest: string, rev: string, };
export type WsInput = { "type": "text", data: string, } | { "type": "binary", dataBase64: string, } | { "type": "close", code: number | null, reason: string | null, };
export type WsOpen = { 
/**
 * `ws://`·`wss://` 절대 URL(변수·비밀 참조 가능).
 */
url: string, headers: Array<HeaderRow>, 
/**
 * `Sec-WebSocket-Protocol` 후보(RFC 9110 token, 치환하지 않음).
 */
subprotocols: Array<string>, tls: TlsOptions, env: EnvSnapshot, origin: RequestOrigin | null, };
export type ZellijSessions = { items: Array<string>, };

export interface Methods {
  "activity.day": { kind: "query"; confirm: "none"; params: DayQuery; result: DayActivity; error: ActivityError };
  "activity.export": { kind: "mutation"; confirm: "none"; params: ExportQuery; result: Exported; error: ActivityError };
  "activity.ingest": { kind: "mutation"; confirm: "none"; params: Ingest; result: Ingested; error: ActivityError };
  "activity.range": { kind: "query"; confirm: "none"; params: RangeQuery; result: RangeActivity; error: ActivityError };
  "activity.rules": { kind: "query"; confirm: "none"; params: NoParams; result: RulesView; error: ActivityError };
  "activity.set_rules": { kind: "mutation"; confirm: "none"; params: SetRules; result: RulesRev; error: ActivityError };
  "activity.status": { kind: "query"; confirm: "none"; params: NoParams; result: ActivityStatus; error: ActivityError };
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
  "api.collection_delete": { kind: "mutation"; confirm: "always"; params: DeleteFile; result: Done; error: ApiError };
  "api.collection_read": { kind: "query"; confirm: "none"; params: FileRef; result: FileText; error: ApiError };
  "api.collection_review": { kind: "mutation"; confirm: "none"; params: ReviewFile; result: Done; error: ApiError };
  "api.collection_write": { kind: "mutation"; confirm: "none"; params: WriteFile; result: Written; error: ApiError };
  "api.collections": { kind: "query"; confirm: "none"; params: CollectionsQuery; result: CollectionList; error: ApiError };
  "api.cookies": { kind: "query"; confirm: "none"; params: CookieJarRef; result: CookieList; error: ApiError };
  "api.cookies_clear": { kind: "mutation"; confirm: "always"; params: CookieJarRef; result: Done; error: ApiError };
  "api.grpc_call": { kind: "stream"; confirm: "none"; params: GrpcCall; result: StreamOpened; error: ApiError };
  "api.grpc_describe": { kind: "query"; confirm: "none"; params: GrpcTarget; result: GrpcCatalog; error: ApiError };
  "api.history": { kind: "query"; confirm: "none"; params: HistoryQuery; result: HistoryPage; error: ApiError };
  "api.history_get": { kind: "query"; confirm: "none"; params: HistoryId; result: HistoryEntry; error: ApiError };
  "api.mcp_connect": { kind: "stream"; confirm: "none"; params: McpConnect; result: StreamOpened; error: ApiError };
  "api.send": { kind: "stream"; confirm: "none"; params: SendRequest; result: StreamOpened; error: ApiError };
  "api.sse_open": { kind: "stream"; confirm: "none"; params: SseOpen; result: StreamOpened; error: ApiError };
  "api.ws_open": { kind: "stream"; confirm: "none"; params: WsOpen; result: StreamOpened; error: ApiError };
  "debug.echo": { kind: "stream"; confirm: "none"; params: EchoParams; result: StreamOpened; error: DebugError };
  "deps.audit": { kind: "mutation"; confirm: "none"; params: ProjectQuery; result: AuditJob; error: DepsError };
  "deps.audit_status": { kind: "query"; confirm: "none"; params: JobQuery; result: AuditJob; error: DepsError };
  "deps.list": { kind: "query"; confirm: "none"; params: ProjectQuery; result: DependencyInventory; error: DepsError };
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
  "files.create": { kind: "mutation"; confirm: "none"; params: FileKey; result: EntryInfo; error: FilesError };
  "files.delete": { kind: "mutation"; confirm: "always"; params: DeleteSourceFile; result: Done; error: FilesError };
  "files.external_editor": { kind: "mutation"; confirm: "none"; params: OpenSourceEditor; result: SourceEditorOpened; error: FilesError };
  "files.preview": { kind: "query"; confirm: "none"; params: PreviewFile; result: SourcePreview; error: FilesError };
  "files.stat": { kind: "query"; confirm: "none"; params: FileKey; result: EntryInfo; error: FilesError };
  "files.tree": { kind: "query"; confirm: "none"; params: FileTreeQuery; result: FileTree; error: FilesError };
  "git.blame": { kind: "query"; confirm: "none"; params: GitPathQuery; result: Array<BlameLine>; error: SourceGitError };
  "git.branch_delete": { kind: "mutation"; confirm: "always"; params: GitBranchDeleteParams; result: Done; error: SourceGitError };
  "git.branches": { kind: "query"; confirm: "none"; params: GitTargetQuery; result: Array<SourceBranch>; error: SourceGitError };
  "git.checkout": { kind: "mutation"; confirm: "none"; params: GitCheckoutParams; result: Done; error: SourceGitError };
  "git.cleanup": { kind: "mutation"; confirm: "always"; params: GitCleanupParams; result: Done; error: SourceGitError };
  "git.cleanup_candidates": { kind: "query"; confirm: "none"; params: GitTargetQuery; result: Array<CleanupCandidate>; error: SourceGitError };
  "git.commit": { kind: "mutation"; confirm: "none"; params: GitCommitParams; result: Done; error: SourceGitError };
  "git.conflict_apply": { kind: "mutation"; confirm: "reversible"; params: ConflictApply; result: Done; error: SourceGitError };
  "git.conflict_preview": { kind: "query"; confirm: "none"; params: GitPathQuery; result: ConflictPreview; error: SourceGitError };
  "git.conflicts": { kind: "query"; confirm: "none"; params: GitTargetQuery; result: Array<ConflictEntry>; error: SourceGitError };
  "git.diff": { kind: "query"; confirm: "none"; params: GitDiffQuery; result: DiffSnapshot; error: SourceGitError };
  "git.discard": { kind: "mutation"; confirm: "always"; params: GitFileChange; result: Done; error: SourceGitError };
  "git.fetch": { kind: "mutation"; confirm: "none"; params: GitTargetQuery; result: GitOperation; error: SourceGitError };
  "git.log": { kind: "query"; confirm: "none"; params: GitLogQuery; result: Array<Commit>; error: SourceGitError };
  "git.operation_cancel": { kind: "mutation"; confirm: "none"; params: GitOperationQuery; result: GitOperation; error: SourceGitError };
  "git.operation_status": { kind: "query"; confirm: "none"; params: GitOperationQuery; result: GitOperation; error: SourceGitError };
  "git.pull": { kind: "mutation"; confirm: "always"; params: GitPullParams; result: GitOperation; error: SourceGitError };
  "git.push": { kind: "mutation"; confirm: "always"; params: GitPushParams; result: GitOperation; error: SourceGitError };
  "git.stage": { kind: "mutation"; confirm: "reversible"; params: GitFileChange; result: Done; error: SourceGitError };
  "git.stash": { kind: "mutation"; confirm: "none"; params: GitStashParams; result: Done; error: SourceGitError };
  "git.stash_drop": { kind: "mutation"; confirm: "always"; params: GitStashDropParams; result: Done; error: SourceGitError };
  "git.stash_list": { kind: "query"; confirm: "none"; params: GitTargetQuery; result: Array<StashEntry>; error: SourceGitError };
  "git.status": { kind: "query"; confirm: "none"; params: GitTargetQuery; result: SourceStatus; error: SourceGitError };
  "git.unstage": { kind: "mutation"; confirm: "reversible"; params: GitFileChange; result: Done; error: SourceGitError };
  "git.worktrees": { kind: "query"; confirm: "none"; params: GitTargetQuery; result: Array<SourceWorktree>; error: SourceGitError };
  "logs.open": { kind: "stream"; confirm: "none"; params: OpenLogs; result: StreamOpened; error: ObserveError };
  "logs.search": { kind: "query"; confirm: "none"; params: SearchLogs; result: SearchResult; error: ObserveError };
  "lsp.install": { kind: "mutation"; confirm: "none"; params: InstallLsp; result: Installed; error: LspError };
  "lsp.open": { kind: "stream"; confirm: "none"; params: OpenLsp; result: StreamOpened; error: LspError };
  "lsp.restart": { kind: "mutation"; confirm: "none"; params: OpenLsp; result: Done; error: LspError };
  "lsp.servers": { kind: "query"; confirm: "none"; params: ServersQuery; result: ServersResult; error: LspError };
  "notes.append": { kind: "mutation"; confirm: "none"; params: AppendNote; result: Appended; error: NotesError };
  "notes.append_response": { kind: "mutation"; confirm: "none"; params: AppendResponseNote; result: ResponseNoteAppended; error: NotesError };
  "notes.attach": { kind: "mutation"; confirm: "none"; params: AttachImage; result: Attached; error: NotesError };
  "notes.backlinks": { kind: "query"; confirm: "none"; params: NotePath; result: Backlinks; error: NotesError };
  "notes.create": { kind: "mutation"; confirm: "none"; params: CreateNote; result: CreatedNote; error: NotesError };
  "notes.daily": { kind: "mutation"; confirm: "none"; params: DailyParams; result: DailyNote; error: NotesError };
  "notes.delete": { kind: "mutation"; confirm: "reversible"; params: NotePath; result: Trashed; error: NotesError };
  "notes.insert_summary": { kind: "mutation"; confirm: "none"; params: InsertSummary; result: SummaryInserted; error: NotesError };
  "notes.read": { kind: "query"; confirm: "none"; params: NotePath; result: NoteContent; error: NotesError };
  "notes.rename": { kind: "mutation"; confirm: "none"; params: RenameNote; result: RenameStarted; error: NotesError };
  "notes.rename_status": { kind: "query"; confirm: "none"; params: RenameOp; result: RenameStatus; error: NotesError };
  "notes.render": { kind: "query"; confirm: "none"; params: RenderNote; result: RenderedNote; error: NotesError };
  "notes.resolve": { kind: "query"; confirm: "none"; params: ResolveLink; result: ResolvedLink; error: NotesError };
  "notes.restore": { kind: "mutation"; confirm: "none"; params: RestoreNote; result: Done; error: NotesError };
  "notes.rewrite_reply": { kind: "mutation"; confirm: "none"; params: RewriteReply; result: Done; error: NotesError };
  "notes.select_response_target": { kind: "query"; confirm: "none"; params: SelectResponseNote; result: ResponseNoteSelection; error: NotesError };
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
  "search.notes": { kind: "query"; confirm: "none"; params: NotesQuery; result: NoteHits; error: SearchError };
  "search.query": { kind: "stream"; confirm: "none"; params: SearchQuery; result: StreamOpened; error: SearchError };
  "search.save": { kind: "mutation"; confirm: "none"; params: SaveSearch; result: SavedRef; error: SearchError };
  "search.saved": { kind: "query"; confirm: "none"; params: NoParams; result: SavedSearches; error: SearchError };
  "search.unsave": { kind: "mutation"; confirm: "reversible"; params: SavedId; result: Done; error: SearchError };
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
  "tools.diagnostics": { kind: "query"; confirm: "none"; params: ToolsEmpty; result: ToolsDiagnosticsInfo; error: ToolsError };
  "tools.install_command": { kind: "query"; confirm: "none"; params: CheckRef; result: InstallPlan; error: ToolsError };
  "tools.support_bundle": { kind: "mutation"; confirm: "none"; params: ToolsEmpty; result: SupportBundle; error: ToolsError };
  "webhooks.fixture_save": { kind: "mutation"; confirm: "none"; params: WebhookSaveFixture; result: WebhookFixture; error: WebhooksError };
  "webhooks.history": { kind: "query"; confirm: "none"; params: WebhookHistoryQuery; result: WebhookReceivedPage; error: WebhooksError };
  "webhooks.history_get": { kind: "query"; confirm: "none"; params: WebhookReceivedId; result: WebhookReceivedDetail; error: WebhooksError };
  "webhooks.listeners": { kind: "query"; confirm: "none"; params: WebhookEmpty; result: WebhookListenerList; error: WebhooksError };
  "webhooks.rule_delete": { kind: "mutation"; confirm: "always"; params: WebhookRuleRef; result: Done; error: WebhooksError };
  "webhooks.rule_upsert": { kind: "mutation"; confirm: "none"; params: WebhookUpsertRule; result: ResponseRule; error: WebhooksError };
  "webhooks.rules": { kind: "query"; confirm: "none"; params: WebhookListenerId; result: WebhookRuleList; error: WebhooksError };
  "webhooks.start": { kind: "mutation"; confirm: "none"; params: WebhookStartListener; result: WebhookListener; error: WebhooksError };
  "webhooks.stop": { kind: "mutation"; confirm: "none"; params: WebhookListenerId; result: Done; error: WebhooksError };
}

export interface Topics {
  "activity.changed": ActivityChanged;
  "activity.status_changed": ActivityStatus;
  "agents.attention": AgentsAttention;
  "agents.changed": AgentsChanged;
  "api.collections_changed": CollectionsChange;
  "deps.audit_changed": AuditProgress;
  "documents.changed": DocumentChanged;
  "documents.lease_lost": LeaseLost;
  "documents.moved": DocumentMoved;
  "git.changed": GitChanged;
  "git.operation_changed": GitOperation;
  "lsp.restarted": Restarted;
  "notes.changed": NotesChanged;
  "notes.rename_incomplete": RenameIncomplete;
  "notes.rewrite_request": RewriteRequest;
  "notify.raised": Notification;
  "observe.problems.changed": ProblemsChange;
  "projects.changed": ProjectsChanged;
  "runtime.changed": RuntimeChange;
  "search.changed": SearchChanged;
  "terminal.changed": TerminalChanged;
  "webhooks.received": WebhookReceived;
}

export const METHOD_INFO = {
  "activity.day": { kind: "query", confirm: "none" },
  "activity.export": { kind: "mutation", confirm: "none" },
  "activity.ingest": { kind: "mutation", confirm: "none" },
  "activity.range": { kind: "query", confirm: "none" },
  "activity.rules": { kind: "query", confirm: "none" },
  "activity.set_rules": { kind: "mutation", confirm: "none" },
  "activity.status": { kind: "query", confirm: "none" },
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
  "api.collection_delete": { kind: "mutation", confirm: "always" },
  "api.collection_read": { kind: "query", confirm: "none" },
  "api.collection_review": { kind: "mutation", confirm: "none" },
  "api.collection_write": { kind: "mutation", confirm: "none" },
  "api.collections": { kind: "query", confirm: "none" },
  "api.cookies": { kind: "query", confirm: "none" },
  "api.cookies_clear": { kind: "mutation", confirm: "always" },
  "api.grpc_call": { kind: "stream", confirm: "none" },
  "api.grpc_describe": { kind: "query", confirm: "none" },
  "api.history": { kind: "query", confirm: "none" },
  "api.history_get": { kind: "query", confirm: "none" },
  "api.mcp_connect": { kind: "stream", confirm: "none" },
  "api.send": { kind: "stream", confirm: "none" },
  "api.sse_open": { kind: "stream", confirm: "none" },
  "api.ws_open": { kind: "stream", confirm: "none" },
  "debug.echo": { kind: "stream", confirm: "none" },
  "deps.audit": { kind: "mutation", confirm: "none" },
  "deps.audit_status": { kind: "query", confirm: "none" },
  "deps.list": { kind: "query", confirm: "none" },
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
  "files.create": { kind: "mutation", confirm: "none" },
  "files.delete": { kind: "mutation", confirm: "always" },
  "files.external_editor": { kind: "mutation", confirm: "none" },
  "files.preview": { kind: "query", confirm: "none" },
  "files.stat": { kind: "query", confirm: "none" },
  "files.tree": { kind: "query", confirm: "none" },
  "git.blame": { kind: "query", confirm: "none" },
  "git.branch_delete": { kind: "mutation", confirm: "always" },
  "git.branches": { kind: "query", confirm: "none" },
  "git.checkout": { kind: "mutation", confirm: "none" },
  "git.cleanup": { kind: "mutation", confirm: "always" },
  "git.cleanup_candidates": { kind: "query", confirm: "none" },
  "git.commit": { kind: "mutation", confirm: "none" },
  "git.conflict_apply": { kind: "mutation", confirm: "reversible" },
  "git.conflict_preview": { kind: "query", confirm: "none" },
  "git.conflicts": { kind: "query", confirm: "none" },
  "git.diff": { kind: "query", confirm: "none" },
  "git.discard": { kind: "mutation", confirm: "always" },
  "git.fetch": { kind: "mutation", confirm: "none" },
  "git.log": { kind: "query", confirm: "none" },
  "git.operation_cancel": { kind: "mutation", confirm: "none" },
  "git.operation_status": { kind: "query", confirm: "none" },
  "git.pull": { kind: "mutation", confirm: "always" },
  "git.push": { kind: "mutation", confirm: "always" },
  "git.stage": { kind: "mutation", confirm: "reversible" },
  "git.stash": { kind: "mutation", confirm: "none" },
  "git.stash_drop": { kind: "mutation", confirm: "always" },
  "git.stash_list": { kind: "query", confirm: "none" },
  "git.status": { kind: "query", confirm: "none" },
  "git.unstage": { kind: "mutation", confirm: "reversible" },
  "git.worktrees": { kind: "query", confirm: "none" },
  "logs.open": { kind: "stream", confirm: "none" },
  "logs.search": { kind: "query", confirm: "none" },
  "lsp.install": { kind: "mutation", confirm: "none" },
  "lsp.open": { kind: "stream", confirm: "none" },
  "lsp.restart": { kind: "mutation", confirm: "none" },
  "lsp.servers": { kind: "query", confirm: "none" },
  "notes.append": { kind: "mutation", confirm: "none" },
  "notes.append_response": { kind: "mutation", confirm: "none" },
  "notes.attach": { kind: "mutation", confirm: "none" },
  "notes.backlinks": { kind: "query", confirm: "none" },
  "notes.create": { kind: "mutation", confirm: "none" },
  "notes.daily": { kind: "mutation", confirm: "none" },
  "notes.delete": { kind: "mutation", confirm: "reversible" },
  "notes.insert_summary": { kind: "mutation", confirm: "none" },
  "notes.read": { kind: "query", confirm: "none" },
  "notes.rename": { kind: "mutation", confirm: "none" },
  "notes.rename_status": { kind: "query", confirm: "none" },
  "notes.render": { kind: "query", confirm: "none" },
  "notes.resolve": { kind: "query", confirm: "none" },
  "notes.restore": { kind: "mutation", confirm: "none" },
  "notes.rewrite_reply": { kind: "mutation", confirm: "none" },
  "notes.select_response_target": { kind: "query", confirm: "none" },
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
  "search.notes": { kind: "query", confirm: "none" },
  "search.query": { kind: "stream", confirm: "none" },
  "search.save": { kind: "mutation", confirm: "none" },
  "search.saved": { kind: "query", confirm: "none" },
  "search.unsave": { kind: "mutation", confirm: "reversible" },
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
  "tools.diagnostics": { kind: "query", confirm: "none" },
  "tools.install_command": { kind: "query", confirm: "none" },
  "tools.support_bundle": { kind: "mutation", confirm: "none" },
  "webhooks.fixture_save": { kind: "mutation", confirm: "none" },
  "webhooks.history": { kind: "query", confirm: "none" },
  "webhooks.history_get": { kind: "query", confirm: "none" },
  "webhooks.listeners": { kind: "query", confirm: "none" },
  "webhooks.rule_delete": { kind: "mutation", confirm: "always" },
  "webhooks.rule_upsert": { kind: "mutation", confirm: "none" },
  "webhooks.rules": { kind: "query", confirm: "none" },
  "webhooks.start": { kind: "mutation", confirm: "none" },
  "webhooks.stop": { kind: "mutation", confirm: "none" },
} as const;

export const RPC_ERROR_CODES = [
  "activity.config_unavailable",
  "activity.invalid_input",
  "activity.invalid_range",
  "activity.invalid_rules",
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
  "api.collection_conflict",
  "api.collection_not_found",
  "api.collection_path_invalid",
  "api.collection_too_large",
  "api.file_missing",
  "api.grpc",
  "api.history_not_found",
  "api.invalid_body",
  "api.invalid_cookie",
  "api.invalid_header",
  "api.invalid_input",
  "api.invalid_method",
  "api.invalid_url",
  "api.mcp",
  "api.network",
  "api.oauth_failed",
  "api.project_not_found",
  "api.redaction_failed",
  "api.response_export_failed",
  "api.review_required",
  "api.secret_missing",
  "api.storage_sensitive_body",
  "api.stream_rejected",
  "api.stream_unsupported",
  "api.tls_invalid",
  "api.too_large",
  "api.too_many_streams",
  "api.variable_missing",
  "cancelled",
  "daemon_restarting",
  "debug.unsupported",
  "deps.busy",
  "deps.invalid_project",
  "deps.job_not_found",
  "deps.limit_exceeded",
  "deps.malformed_lock",
  "deps.missing_manifest",
  "deps.no_lockfile",
  "deps.read_failed",
  "deps.unsafe_path",
  "deps.unsupported_lock",
  "documents.conflict",
  "documents.exists",
  "documents.invalid_input",
  "documents.lease_lost",
  "documents.not_empty",
  "documents.not_found",
  "documents.out_of_scope",
  "documents.stale_journal",
  "documents.too_large",
  "documents.unsupported",
  "files.editor_unavailable",
  "files.exists",
  "files.invalid_input",
  "files.limit_exceeded",
  "files.not_empty",
  "files.not_found",
  "files.out_of_scope",
  "files.state_changed",
  "git.conflict_partial",
  "git.conflict_stale",
  "git.invalid_target",
  "git.source_busy",
  "git.source_failed",
  "git.source_invalid_input",
  "git.source_limit_exceeded",
  "git.source_timed_out",
  "git.state_changed",
  "git.unsafe_source",
  "hello_required",
  "internal",
  "invalid_params",
  "lsp.failed",
  "lsp.install_unavailable",
  "lsp.invalid_input",
  "lsp.lease_lost",
  "lsp.limit_exceeded",
  "lsp.missing_server",
  "lsp.outside_project",
  "lsp.project_not_found",
  "lsp.protocol",
  "lsp.stream_unavailable",
  "method_not_found",
  "not_found",
  "notes.append_operation_mismatch",
  "notes.append_uncertain",
  "notes.busy",
  "notes.conflict",
  "notes.exists",
  "notes.invalid_image",
  "notes.invalid_input",
  "notes.invalid_name",
  "notes.invalid_template",
  "notes.not_found",
  "notes.open_unsaved",
  "notes.out_of_vault",
  "notes.response_target_changed",
  "notes.response_unavailable",
  "notes.stale_cursor",
  "notes.summary_broken",
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
  "runtime.windows_copy_busy",
  "runtime.windows_copy_unavailable",
  "search.invalid_input",
  "search.invalid_regex",
  "search.not_found",
  "search.project_not_found",
  "search.stream_unsupported",
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
  "tools.invalid_input",
  "tools.support_failed",
  "tools.support_too_large",
  "webhooks.bind_failed",
  "webhooks.invalid_input",
  "webhooks.limit_reached",
  "webhooks.not_found",
  "webhooks.storage",
] as const;
export type RpcErrorCode = (typeof RPC_ERROR_CODES)[number];
