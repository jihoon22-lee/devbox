import {
  changeLspDocument,
  closeLspDocument,
  languageServerStatuses,
  loadLspConfig,
  openLspDocument,
  reloadLspDocument,
  saveLspDocument,
  startLanguageServer,
  stopLanguageServer,
} from "./api";
import type {
  LanguageServerStatus,
  LoadedLspConfig,
  LspConfig,
  LspDidChange,
  LspDidClose,
  LspDidOpen,
  LspDidSave,
} from "./types";

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
};

export interface LspDocumentSyncState {
  enabled: boolean;
  workspaceReady: boolean;
  workspaceRoot: string | null;
  configuredLanguages: string[];
  runningLanguages: string[];
  lastError: string | null;
}

interface OpenDocument {
  languageId: LspLanguageId;
  uri: string;
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
        await this.ensureOpen(state, generation, languageId, document.text);
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
        if (dirty) await this.transport.change(languageId, opened.uri, text, true);
        else await this.transport.reload(languageId, opened.uri, text);
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
        await this.transport.save(languageId, opened.uri);
      } catch (cause) {
        if (this.isCurrent(state, generation, true)) this.recordError(cause);
      }
    });
  }

  /** Send the final didClose for a logical document and forget its queue. */
  close(documentId: string): Promise<void> {
    const state = this.documents.get(documentId);
    if (!state) return Promise.resolve();
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

  private enqueue(state: DocumentState, operation: () => Promise<void>): Promise<void> {
    const previous = state.queue;
    const transition = this.transition;
    const next = transition
      .catch(() => undefined)
      .then(() => previous.catch(() => undefined))
      .then(operation)
      .catch((cause) => this.recordError(cause));
    state.queue = next;
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
      await this.closeWithoutReporting({ languageId, uri: opened.uri, generation });
      return null;
    }
    state.opened = { languageId, uri: opened.uri, generation };
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
    };
    this.stateSnapshot = next;
    for (const listener of this.listeners) listener(this.getState());
  }
}
