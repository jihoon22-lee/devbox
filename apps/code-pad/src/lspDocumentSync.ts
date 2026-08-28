import {
  changeLspDocument,
  closeLspDocument,
  languageServerStatuses,
  loadLspConfig,
  openLspDocument,
  pullLspDiagnostics,
  requestLspCompletion,
  requestLspDefinition,
  requestLspFormatting,
  requestLspHover,
  requestLspReferences,
  requestLspRename,
  applyLspRename,
  cancelLspRename,
  discardLspRename,
  restartLanguageServer,
  reloadLspDocument,
  saveLspDocument,
  startLanguageServer,
  stopLanguageServer,
} from "./api";
import type {
  LanguageServerStatus,
  AppliedDocumentEdits,
  LspRenameApplyResult,
  LspRenamePreview,
  LoadedLspConfig,
  LspConfig,
  LspCompletionResult,
  LspDiagnosticResult,
  LspDidChange,
  LspDidClose,
  LspDidOpen,
  LspDidSave,
  LspFeatureResponse,
  LspFilteredLocations,
  LspHoverResult,
  LspPosition,
} from "./types";
import type {
  LspDiagnosticsEvent,
  LspStatusEvent,
} from "./types";
import { positionForOffset } from "./lspFeatures";

/** The document-facing language IDs understood by the LSP boundary. */
export const LSP_LANGUAGE_IDS = {
  rs: "rust",
  ts: "typescript",
  tsx: "typescriptreact",
  js: "javascript",
  jsx: "javascriptreact",
  py: "python",
  pyi: "python",
  json: "json",
  jsonc: "jsonc",
  html: "html",
  htm: "html",
  css: "css",
  scss: "scss",
  less: "less",
} as const;

export type LspLanguageId = (typeof LSP_LANGUAGE_IDS)[keyof typeof LSP_LANGUAGE_IDS];

const LANGUAGE_BY_EXTENSION: Record<string, LspLanguageId> = LSP_LANGUAGE_IDS;

/**
 * Return the exact LSP language ID for a file extension.
 *
 * This intentionally has a narrower mapping than CodeMirror's syntax mapping:
 * an unsupported editor mode must not accidentally start a language server.
 */
export function languageIdForPath(path: string): LspLanguageId | null {
  const fileName = path.split("\\").join("/").split("/").pop() ?? "";
  const dot = fileName.lastIndexOf(".");
  if (dot < 0 || dot === fileName.length - 1) return null;
  return LANGUAGE_BY_EXTENSION[fileName.slice(dot + 1).toLowerCase()] ?? null;
}

export interface LspDocumentSnapshot {
  id: string;
  path: string;
  text: string;
  dirty: boolean;
}

export interface LspDocumentTransport {
  loadConfig: () => Promise<LoadedLspConfig>;
  statuses: () => Promise<LanguageServerStatus[]>;
  start: (languageId: string) => Promise<void>;
  stop: (languageId: string) => Promise<void>;
  open: (languageId: string, path: string, text: string) => Promise<LspDidOpen>;
  change: (languageId: string, uri: string, text: string, dirty: boolean) => Promise<LspDidChange>;
  reload: (languageId: string, uri: string, text: string) => Promise<LspDidChange>;
  save: (languageId: string, uri: string) => Promise<LspDidSave>;
  close: (languageId: string, uri: string) => Promise<LspDidClose>;
  pullDiagnostics: (languageId: string, uri: string) => Promise<LspFeatureResponse<LspDiagnosticResult>>;
  completion: (languageId: string, uri: string, position: LspPosition) => Promise<LspFeatureResponse<LspCompletionResult>>;
  hover: (languageId: string, uri: string, position: LspPosition) => Promise<LspFeatureResponse<LspHoverResult | null>>;
  definition: (languageId: string, uri: string, position: LspPosition) => Promise<LspFeatureResponse<LspFilteredLocations>>;
  references: (languageId: string, uri: string, position: LspPosition, includeDeclaration: boolean) => Promise<LspFeatureResponse<LspFilteredLocations>>;
  rename: (languageId: string, uri: string, position: LspPosition, newName: string) => Promise<LspRenamePreview>;
  applyRename: (planId: string) => Promise<LspRenameApplyResult>;
  cancelRename: (planId: string) => Promise<boolean>;
  discardRename: (planId: string) => Promise<boolean>;
  formatting: (languageId: string, uri: string, tabSize: number, insertSpaces: boolean) => Promise<AppliedDocumentEdits>;
  restart: (languageId: string) => Promise<void>;
}

export const nativeLspDocumentTransport: LspDocumentTransport = {
  loadConfig: loadLspConfig,
  statuses: languageServerStatuses,
  start: startLanguageServer,
  stop: stopLanguageServer,
  open: openLspDocument,
  change: changeLspDocument,
  reload: reloadLspDocument,
  save: saveLspDocument,
  close: closeLspDocument,
  pullDiagnostics: pullLspDiagnostics,
  completion: requestLspCompletion,
  hover: requestLspHover,
  definition: requestLspDefinition,
  references: requestLspReferences,
  rename: requestLspRename,
  applyRename: applyLspRename,
  cancelRename: cancelLspRename,
  discardRename: discardLspRename,
  formatting: requestLspFormatting,
  restart: restartLanguageServer,
};

export interface LspDiagnosticsSnapshot {
  documentId: string;
  response: LspFeatureResponse<LspDiagnosticResult>;
}

export interface LspDocumentSyncState {
  enabled: boolean;
  workspaceReady: boolean;
  workspaceRoot: string | null;
  configuredLanguages: string[];
  runningLanguages: string[];
  lastError: string | null;
  staleDiagnostics: boolean;
}

interface OpenDocument {
  languageId: LspLanguageId;
  uri: string;
  version: number;
  text: string;
  generation: number;
}

interface DocumentState {
  doc: LspDocumentSnapshot;
  languageId: LspLanguageId | null;
  generation: number;
  active: boolean;
  closing: boolean;
  opened: OpenDocument | null;
  queue: Promise<void>;
}

function errorMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

function isAlreadyRunning(cause: unknown): boolean {
  return /already\s+running|이미\s*실행/u.test(errorMessage(cause));
}

function isNotRunning(cause: unknown): boolean {
  return /not\s+running|not\s+found|찾을 수 없|실행 중이 아니/u.test(errorMessage(cause));
}

function configuredLanguages(config: LspConfig | null): string[] {
  if (!config) return [];
  const languages = new Set(Object.keys(config.server_by_language));
  for (const server of config.custom_servers) {
    for (const languageId of server.language_ids) languages.add(languageId);
  }
  return [...languages].sort();
}

function configFingerprint(config: LspConfig | null): string {
  return config ? JSON.stringify(config) : "null";
}

function normalizedPath(path: string): string {
  const slashPath = path.replace(/\\/gu, "/");
  const prefix = slashPath.startsWith("//") ? "//" : slashPath.startsWith("/") ? "/" : "";
  const normalized = `${prefix}${slashPath.slice(prefix.length).replace(/\/{2,}/gu, "/")}`;
  if (normalized !== "/" && !/^[A-Za-z]:\/$/u.test(normalized)) {
    return normalized.replace(/\/$/u, "");
  }
  return normalized;
}

function pathsEqual(left: string, right: string): boolean {
  const normalizedLeft = normalizedPath(left);
  const normalizedRight = normalizedPath(right);
  const windowsPath = /^[A-Za-z]:\//u.test(normalizedLeft)
    || /^[A-Za-z]:\//u.test(normalizedRight)
    || normalizedLeft.startsWith("//")
    || normalizedRight.startsWith("//");
  return windowsPath
    ? normalizedLeft.toLowerCase() === normalizedRight.toLowerCase()
    : normalizedLeft === normalizedRight;
}

function pathWithinWorkspace(path: string, workspaceRoot: string | null): boolean {
  if (!workspaceRoot) return false;
  const normalizedPathValue = normalizedPath(path);
  const normalizedRoot = normalizedPath(workspaceRoot);
  const windowsPath = /^[A-Za-z]:\//u.test(normalizedPathValue)
    || /^[A-Za-z]:\//u.test(normalizedRoot)
    || normalizedPathValue.startsWith("//")
    || normalizedRoot.startsWith("//");
  const candidate = windowsPath ? normalizedPathValue.toLowerCase() : normalizedPathValue;
  const root = windowsPath ? normalizedRoot.toLowerCase() : normalizedRoot;
  if (root === "/" || /^[a-z]:\/$/u.test(root)) return candidate.startsWith(root);
  return pathsEqual(path, workspaceRoot) || candidate.startsWith(`${root}/`);
}

function relativeWorkspacePath(path: string, workspaceRoot: string | null): string | null {
  if (!pathWithinWorkspace(path, workspaceRoot) || !workspaceRoot) return null;
  const normalizedValue = normalizedPath(path);
  const normalizedRoot = normalizedPath(workspaceRoot);
  if (pathsEqual(path, workspaceRoot)) return "";
  return normalizedValue.slice(normalizedRoot.length).replace(/^\/+/u, "");
}

/**
 * Ordered, failure-tolerant bridge between editor documents and the native
 * language-server commands.
 *
 * The class deliberately owns no editor state. App supplies a snapshot for
 * each logical document transaction, which keeps the boundary easy to test
 * and makes a failed server an ordinary, non-blocking side effect.
 */
export class LspDocumentSync {
  private readonly transport: LspDocumentTransport;
  private readonly documents = new Map<string, DocumentState>();
  private readonly listeners = new Set<(state: LspDocumentSyncState) => void>();
  private readonly diagnosticsListeners = new Set<(snapshot: LspDiagnosticsSnapshot) => void>();
  private readonly diagnostics = new Map<string, LspDiagnosticsSnapshot>();
  private readonly diagnosticsTimers = new Map<string, ReturnType<typeof setTimeout>>();
  private readonly featureTokens = new Map<string, number>();
  private config: LspConfig | null = null;
  private configLoaded = false;
  private contextLoadPromise: Promise<void> | null = null;
  private workspaceRoot: string | null = null;
  private statusesSnapshot: LanguageServerStatus[] = [];
  private readonly startedLanguages = new Set<string>();
  private contextGeneration = 0;
  private transition: Promise<void> = Promise.resolve();
  private lastConfigFingerprint = "null";
  private stateSnapshot: LspDocumentSyncState = {
    enabled: false,
    workspaceReady: false,
    workspaceRoot: null,
    configuredLanguages: [],
    runningLanguages: [],
    lastError: null,
    staleDiagnostics: false,
  };

  constructor(transport: LspDocumentTransport = nativeLspDocumentTransport) {
    this.transport = transport;
  }

  getState(): LspDocumentSyncState {
    return {
      ...this.stateSnapshot,
      configuredLanguages: this.stateSnapshot.configuredLanguages.slice(),
      runningLanguages: this.stateSnapshot.runningLanguages.slice(),
    };
  }

  subscribe(listener: (state: LspDocumentSyncState) => void): () => void {
    this.listeners.add(listener);
    listener(this.getState());
    return () => this.listeners.delete(listener);
  }

  subscribeDiagnostics(listener: (snapshot: LspDiagnosticsSnapshot) => void): () => void {
    this.diagnosticsListeners.add(listener);
    for (const snapshot of this.diagnostics.values()) listener(snapshot);
    return () => this.diagnosticsListeners.delete(listener);
  }

  getDiagnostics(documentId: string): LspDiagnosticsSnapshot | null {
    return this.diagnostics.get(documentId) ?? null;
  }

  statusForDocument(documentId: string): LanguageServerStatus | null {
    const state = this.documents.get(documentId);
    return state?.languageId
      ? this.statusesSnapshot.find((status) => status.languageId === state.languageId) ?? null
      : null;
  }

  documentIdForUri(uri: string): string | null {
    for (const [documentId, state] of this.documents) {
      if (state.opened?.uri === uri) return documentId;
    }
    return null;
  }

  documentVersion(documentId: string): number | null {
    return this.documents.get(documentId)?.opened?.version ?? null;
  }

  documentText(documentId: string): string | null {
    return this.documents.get(documentId)?.opened?.text ?? null;
  }

  documentUri(documentId: string): string | null {
    return this.documents.get(documentId)?.opened?.uri ?? null;
  }

  acceptStatusEvent(event: LspStatusEvent): void {
    const index = this.statusesSnapshot.findIndex((status) => status.languageId === event.languageId);
    if (index === -1) this.statusesSnapshot = [...this.statusesSnapshot, event.status];
    else {
      const next = this.statusesSnapshot.slice();
      next[index] = event.status;
      this.statusesSnapshot = next;
    }
    if (event.reason) this.recordError(event.reason);
    else this.publishState();
  }

  acceptDiagnosticsEvent(event: LspDiagnosticsEvent): void {
    // A push report without a server document version has no safe generation
    // boundary. Pull reports carry the request snapshot metadata instead;
    // ignore versionless pushes rather than presenting a late report as
    // current after an editor mutation.
    if (event.response.value.origin === "push" && event.response.value.version == null) return;
    const entry = [...this.documents.entries()].find(([, state]) =>
      state.languageId === event.languageId && state.opened?.uri === event.response.metadata.uri,
    );
    if (!entry) return;
    const [documentId, state] = entry;
    const currentVersion = state.opened?.version;
    const payloadVersion = event.response.value.version;
    const stale = currentVersion !== undefined && (
      event.response.metadata.version !== currentVersion
      || (payloadVersion !== null && payloadVersion !== undefined && payloadVersion !== currentVersion)
      || state.opened?.text !== state.doc.text
    );
    const previous = this.diagnostics.get(documentId);
    const incomingStale = event.response.stale || stale;
    const previousMatchesCurrent = Boolean(
      previous
      && currentVersion !== undefined
      && !previous.response.stale
      && previous.response.metadata.version === currentVersion
      && (previous.response.value.version === null
        || previous.response.value.version === undefined
        || previous.response.value.version === currentVersion)
      && state.opened?.text === state.doc.text,
    );
    // A late push must never replace or stale-mark a diagnostic result that
    // already describes the current native mirror.  This is common when a
    // server's push queue races with a newer pull result.  Once the editor
    // text has changed, the previous result is genuinely old and remains
    // visible only as stale until a fresh result arrives.
    if (incomingStale && previousMatchesCurrent) return;
    if (incomingStale && previous?.response.stale && state.opened?.text !== state.doc.text) return;
    // Do not let a stale-only first event create a banner or an empty lint
    // snapshot for a document that has never had a current result.
    if (incomingStale && !previous) return;
    const staleVersion = currentVersion ?? previous?.response.metadata.version ?? event.response.metadata.version;
    const response = incomingStale && previous
      ? {
          ...previous.response,
          metadata: { ...previous.response.metadata, version: staleVersion },
          value: { ...previous.response.value, version: staleVersion, stale: true },
          stale: true,
        }
      : {
          ...event.response,
          value: { ...event.response.value, stale: event.response.value.stale || incomingStale },
          stale: incomingStale,
        };
    const snapshot = { documentId, response };
    this.diagnostics.set(documentId, snapshot);
    this.publishDiagnostics(snapshot);
    this.stateSnapshot = {
      ...this.stateSnapshot,
      staleDiagnostics: [...this.diagnostics.values()].some((item) => item.response.stale),
    };
    this.publishState();
  }

  /** Native restart reopens documents; the status event is the UI boundary. */
  restart(languageId: string): Promise<void> {
    return this.transport.restart(languageId).catch((cause) => {
      this.recordError(cause);
    });
  }

  /** Change the active workspace and safely close/reopen all registered docs. */
  async setWorkspace(workspaceRoot: string | null): Promise<void> {
    if (this.workspaceRoot === workspaceRoot) {
      this.publishState();
      return;
    }
    this.workspaceRoot = workspaceRoot;
    this.publishState();
    await this.reconfigureContext();
  }

  /** Apply a freshly saved config, including the safe restart boundary. */
  async setConfig(config: LspConfig): Promise<void> {
    const nextFingerprint = configFingerprint(config);
    if (this.configLoaded && this.lastConfigFingerprint === nextFingerprint) return;
    this.config = config;
    this.configLoaded = true;
    this.lastConfigFingerprint = nextFingerprint;
    this.publishState();
    await this.reconfigureContext();
  }

  /** Refresh an externally changed config without making edits wait on it. */
  async refreshConfig(): Promise<void> {
    let loaded: LoadedLspConfig;
    try {
      loaded = await this.transport.loadConfig();
    } catch (cause) {
      this.configLoaded = true;
      this.config = null;
      this.lastConfigFingerprint = "null";
      this.recordError(cause);
      return;
    }
    const changed = !this.configLoaded || this.lastConfigFingerprint !== configFingerprint(loaded.config);
    this.config = loaded.config;
    this.configLoaded = true;
    this.lastConfigFingerprint = configFingerprint(loaded.config);
    if (loaded.error) this.recordError(loaded.error);
    this.publishState();
    if (changed) await this.reconfigureContext();
  }

  /** Register a document and send didOpen once a configured server is ready. */
  open(document: LspDocumentSnapshot): Promise<void> {
    const state = this.activateDocument(document);
    const languageId = state.languageId;
    if (!languageId) return Promise.resolve();
    const generation = state.generation;
    return this.enqueue(state, async () => {
      if (!this.isCurrent(state, generation, true) || state.languageId !== languageId) return;
      try {
        const opened = await this.ensureOpen(state, generation, languageId, document.text);
        if (opened) this.scheduleDiagnostics(document.id, generation);
      } catch (cause) {
        if (this.isCurrent(state, generation, true)) this.recordError(cause);
      }
    });
  }

  /** Queue one local editor transaction as an ordered full-document change. */
  change(document: LspDocumentSnapshot): Promise<void> {
    return this.changeWithDirty(document, document.dirty);
  }

  /** Queue a disk reload as didChange without marking the native buffer dirty. */
  reload(document: LspDocumentSnapshot): Promise<void> {
    return this.changeWithDirty(document, false);
  }

  private changeWithDirty(document: LspDocumentSnapshot, dirty: boolean): Promise<void> {
    const state = this.documents.get(document.id);
    if (!state || !state.active || state.closing) return Promise.resolve();
    state.doc = document;
    // Invalidate editor lint immediately when its text changes. A versionless
    // push is deliberately ignored at the native boundary, so retaining an
    // old snapshot as current would leave stale ranges visible indefinitely.
    this.invalidateDiagnostics(document.id);
    const languageId = state.languageId;
    if (!languageId) return Promise.resolve();
    const generation = state.generation;
    // Capture the transaction text now. Reading state.doc in the async body
    // would collapse two fast editor transactions into one notification.
    const text = document.text;
    return this.enqueue(state, async () => {
      if (!this.isCurrent(state, generation, true) || state.languageId !== languageId) return;
      try {
        const opened = await this.ensureOpen(state, generation, languageId, text);
        if (!opened || !this.isCurrent(state, generation, true)) return;
        const changed = dirty
          ? await this.transport.change(languageId, opened.uri, text, true)
          : await this.transport.reload(languageId, opened.uri, text);
        opened.version = changed.version;
        opened.text = text;
        this.scheduleDiagnostics(document.id, generation);
      } catch (cause) {
        if (this.isCurrent(state, generation, true)) this.recordError(cause);
      }
    });
  }

  /** Queue didSave only after the caller has completed the native file save. */
  save(documentId: string): Promise<void> {
    const state = this.documents.get(documentId);
    if (!state || !state.active || state.closing || !state.languageId) return Promise.resolve();
    const languageId = state.languageId;
    const generation = state.generation;
    return this.enqueue(state, async () => {
      if (!this.isCurrent(state, generation, true) || state.languageId !== languageId) return;
      try {
        const opened = await this.ensureOpen(state, generation, languageId, state.doc.text);
        if (!opened || !this.isCurrent(state, generation, true)) return;
        const saved = await this.transport.save(languageId, opened.uri);
        opened.version = saved.version;
        this.scheduleDiagnostics(documentId, generation);
      } catch (cause) {
        if (this.isCurrent(state, generation, true)) this.recordError(cause);
      }
    });
  }

  requestCompletion(documentId: string, offset: number): Promise<LspFeatureResponse<LspCompletionResult> | null> {
    const token = this.nextFeatureToken(documentId);
    return this.requestFeature(documentId, token, async (state, opened, status) => {
      if (status?.capabilities.completion === false) return null;
      const position = positionForOffset(state.doc.text, offset, status?.capabilities.positionEncoding);
      const response = await this.transport.completion(state.languageId!, opened.uri, position);
      return this.isFeatureCurrent(documentId, token) ? response : null;
    }, true);
  }

  requestHover(documentId: string, offset: number): Promise<LspFeatureResponse<LspHoverResult | null> | null> {
    const token = this.nextFeatureToken(documentId);
    return this.requestFeature(documentId, token, async (state, opened, status) => {
      if (status?.capabilities.hover === false) return null;
      const position = positionForOffset(state.doc.text, offset, status?.capabilities.positionEncoding);
      const response = await this.transport.hover(state.languageId!, opened.uri, position);
      return this.isFeatureCurrent(documentId, token) ? response : null;
    }, true);
  }

  requestDefinition(documentId: string, offset: number): Promise<LspFeatureResponse<LspFilteredLocations> | null> {
    const token = this.nextFeatureToken(documentId);
    return this.requestFeature(documentId, token, async (state, opened, status) => {
      if (status?.capabilities.definition === false) return null;
      const position = positionForOffset(state.doc.text, offset, status?.capabilities.positionEncoding);
      const response = await this.transport.definition(state.languageId!, opened.uri, position);
      return this.isFeatureCurrent(documentId, token) ? response : null;
    });
  }

  requestReferences(
    documentId: string,
    offset: number,
    includeDeclaration = true,
  ): Promise<LspFeatureResponse<LspFilteredLocations> | null> {
    const token = this.nextFeatureToken(documentId);
    return this.requestFeature(documentId, token, async (state, opened, status) => {
      if (status?.capabilities.references === false) return null;
      const position = positionForOffset(state.doc.text, offset, status?.capabilities.positionEncoding);
      const response = await this.transport.references(
        state.languageId!,
        opened.uri,
        position,
        includeDeclaration,
      );
      return this.isFeatureCurrent(documentId, token) ? response : null;
    });
  }

  requestRename(documentId: string, offset: number, newName: string): Promise<LspRenamePreview | null> {
    const token = this.nextFeatureToken(documentId);
    return this.requestFeature(documentId, token, async (state, opened, status) => {
      if (status?.capabilities.rename === false || (status && status.capabilities.syncKind == null)) return null;
      const position = positionForOffset(state.doc.text, offset, status?.capabilities.positionEncoding);
      const result = await this.transport.rename(state.languageId!, opened.uri, position, newName);
      if (this.isFeatureCurrent(documentId, token)) return result;
      if (result.planId) void this.transport.discardRename(result.planId);
      return null;
    });
  }

  applyRename(planId: string): Promise<LspRenameApplyResult> {
    return this.transport.applyRename(planId);
  }

  cancelRename(planId: string): Promise<boolean> {
    return this.transport.cancelRename(planId);
  }

  discardRename(planId: string): Promise<boolean> {
    return this.transport.discardRename(planId);
  }

  requestFormatting(
    documentId: string,
    tabSize = 4,
    insertSpaces = true,
  ): Promise<AppliedDocumentEdits | null> {
    const token = this.nextFeatureToken(documentId);
    return this.requestFeature(documentId, token, async (state, opened, status) => {
      if (status?.capabilities.formatting === false) return null;
      const result = await this.transport.formatting(state.languageId!, opened.uri, tabSize, insertSpaces);
      return this.isFeatureCurrent(documentId, token) ? result : null;
    });
  }

  /** Advance the native mirror after App atomically accepts a mutation result. */
  applyDocuments(
    documents: AppliedDocumentEdits["documents"],
    saved = false,
  ): void {
    for (const edited of documents) {
      const match = [...this.documents.values()].find((state) => state.opened?.uri === edited.uri);
      if (match?.opened) {
        match.opened.version = edited.version;
        match.opened.text = edited.text;
        match.doc = { ...match.doc, text: edited.text, dirty: !saved };
      }
    }
    for (const [documentId, snapshot] of this.diagnostics) {
      if (documents.some((edited) => edited.uri === snapshot.response.metadata.uri)) {
        this.invalidateDiagnostics(documentId);
        this.diagnostics.delete(documentId);
      }
    }
    this.stateSnapshot = {
      ...this.stateSnapshot,
      staleDiagnostics: [...this.diagnostics.values()].some((item) => item.response.stale),
    };
    this.publishState();
  }

  /** Advance rename documents without exposing native absolute URIs to the
   * renderer. The workspace-relative path is matched against the logical
   * editor document and its existing native URI is retained internally. */
  applyRenameDocuments(
    documents: LspRenameApplyResult["documents"],
    saved = true,
  ): void {
    const workspaceRoot = this.workspaceRoot;
    for (const edited of documents) {
      const state = [...this.documents.values()].find((candidate) =>
        relativeWorkspacePath(candidate.doc.path, workspaceRoot) === edited.path,
      );
      if (!state?.opened) continue;
      state.opened.version = edited.version;
      state.opened.text = edited.text;
      state.doc = { ...state.doc, text: edited.text, dirty: !saved };
    }
    for (const documentId of this.diagnostics.keys()) {
      if (documents.some((edited) =>
        relativeWorkspacePath(
          this.documents.get(documentId)?.doc.path ?? "",
          workspaceRoot,
        ) === edited.path,
      )) {
        this.invalidateDiagnostics(documentId);
        this.diagnostics.delete(documentId);
      }
    }
    this.stateSnapshot = {
      ...this.stateSnapshot,
      staleDiagnostics: [...this.diagnostics.values()].some((item) => item.response.stale),
    };
    this.publishState();
  }

  pullDiagnostics(documentId: string): Promise<LspFeatureResponse<LspDiagnosticResult> | null> {
    const state = this.documents.get(documentId);
    if (!state || !state.active || state.closing || !state.languageId) return Promise.resolve(null);
    const generation = state.generation;
    const execute = async () => {
      if (!this.isCurrent(state, generation, true) || !state.languageId) return null;
      const opened = await this.ensureOpen(state, generation, state.languageId, state.doc.text);
      if (!opened || !this.isCurrent(state, generation, true)) return null;
      const status = this.statusesSnapshot.find((item) => item.languageId === state.languageId);
      if (status?.capabilities.diagnostics === false) return null;
      const response = await this.transport.pullDiagnostics(state.languageId, opened.uri);
      if (!this.isCurrent(state, generation, true)) return null;
      const stale = opened.version !== response.metadata.version
        || opened.text !== state.doc.text
        || (response.value.version !== null && response.value.version !== opened.version);
      const currentResponse = stale
        ? {
            ...response,
            value: { ...response.value, stale: true },
            stale: true,
          }
        : response;
      const snapshot = { documentId, response: currentResponse };
      this.diagnostics.set(documentId, snapshot);
      this.publishDiagnostics(snapshot);
      this.stateSnapshot = {
        ...this.stateSnapshot,
        staleDiagnostics: [...this.diagnostics.values()].some((item) => item.response.stale),
      };
      this.publishState();
      return response;
    };
    // Diagnostics is a read request like completion/hover: it must observe the
    // settled document queue without ever blocking it. A slow or hanging pull
    // would otherwise stall rename, formatting, and navigation requests that
    // serialize behind it.
    const previous = state.queue;
    return this.transition
      .catch(() => undefined)
      .then(() => previous.catch(() => undefined))
      .then(execute)
      .catch((cause) => {
        this.recordError(cause);
        return null;
      })
      .then((response) => response ?? null);
  }

  /** Send the final didClose for a logical document and forget its queue. */
  close(documentId: string): Promise<void> {
    const state = this.documents.get(documentId);
    if (!state) return Promise.resolve();
    const diagnosticsTimer = this.diagnosticsTimers.get(documentId);
    if (diagnosticsTimer) clearTimeout(diagnosticsTimer);
    this.diagnosticsTimers.delete(documentId);
    this.diagnostics.delete(documentId);
    this.stateSnapshot = {
      ...this.stateSnapshot,
      staleDiagnostics: [...this.diagnostics.values()].some((item) => item.response.stale),
    };
    this.publishState();
    if (!state.active && state.closing) return state.queue;
    state.active = false;
    state.closing = true;
    return this.enqueue(state, async () => {
      const opened = state.opened;
      state.opened = null;
      if (opened) {
        try {
          await this.transport.close(opened.languageId, opened.uri);
        } catch (cause) {
          this.recordError(cause);
        }
      }
      if (this.documents.get(documentId) === state && !state.active) {
        state.closing = false;
        this.documents.delete(documentId);
      }
    });
  }

  /** Wait until all currently queued lifecycle work has settled. */
  async flush(): Promise<void> {
    await this.transition.catch(() => undefined);
    await Promise.all([...this.documents.values()].map((state) => state.queue.catch(() => undefined)));
  }

  private activateDocument(document: LspDocumentSnapshot): DocumentState {
    const languageId = languageIdForPath(document.path);
    const existing = this.documents.get(document.id);
    if (existing) {
      if (!existing.active) {
        existing.active = true;
        existing.closing = false;
        existing.generation = this.contextGeneration;
      }
      existing.doc = document;
      existing.languageId = languageId;
      return existing;
    }
    const state: DocumentState = {
      doc: document,
      languageId,
      generation: this.contextGeneration,
      active: true,
      closing: false,
      opened: null,
      queue: Promise.resolve(),
    };
    this.documents.set(document.id, state);
    return state;
  }

  private scheduleDiagnostics(documentId: string, generation: number): void {
    const current = this.diagnosticsTimers.get(documentId);
    if (current) clearTimeout(current);
    const timer = setTimeout(() => {
      this.diagnosticsTimers.delete(documentId);
      const state = this.documents.get(documentId);
      if (!state || state.generation !== generation || !state.active || state.closing) return;
      void this.pullDiagnostics(documentId);
    }, 150);
    this.diagnosticsTimers.set(documentId, timer);
  }

  private publishDiagnostics(snapshot: LspDiagnosticsSnapshot): void {
    for (const listener of this.diagnosticsListeners) listener(snapshot);
  }

  private invalidateDiagnostics(documentId: string): void {
    const previous = this.diagnostics.get(documentId);
    if (!previous || previous.response.stale) return;
    const snapshot: LspDiagnosticsSnapshot = {
      documentId,
      response: {
        ...previous.response,
        value: { ...previous.response.value, stale: true },
        stale: true,
      },
    };
    this.diagnostics.set(documentId, snapshot);
    this.publishDiagnostics(snapshot);
    this.stateSnapshot = {
      ...this.stateSnapshot,
      staleDiagnostics: true,
    };
    this.publishState();
  }

  private nextFeatureToken(documentId: string): number {
    const token = (this.featureTokens.get(documentId) ?? 0) + 1;
    this.featureTokens.set(documentId, token);
    return token;
  }

  private isFeatureCurrent(documentId: string, token: number): boolean {
    return this.featureTokens.get(documentId) === token
      && Boolean(this.documents.get(documentId)?.active);
  }

  private requestFeature<T>(
    documentId: string,
    token: number,
    operation: (
      state: DocumentState,
      opened: OpenDocument,
      status: LanguageServerStatus | undefined,
    ) => Promise<T | null>,
    parallel = false,
  ): Promise<T | null> {
    const state = this.documents.get(documentId);
    if (!state || !state.active || state.closing || !state.languageId) return Promise.resolve(null);
    const generation = state.generation;
    const execute = async () => {
      if (!this.isCurrent(state, generation, true) || !state.languageId) return null;
      const opened = await this.ensureOpen(state, generation, state.languageId, state.doc.text);
      if (!opened || !this.isCurrent(state, generation, true)) return null;
      if (!this.isFeatureCurrent(documentId, token)) return null;
      const status = this.statusesSnapshot.find((item) => item.languageId === state.languageId);
      if (status?.status === "crashed" || status?.status === "stopped") return null;
      return operation(state, opened, status);
    };
    if (parallel) {
      const previous = state.queue;
      return this.transition
        .catch(() => undefined)
        .then(() => previous.catch(() => undefined))
        .then(execute)
        .catch((cause) => {
          this.recordError(cause);
          return null;
        })
        .then((result) => result ?? null);
    }
    return this.enqueueResult(state, execute).then((result) => result ?? null);
  }

  private enqueue(state: DocumentState, operation: () => Promise<void>): Promise<void> {
    return this.enqueueResult(state, async () => {
      await operation();
      return undefined;
    }).then(() => undefined);
  }

  private enqueueResult<T>(state: DocumentState, operation: () => Promise<T>): Promise<T | undefined> {
    const previous = state.queue;
    const transition = this.transition;
    const next = transition
      .catch(() => undefined)
      .then(() => previous.catch(() => undefined))
      .then(operation)
      .catch((cause) => {
        this.recordError(cause);
        return undefined;
      });
    state.queue = next.then(() => undefined);
    return next;
  }

  private isCurrent(state: DocumentState, generation: number, allowClosing = false): boolean {
    return this.documents.get(state.doc.id) === state
      && state.generation === generation
      && this.contextGeneration === generation
      && (state.active || (allowClosing && state.closing));
  }

  private async ensureOpen(
    state: DocumentState,
    generation: number,
    languageId: LspLanguageId,
    text: string,
  ): Promise<OpenDocument | null> {
    if (!this.isCurrent(state, generation, true)) return null;
    if (state.opened?.generation === generation && state.opened.languageId === languageId) {
      return state.opened;
    }
    if (!(await this.ensureLanguage(state, generation, languageId))) return null;
    if (!this.isCurrent(state, generation, true)) return null;
    if (state.opened) {
      await this.closeWithoutReporting(state.opened);
      state.opened = null;
    }
    const opened = await this.transport.open(languageId, state.doc.path, text);
    if (!this.isCurrent(state, generation, true) || state.languageId !== languageId) {
      await this.closeWithoutReporting({
        languageId,
        uri: opened.uri,
        version: opened.version,
        text: opened.text,
        generation,
      });
      return null;
    }
    state.opened = { languageId, uri: opened.uri, version: opened.version, text, generation };
    return state.opened;
  }

  private async ensureLanguage(
    state: DocumentState,
    generation: number,
    languageId: LspLanguageId,
  ): Promise<boolean> {
    await this.loadContextIfNeeded();
    if (!this.isCurrent(state, generation, true) || !this.isEligible(languageId)) return false;
    let status = this.statusesSnapshot.find((item) => item.languageId === languageId);
    if (!status || status.status === "stopped" || status.status === "crashed") {
      try {
        await this.transport.start(languageId);
        this.startedLanguages.add(languageId);
      } catch (cause) {
        if (!isAlreadyRunning(cause)) {
          if (this.isCurrent(state, generation, true)) this.recordError(cause);
          return false;
        }
        this.startedLanguages.add(languageId);
      }
      try {
        this.statusesSnapshot = await this.transport.statuses();
        this.publishState();
      } catch (cause) {
        if (this.isCurrent(state, generation, true)) this.recordError(cause);
      }
      status = this.statusesSnapshot.find((item) => item.languageId === languageId);
      // A successful start is itself the running boundary. A status refresh
      // can legitimately lag behind a process that just initialized.
      return this.isCurrent(state, generation, true)
        && (!status || (status.status !== "stopped" && status.status !== "crashed"));
    }
    return true;
  }

  private async loadContextIfNeeded(): Promise<void> {
    if (this.configLoaded) return;
    if (this.contextLoadPromise) return this.contextLoadPromise;
    const loadPromise = (async () => {
      try {
        const loaded = await this.transport.loadConfig();
        // A user-saved config can arrive while the initial load is pending.
        // Never let the older response overwrite that explicit snapshot.
        if (this.configLoaded) return;
        this.config = loaded.config;
        this.lastConfigFingerprint = configFingerprint(loaded.config);
        if (loaded.error) this.recordError(loaded.error);
        this.publishState();
      } catch (cause) {
        if (!this.configLoaded) {
          this.config = null;
          this.lastConfigFingerprint = "null";
          this.recordError(cause);
        }
        return;
      }
      if (this.configLoaded) return;
      try {
        this.statusesSnapshot = await this.transport.statuses();
        this.publishState();
      } catch (cause) {
        this.recordError(cause);
      }
    })().finally(() => {
      if (this.contextLoadPromise === loadPromise) this.contextLoadPromise = null;
      this.configLoaded = true;
    });
    this.contextLoadPromise = loadPromise;
    return loadPromise;
  }

  private isEligible(languageId: LspLanguageId): boolean {
    if (!this.config?.enabled || !this.workspaceRoot) return false;
    if (!pathsEqual(this.config.workspace_root, this.workspaceRoot)) return false;
    return Object.prototype.hasOwnProperty.call(this.config.server_by_language, languageId)
      || this.config.custom_servers.some((server) => server.language_ids.includes(languageId));
  }

  private async reconfigureContext(): Promise<void> {
    const nextGeneration = ++this.contextGeneration;
    const oldStates = [...this.documents.values()];
    const oldQueues = oldStates.map((state) => state.queue.catch(() => undefined));
    const oldLanguages = new Set<string>([
      ...this.startedLanguages,
      ...oldStates.flatMap((state) => state.opened ? [state.opened.languageId] : []),
      ...this.statusesSnapshot
        .filter((status) => status.status !== "stopped")
        .map((status) => status.languageId),
    ]);
    const previous = this.transition;
    const transition = previous
      .catch(() => undefined)
      .then(async () => {
        await Promise.all(oldQueues);
        for (const state of oldStates) {
          if (state.opened) {
            await this.closeWithoutReporting(state.opened);
            state.opened = null;
          }
        }
        for (const languageId of oldLanguages) {
          try {
            await this.transport.stop(languageId);
            this.startedLanguages.delete(languageId);
          } catch (cause) {
            if (isNotRunning(cause)) this.startedLanguages.delete(languageId);
            else this.recordError(cause);
          }
        }
        for (const state of oldStates) {
          if (this.documents.get(state.doc.id) === state) {
            state.generation = nextGeneration;
            state.closing = false;
          }
        }
        this.statusesSnapshot = [];
        this.diagnostics.clear();
        for (const timer of this.diagnosticsTimers.values()) clearTimeout(timer);
        this.diagnosticsTimers.clear();
        this.publishState();
      });
    this.transition = transition;
    await transition;
    // Reopen using the latest snapshot, not the text from the old session.
    await Promise.all(
      [...this.documents.values()]
        .filter((state) => state.active && pathWithinWorkspace(state.doc.path, this.workspaceRoot))
        .map((state) => this.open(state.doc)),
    );
  }

  private async closeWithoutReporting(opened: OpenDocument): Promise<void> {
    try {
      await this.transport.close(opened.languageId, opened.uri);
    } catch {
      // The native save/config boundary may already have stopped the session.
      // A subsequent transition will still establish a clean generation.
    }
  }

  private recordError(cause: unknown): void {
    this.stateSnapshot = { ...this.stateSnapshot, lastError: errorMessage(cause) };
    this.publishState();
  }

  private publishState(): void {
    const enabled = Boolean(this.config?.enabled);
    const workspaceReady = Boolean(
      this.workspaceRoot && pathsEqual(this.config?.workspace_root ?? "", this.workspaceRoot),
    );
    const next: LspDocumentSyncState = {
      enabled,
      workspaceReady,
      workspaceRoot: this.workspaceRoot,
      configuredLanguages: configuredLanguages(this.config),
      runningLanguages: this.statusesSnapshot
        .filter((status) => status.status !== "stopped" && status.status !== "crashed")
        .map((status) => status.languageId)
        .sort(),
      lastError: this.stateSnapshot.lastError,
      staleDiagnostics: [...this.diagnostics.values()].some((item) => item.response.stale),
    };
    this.stateSnapshot = next;
    for (const listener of this.listeners) listener(this.getState());
  }
}
