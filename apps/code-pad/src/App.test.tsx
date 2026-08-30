import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import App from "./App";
import {
  changeLspDocument,
  closeLspDocument,
  applyLspRename,
  cancelLspRename,
  deleteFileAction,
  discardLspRename,
  languageServerStatuses,
  listWorkspaceFiles,
  loadLspConfig,
  loadSession,
  openFile,
  openLspDocument,
  pullLspDiagnostics,
  reloadLspDocument,
  requestLspDefinition,
  requestLspFormatting,
  requestLspReferences,
  requestLspRename,
  renameFileAction,
  renderPreview,
  revealFileAction,
  saveFile,
  saveLspDocument,
  startLanguageServer,
  stopLanguageServer,
  takePendingOpen,
  unwatchFile,
  validateEncoding,
  watchFile,
  workspaceCapabilities,
} from "./api";
import type {
  LanguageServerStatus,
  LspDiagnosticsEvent,
  LspRenamePreview,
  LspStatusEvent,
} from "./types";

const fileChangedHandlerRef: {
  current: ((event: { payload: { path: string; mtimeNanos: string; contentHash: string; size: number } }) => void) | null;
} = { current: null };
const lspDiagnosticsHandlerRef: {
  current: ((event: { payload: LspDiagnosticsEvent }) => void) | null;
} = { current: null };
const lspStatusHandlerRef: {
  current: ((event: { payload: LspStatusEvent }) => void) | null;
} = { current: null };
const appLinkHandlerRef: {
  current: ((event: { payload: { target: { kind: string; [key: string]: unknown }; from: string | null } }) => void) | null;
} = { current: null };
const appLinkOrder: string[] = [];
const rejectAppLinkListenRef = { current: false };

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, handler: unknown) => {
    if (event === "file-changed") fileChangedHandlerRef.current = handler as typeof fileChangedHandlerRef.current;
    if (event === "lsp/diagnostics") lspDiagnosticsHandlerRef.current = handler as typeof lspDiagnosticsHandlerRef.current;
    if (event === "lsp/status") lspStatusHandlerRef.current = handler as typeof lspStatusHandlerRef.current;
    if (event === "devbox://open") {
      if (rejectAppLinkListenRef.current) throw new Error("listener unavailable");
      appLinkOrder.push("listen");
      appLinkHandlerRef.current = handler as typeof appLinkHandlerRef.current;
    }
    return () => {
      if (event === "file-changed" && fileChangedHandlerRef.current === handler) fileChangedHandlerRef.current = null;
      if (event === "lsp/diagnostics" && lspDiagnosticsHandlerRef.current === handler) lspDiagnosticsHandlerRef.current = null;
      if (event === "lsp/status" && lspStatusHandlerRef.current === handler) lspStatusHandlerRef.current = null;
      if (event === "devbox://open" && appLinkHandlerRef.current === handler) appLinkHandlerRef.current = null;
    };
  }),
}));

vi.mock("./components/DocHost", () => ({
  default: (props: {
    docs: Array<{ id: string; path: string; text: string }>;
    onChange: (docId: string, text: string) => void;
    onReplaceCommandReady?: (docId: string, command: (() => boolean) | null) => void;
    diagnostics?: (docId: string) => Array<{ message: string }>;
  }) => (
    <div data-testid="mock-doc-host">
      {props.docs.map((doc) => (
        <button
          type="button"
          key={doc.id}
          aria-label={`edit ${doc.path}`}
          onClick={() => props.onChange(doc.id, `${doc.text}!`)}
        >
          edit
        </button>
      ))}
      {props.docs.map((doc) => (
        <button
          type="button"
          key={`${doc.id}-replace`}
          aria-label={`replace ${doc.path}`}
          onClick={() => props.onReplaceCommandReady?.(doc.id, () => true)}
        >
          replace
        </button>
      ))}
      {props.docs.map((doc) => (
        <output data-testid={`lsp-diagnostics-${doc.id}`} key={`${doc.id}-diagnostics`}>
          {props.diagnostics?.(doc.id).map((diagnostic) => diagnostic.message).join("|")}
        </output>
      ))}
      {props.docs.map((doc) => (
        <output data-testid={`doc-text-${doc.path}`} key={`${doc.id}-text`}>{doc.text}</output>
      ))}
    </div>
  ),
}));

vi.mock("./api", () => ({
  openFile: vi.fn(),
  saveFile: vi.fn(),
  validateEncoding: vi.fn().mockResolvedValue(undefined),
  loadSession: vi.fn().mockResolvedValue({
    session: {
      version: 1,
      workspace_folder: null,
      docs: [],
      views: [[], []],
      active_view: 0,
      active_doc_by_view: [null, null],
      recent_files: [],
    },
    persistAllowed: true,
  }),
  watchFile: vi.fn().mockResolvedValue(undefined),
  unwatchFile: vi.fn().mockResolvedValue(undefined),
  saveSession: vi.fn().mockResolvedValue(undefined),
  loadRecovery: vi.fn().mockResolvedValue([]),
  canonicalizeWorkspace: vi.fn(),
  workspaceCapabilities: vi.fn(),
  listWorkspaceFiles: vi.fn(),
  renderPreview: vi.fn(),
  loadLspConfig: vi.fn().mockResolvedValue({
    config: {
      version: 1,
      enabled: false,
      workspace_root: "",
      server_by_language: {},
      custom_servers: [],
      update_policy: "manual",
    },
    persist_allowed: true,
    error: null,
  }),
  languageServerLogs: vi.fn().mockResolvedValue([]),
  languageServerStatuses: vi.fn().mockResolvedValue([]),
  startLanguageServer: vi.fn().mockResolvedValue(undefined),
  stopLanguageServer: vi.fn().mockResolvedValue(undefined),
  openLspDocument: vi.fn(),
  reloadLspDocument: vi.fn(),
  changeLspDocument: vi.fn(),
  saveLspDocument: vi.fn(),
  closeLspDocument: vi.fn(),
  deleteFileAction: vi.fn(),
  pullLspDiagnostics: vi.fn().mockResolvedValue({ metadata: { uri: "", version: 1 }, value: { uri: "", version: 1, diagnostics: [], origin: "pull" }, stale: false }),
  requestLspCompletion: vi.fn().mockResolvedValue({ metadata: { uri: "", version: 1 }, value: { isIncomplete: false, items: [] }, stale: false }),
  requestLspDefinition: vi.fn().mockResolvedValue({ metadata: { uri: "", version: 1 }, value: { locations: [], rejected: 0 }, stale: false }),
  requestLspFormatting: vi.fn().mockResolvedValue({ documents: [] }),
  requestLspHover: vi.fn().mockResolvedValue({ metadata: { uri: "", version: 1 }, value: null, stale: false }),
  requestLspReferences: vi.fn().mockResolvedValue({ metadata: { uri: "", version: 1 }, value: { locations: [], rejected: 0 }, stale: false }),
  requestLspRename: vi.fn().mockResolvedValue({ planId: "", files: [] }),
  applyLspRename: vi.fn(),
  cancelLspRename: vi.fn().mockResolvedValue(false),
  discardLspRename: vi.fn().mockResolvedValue(false),
  renameFileAction: vi.fn(),
  revealFileAction: vi.fn().mockResolvedValue(undefined),
  restartLanguageServer: vi.fn().mockResolvedValue(undefined),
  takePendingOpen: vi.fn().mockResolvedValue(null),
}));

const openFileMock = vi.mocked(openFile);
const saveFileMock = vi.mocked(saveFile);
const validateEncodingMock = vi.mocked(validateEncoding);
const loadSessionMock = vi.mocked(loadSession);
const watchFileMock = vi.mocked(watchFile);
const unwatchFileMock = vi.mocked(unwatchFile);
const loadLspConfigMock = vi.mocked(loadLspConfig);
const languageServerStatusesMock = vi.mocked(languageServerStatuses);
const listWorkspaceFilesMock = vi.mocked(listWorkspaceFiles);
const workspaceCapabilitiesMock = vi.mocked(workspaceCapabilities);
const startLanguageServerMock = vi.mocked(startLanguageServer);
const stopLanguageServerMock = vi.mocked(stopLanguageServer);
const openLspDocumentMock = vi.mocked(openLspDocument);
const reloadLspDocumentMock = vi.mocked(reloadLspDocument);
const changeLspDocumentMock = vi.mocked(changeLspDocument);
const saveLspDocumentMock = vi.mocked(saveLspDocument);
const closeLspDocumentMock = vi.mocked(closeLspDocument);
const deleteFileActionMock = vi.mocked(deleteFileAction);
const pullLspDiagnosticsMock = vi.mocked(pullLspDiagnostics);
const requestLspDefinitionMock = vi.mocked(requestLspDefinition);
const requestLspFormattingMock = vi.mocked(requestLspFormatting);
const requestLspReferencesMock = vi.mocked(requestLspReferences);
const requestLspRenameMock = vi.mocked(requestLspRename);
const applyLspRenameMock = vi.mocked(applyLspRename);
const cancelLspRenameMock = vi.mocked(cancelLspRename);
const discardLspRenameMock = vi.mocked(discardLspRename);
const renameFileActionMock = vi.mocked(renameFileAction);
const renderPreviewMock = vi.mocked(renderPreview);
const revealFileActionMock = vi.mocked(revealFileAction);
const takePendingOpenMock = vi.mocked(takePendingOpen);

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function openedFile(text = "before", path = "/tmp/one.ts") {
  return {
    path,
    text,
    encoding: { encodingKind: "utf8" as const, bom: false },
    lineEnding: "lf" as const,
    readOnly: false,
    size: text.length,
    mtimeNanos: "1",
    contentHash: "hash-1",
    lossy: false,
    durabilityWarning: null,
  };
}

function savedFile() {
  return {
    path: "/tmp/one.ts",
    mtimeNanos: "2",
    size: 7,
    contentHash: "hash-2",
    durabilityWarning: null,
  };
}

function configureDiagnosticsApp(
  capabilityOverrides: Partial<LanguageServerStatus["capabilities"]> = {},
) {
  loadSessionMock.mockResolvedValue({
    session: {
      version: 1,
      workspace_folder: "/tmp",
      docs: [],
      views: [[], []],
      active_view: 0,
      active_doc_by_view: [null, null],
      recent_files: [],
    },
    persistAllowed: true,
  });
  loadLspConfigMock.mockResolvedValue({
    config: {
      version: 1,
      enabled: true,
      workspace_root: "/tmp",
      server_by_language: {
        typescript: { kind: "local", installed_path: "/tools/tsls", args: [] },
      },
      custom_servers: [],
      update_policy: "manual",
    },
    persist_allowed: true,
    error: null,
  });
  const status: LanguageServerStatus = {
    languageId: "typescript",
    status: "ready",
    processState: "running",
    serverInfo: null,
    capabilities: {
      positionEncoding: "utf-16",
      legacyPositionEncoding: false,
      syncKind: "full",
      openClose: true,
      save: true,
      completion: true,
      hover: true,
      definition: true,
      references: true,
      rename: true,
      formatting: true,
      diagnostics: true,
      ...capabilityOverrides,
    },
    documentCount: 1,
  };
  languageServerStatusesMock.mockResolvedValue([status]);
  openLspDocumentMock.mockResolvedValue({
    uri: "file:///tmp/one.ts",
    languageId: "typescript",
    version: 1,
    text: "before",
  });
  // Keep the debounce request pending so these tests exercise only the
  // event freshness boundary under test.
  pullLspDiagnosticsMock.mockReturnValue(new Promise(() => undefined));
}

function diagnosticEvent(version: number, message: string): LspDiagnosticsEvent {
  return {
    languageId: "typescript",
    response: {
      metadata: { uri: "file:///tmp/one.ts", version },
      value: {
        uri: "file:///tmp/one.ts",
        version,
        diagnostics: [{
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 2 } },
          message,
          severity: 2,
        }],
        origin: "push",
      },
      stale: false,
    },
  };
}

beforeEach(() => {
  openFileMock.mockReset();
  saveFileMock.mockReset();
  validateEncodingMock.mockReset().mockResolvedValue(undefined);
  loadSessionMock.mockReset().mockResolvedValue({
    session: {
      version: 1,
      workspace_folder: null,
      docs: [],
      views: [[], []],
      active_view: 0,
      active_doc_by_view: [null, null],
      recent_files: [],
    },
    persistAllowed: true,
  });
  watchFileMock.mockClear();
  unwatchFileMock.mockClear();
  loadLspConfigMock.mockReset().mockResolvedValue({
    config: {
      version: 1,
      enabled: false,
      workspace_root: "",
      server_by_language: {},
      custom_servers: [],
      update_policy: "manual",
    },
    persist_allowed: true,
    error: null,
  });
  languageServerStatusesMock.mockReset().mockResolvedValue([]);
  workspaceCapabilitiesMock.mockReset().mockImplementation(async (path) => ({
    path,
    sourceKind: "native",
    watchMode: "native",
    editSupported: true,
    lspSupported: true,
    lspReason: null,
  }));
  listWorkspaceFilesMock.mockReset().mockResolvedValue({ files: [], truncated: false, incomplete: false });
  startLanguageServerMock.mockReset().mockResolvedValue(undefined);
  stopLanguageServerMock.mockReset().mockResolvedValue(undefined);
  openLspDocumentMock.mockReset();
  reloadLspDocumentMock.mockReset();
  changeLspDocumentMock.mockReset();
  saveLspDocumentMock.mockReset();
  closeLspDocumentMock.mockReset();
  deleteFileActionMock.mockReset().mockResolvedValue(undefined);
  requestLspDefinitionMock.mockReset().mockResolvedValue({ metadata: { uri: "", version: 1 }, value: { locations: [], rejected: 0 }, stale: false });
  requestLspFormattingMock.mockReset().mockResolvedValue({ documents: [] });
  requestLspReferencesMock.mockReset().mockResolvedValue({ metadata: { uri: "", version: 1 }, value: { locations: [], rejected: 0 }, stale: false });
  requestLspRenameMock.mockReset().mockResolvedValue({ planId: "", files: [] });
  applyLspRenameMock.mockReset();
  cancelLspRenameMock.mockReset().mockResolvedValue(false);
  discardLspRenameMock.mockReset().mockResolvedValue(false);
  renameFileActionMock.mockReset();
  renderPreviewMock.mockReset().mockResolvedValue({
    kind: "markdown",
    html: "<p>preview</p>",
    mermaid: [],
    source: null,
  });
  revealFileActionMock.mockReset().mockResolvedValue(undefined);
  takePendingOpenMock.mockReset().mockImplementation(async () => {
    appLinkOrder.push("take");
    return null;
  });
  fileChangedHandlerRef.current = null;
  lspDiagnosticsHandlerRef.current = null;
  lspStatusHandlerRef.current = null;
  appLinkHandlerRef.current = null;
  appLinkOrder.length = 0;
  rejectAppLinkListenRef.current = false;
});

afterEach(() => cleanup());

async function openOne() {
  openFileMock.mockResolvedValue(openedFile());
  const rendered = render(<App />);
  const input = rendered.getByRole("textbox", { name: "열 파일 경로" });
  await waitFor(() => expect((input as HTMLInputElement).disabled).toBe(false));
  fireEvent.change(input, { target: { value: "/tmp/one.ts" } });
  fireEvent.click(rendered.getByRole("button", { name: "파일 열기" }));
  await waitFor(() => expect(rendered.getByRole("tab", { name: /one\.ts/ })).toBeTruthy());
  return rendered;
}

async function openAdditional(
  rendered: Awaited<ReturnType<typeof openOne>>,
  path: string,
  text = "before",
) {
  openFileMock.mockResolvedValueOnce(openedFile(text, path));
  const input = rendered.getByRole("textbox", { name: "열 파일 경로" });
  fireEvent.change(input, { target: { value: path } });
  fireEvent.click(rendered.getByRole("button", { name: "파일 열기" }));
  await waitFor(() => expect(rendered.getByRole("tab", { name: new RegExp(fileName(path)) })).toBeTruthy());
}

function fileName(path: string): string {
  return path.split("/").pop() ?? path;
}

describe("App editor shell operations", () => {
  it("초기 셸이 접근성 위반 없이 렌더링된다", async () => {
    const { container, getByRole } = render(<App />);
    await waitFor(() => expect((getByRole("textbox", { name: "열 파일 경로" }) as HTMLInputElement).disabled).toBe(false));
    await assertNoA11yViolations(container);
  });

  it("exposes separate editor and preview regions while keeping preview state explicit", async () => {
    loadSessionMock.mockResolvedValue({
      session: {
        version: 1,
        workspace_folder: "/tmp",
        docs: [],
        views: [[], []],
        active_view: 0,
        active_doc_by_view: [null, null],
        recent_files: [],
      },
      persistAllowed: true,
    });
    openFileMock.mockResolvedValue(openedFile("# Title", "/tmp/readme.md"));

    const rendered = render(<App />);
    const input = rendered.getByRole("textbox", { name: "열 파일 경로" });
    await waitFor(() => expect((input as HTMLInputElement).disabled).toBe(false));
    fireEvent.change(input, { target: { value: "/tmp/readme.md" } });
    fireEvent.click(rendered.getByRole("button", { name: "파일 열기" }));
    await waitFor(() => expect(rendered.getByRole("tab", { name: /readme\.md/ })).toBeTruthy());

    const previewToggle = rendered.getByRole("button", { name: "프리뷰" });
    expect(previewToggle.getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(previewToggle);

    const previewRegion = await rendered.findByRole("complementary", { name: "프리뷰" });
    const contentArea = previewRegion.parentElement;
    expect(contentArea?.classList.contains("with-preview")).toBe(true);
    expect(contentArea?.querySelector(".editor-area")).not.toBeNull();
    expect(previewRegion.querySelector(".preview-pane-path")?.textContent).toBe("/tmp/readme.md");
    expect(rendered.getByRole("tab", { name: /readme\.md/ }).getAttribute("aria-selected")).toBe("true");
    expect(previewToggle.getAttribute("aria-pressed")).toBe("true");
    await waitFor(() => expect(renderPreviewMock).toHaveBeenCalledWith(
      "/tmp/readme.md",
      "# Title",
      "/tmp",
    ));

    fireEvent.click(previewToggle);
    expect(rendered.queryByRole("complementary", { name: "프리뷰" })).toBeNull();
    expect(contentArea?.classList.contains("with-preview")).toBe(false);
    expect(rendered.getByRole("tab", { name: /readme\.md/ })).toBeTruthy();
  });

  it("loads the restored workspace when Quick Open is opened with Ctrl+P", async () => {
    loadSessionMock.mockResolvedValue({
      session: {
        version: 1,
        workspace_folder: "/tmp/workspace",
        docs: [],
        views: [[], []],
        active_view: 0,
        active_doc_by_view: [null, null],
        recent_files: [],
      },
      persistAllowed: true,
    });
    listWorkspaceFilesMock.mockResolvedValue({
      files: [{ path: "/tmp/workspace/src/main.ts", relativePath: "src/main.ts", size: 12 }],
      truncated: false,
      incomplete: false,
    });

    const rendered = render(<App />);
    await waitFor(() => expect((rendered.getByRole("textbox", { name: "열 파일 경로" }) as HTMLInputElement).disabled).toBe(false));
    fireEvent.keyDown(window, { key: "p", ctrlKey: true });
    await waitFor(() => expect(listWorkspaceFilesMock).toHaveBeenCalledWith("/tmp/workspace"));
    expect(await rendered.findByRole("dialog", { name: "빠른 파일 열기" })).toBeTruthy();
    expect(rendered.getByRole("option", { name: "src/main.ts" })).toBeTruthy();
  });

  it("registers the app-link listener before consuming and takes again on relaunch", async () => {
    openFileMock.mockResolvedValue(openedFile());
    render(<App />);

    await waitFor(() => expect(appLinkHandlerRef.current).not.toBeNull());
    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(1));
    expect(appLinkOrder.slice(0, 2)).toEqual(["listen", "take"]);

    takePendingOpenMock.mockResolvedValueOnce({
      target: { kind: "path", path: "/tmp/one.ts", line: null, column: null },
      from: "repo-manager",
    });
    appLinkHandlerRef.current?.({
      payload: { target: { kind: "query", text: "stale-event-payload" }, from: "test" },
    });

    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(openFileMock).toHaveBeenCalledWith("/tmp/one.ts", null));
  });

  it("consumes a cold-start request when app-link listener registration fails", async () => {
    rejectAppLinkListenRef.current = true;
    openFileMock.mockResolvedValue(openedFile());
    takePendingOpenMock.mockResolvedValueOnce({
      target: { kind: "path", path: "/tmp/listener-fallback.ts", line: null, column: null },
      from: "repo-manager",
    });

    render(<App />);

    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(openFileMock).toHaveBeenCalledWith("/tmp/listener-fallback.ts", null));
  });

  it("locks duplicate open requests before React can rerender", async () => {
    const request = deferred<ReturnType<typeof openedFile>>();
    openFileMock.mockReturnValue(request.promise);
    const rendered = render(<App />);
    const input = rendered.getByRole("textbox", { name: "열 파일 경로" });
    await waitFor(() => expect((input as HTMLInputElement).disabled).toBe(false));
    fireEvent.change(input, { target: { value: "/tmp/one.ts" } });
    fireEvent.click(rendered.getByRole("button", { name: "파일 열기" }));
    fireEvent.click(rendered.getByRole("button", { name: "파일 열기" }));
    expect(openFileMock).toHaveBeenCalledTimes(1);
    request.resolve(openedFile());
    await waitFor(() => expect(rendered.getByRole("tab", { name: /one\.ts/ })).toBeTruthy());
  });

  it("does not open a path while an IME composition is committing Enter", async () => {
    openFileMock.mockResolvedValue(openedFile());
    const rendered = render(<App />);
    const input = rendered.getByRole("textbox", { name: "열 파일 경로" });
    await waitFor(() => expect((input as HTMLInputElement).disabled).toBe(false));
    fireEvent.change(input, { target: { value: "/tmp/one.ts" } });

    fireEvent.keyDown(input, { key: "Enter", isComposing: true });
    expect(openFileMock).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(openFileMock).toHaveBeenCalledTimes(1));
  });

  it("keeps a newer edit dirty after the save response refreshes disk metadata", async () => {
    const rendered = await openOne();
    const edit = rendered.getByRole("button", { name: "edit /tmp/one.ts" });
    fireEvent.click(edit);
    const request = deferred<ReturnType<typeof savedFile>>();
    saveFileMock.mockReturnValue(request.promise);
    fireEvent.keyDown(window, { key: "s", ctrlKey: true });
    fireEvent.click(edit);
    fireEvent.keyDown(window, { key: "s", ctrlKey: true });
    expect(saveFileMock).toHaveBeenCalledTimes(1);
    request.resolve(savedFile());
    await waitFor(() => expect(rendered.getByRole("tab").textContent).toContain("●"));
  });

  it("sends didSave only after the native file save succeeds", async () => {
    loadSessionMock.mockResolvedValue({
      session: {
        version: 1,
        workspace_folder: "/tmp",
        docs: [],
        views: [[], []],
        active_view: 0,
        active_doc_by_view: [null, null],
        recent_files: [],
      },
      persistAllowed: true,
    });
    loadLspConfigMock.mockResolvedValue({
      config: {
        version: 1,
        enabled: true,
        workspace_root: "/tmp",
        server_by_language: {
          typescript: { kind: "local", installed_path: "/tools/tsls", args: [] },
        },
        custom_servers: [],
        update_policy: "manual",
      },
      persist_allowed: true,
      error: null,
    });
    languageServerStatusesMock.mockResolvedValue([]);
    openLspDocumentMock.mockResolvedValue({
      uri: "file:///tmp/one.ts",
      languageId: "typescript",
      version: 1,
      text: "before",
    });
    changeLspDocumentMock.mockResolvedValue({
      uri: "file:///tmp/one.ts",
      version: 2,
      contentChanges: [{ text: "before!" }],
    });
    saveLspDocumentMock.mockResolvedValue({ uri: "file:///tmp/one.ts", version: 2 });
    const events: string[] = [];
    saveFileMock.mockImplementation(async () => {
      events.push("file-save");
      return savedFile();
    });
    saveLspDocumentMock.mockImplementation(async () => {
      events.push("did-save");
      return { uri: "file:///tmp/one.ts", version: 2 };
    });

    const rendered = await openOne();
    fireEvent.click(rendered.getByRole("button", { name: "edit /tmp/one.ts" }));
    fireEvent.keyDown(window, { key: "s", ctrlKey: true, isComposing: true });
    expect(saveFileMock).not.toHaveBeenCalled();
    expect(saveLspDocumentMock).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "s", ctrlKey: true });
    await waitFor(() => expect(saveLspDocumentMock).toHaveBeenCalledTimes(1));
    expect(events).toEqual(["file-save", "did-save"]);
  });

  it("uses the full reload boundary for clean external changes", async () => {
    loadSessionMock.mockResolvedValue({
      session: {
        version: 1,
        workspace_folder: "/tmp",
        docs: [],
        views: [[], []],
        active_view: 0,
        active_doc_by_view: [null, null],
        recent_files: [],
      },
      persistAllowed: true,
    });
    loadLspConfigMock.mockResolvedValue({
      config: {
        version: 1,
        enabled: true,
        workspace_root: "/tmp",
        server_by_language: {
          typescript: { kind: "local", installed_path: "/tools/tsls", args: [] },
        },
        custom_servers: [],
        update_policy: "manual",
      },
      persist_allowed: true,
      error: null,
    });
    openLspDocumentMock.mockResolvedValue({
      uri: "file:///tmp/one.ts",
      languageId: "typescript",
      version: 1,
      text: "before",
    });
    reloadLspDocumentMock.mockResolvedValue({
      uri: "file:///tmp/one.ts",
      version: 2,
      contentChanges: [{ text: "from disk" }],
    });

    await openOne();
    openFileMock.mockResolvedValue(openedFile("from disk"));
    await waitFor(() => expect(fileChangedHandlerRef.current).not.toBeNull());
    fileChangedHandlerRef.current?.({ payload: {
      path: "/tmp/one.ts",
      mtimeNanos: "2",
      contentHash: "hash-2",
      size: 9,
    } });
    await waitFor(() => expect(reloadLspDocumentMock).toHaveBeenCalledWith(
      "typescript",
      "file:///tmp/one.ts",
      "from disk",
    ));
    expect(changeLspDocumentMock).not.toHaveBeenCalled();
  });

  it("does not send didSave when the native file save fails", async () => {
    loadSessionMock.mockResolvedValue({
      session: {
        version: 1,
        workspace_folder: "/tmp",
        docs: [],
        views: [[], []],
        active_view: 0,
        active_doc_by_view: [null, null],
        recent_files: [],
      },
      persistAllowed: true,
    });
    loadLspConfigMock.mockResolvedValue({
      config: {
        version: 1,
        enabled: true,
        workspace_root: "/tmp",
        server_by_language: {
          typescript: { kind: "local", installed_path: "/tools/tsls", args: [] },
        },
        custom_servers: [],
        update_policy: "manual",
      },
      persist_allowed: true,
      error: null,
    });
    openLspDocumentMock.mockResolvedValue({
      uri: "file:///tmp/one.ts",
      languageId: "typescript",
      version: 1,
      text: "before",
    });
    saveFileMock.mockRejectedValue(new Error("disk is read-only"));

    const rendered = await openOne();
    fireEvent.click(rendered.getByRole("button", { name: "edit /tmp/one.ts" }));
    fireEvent.keyDown(window, { key: "s", ctrlKey: true });
    await waitFor(() => expect(rendered.getByRole("alert").textContent).toContain("읽기 전용 파일이라 저장할 수 없습니다."));
    expect(rendered.getByRole("alert").textContent).not.toContain("disk is read-only");
    expect(saveLspDocumentMock).not.toHaveBeenCalled();
  });

  it("does not leak a second native watch when reopening an existing document", async () => {
    const rendered = await openOne();
    openFileMock.mockResolvedValue(openedFile());
    const input = rendered.getByRole("textbox", { name: "열 파일 경로" });
    fireEvent.change(input, { target: { value: "/tmp/one.ts" } });
    fireEvent.click(rendered.getByRole("button", { name: "파일 열기" }));
    await waitFor(() => expect(rendered.getAllByRole("tab", { name: /one\.ts/ })).toHaveLength(1));
    expect(watchFileMock).toHaveBeenCalledTimes(1);
  });

  it("keeps the document open but discloses an unavailable file watcher", async () => {
    watchFileMock.mockRejectedValueOnce(new Error("watch capacity"));
    const rendered = await openOne();
    expect(rendered.getByRole("tab", { name: /one\.ts/ })).toBeTruthy();
    await waitFor(() => expect(rendered.getByRole("alert").textContent).toContain(
      "외부 변경 감시를 시작하지 못했습니다",
    ));
  });

  it("rolls back hydration watches across StrictMode effect lifetimes", async () => {
    loadSessionMock.mockResolvedValue({
      session: {
        version: 1,
        workspace_folder: null,
        docs: [{ id: "one", path: "/tmp/one.ts", cursor: 0, bookmarks: [] }],
        views: [["one"], []],
        active_view: 0,
        active_doc_by_view: ["one", null],
        recent_files: [],
      },
      persistAllowed: true,
    });
    openFileMock.mockResolvedValue(openedFile());
    const rendered = render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    await waitFor(() => expect(rendered.getByRole("tab", { name: /one\.ts/ })).toBeTruthy());
    expect(watchFileMock).toHaveBeenCalledTimes(1);
    rendered.unmount();
    await waitFor(() => expect(unwatchFileMock).toHaveBeenCalledTimes(1));
  });

  it("uses an accessible app dialog before closing a dirty tab", async () => {
    const rendered = await openOne();
    fireEvent.click(rendered.getByRole("button", { name: "edit /tmp/one.ts" }));
    const closeButton = rendered.getByRole("button", { name: "/tmp/one.ts 닫기" });
    closeButton.focus();
    fireEvent.click(closeButton);

    const dialog = rendered.getByRole("dialog", { name: "저장되지 않은 변경 사항" });
    expect(dialog).toBeTruthy();
    const cancelButton = rendered.getByRole("button", { name: "취소" });
    await waitFor(() => expect(document.activeElement).toBe(cancelButton));
    const saveButton = rendered.getByRole("button", { name: "저장 후 닫기" });
    saveButton.focus();
    fireEvent.keyDown(saveButton, { key: "Tab" });
    expect(document.activeElement).toBe(cancelButton);
    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => expect(rendered.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(closeButton));

    fireEvent.click(closeButton);
    fireEvent.click(rendered.getByRole("button", { name: "변경 내용 버리고 닫기" }));
    await waitFor(() => expect(rendered.queryByRole("tab", { name: /one\.ts/ })).toBeNull());
  });

  it("closes clean right-hand tabs immediately and queues dirty confirmations in tab order", async () => {
    const rendered = await openOne();
    await openAdditional(rendered, "/tmp/two.ts");
    await openAdditional(rendered, "/tmp/three.ts");
    await openAdditional(rendered, "/tmp/four.ts");
    fireEvent.click(rendered.getByRole("button", { name: "edit /tmp/two.ts" }));
    fireEvent.click(rendered.getByRole("button", { name: "edit /tmp/four.ts" }));

    fireEvent.contextMenu(rendered.getByRole("tab", { name: "one.ts" }), { clientX: 10, clientY: 10 });
    fireEvent.click(rendered.getByRole("menuitem", { name: "오른쪽 탭 모두 닫기" }));

    const firstDialog = rendered.getByRole("dialog", { name: "저장되지 않은 변경 사항" });
    expect(firstDialog.textContent).toContain("/tmp/two.ts");
    expect(firstDialog.textContent).toContain("이후 1개 대기");
    expect(rendered.queryByRole("tab", { name: "three.ts" })).toBeNull();
    fireEvent.click(rendered.getByRole("button", { name: "변경 내용 버리고 닫기" }));
    await waitFor(() => expect(rendered.getByRole("dialog").textContent).toContain("/tmp/four.ts"));
    fireEvent.click(rendered.getByRole("button", { name: "취소" }));

    expect(rendered.queryByRole("tab", { name: "two.ts" })).toBeNull();
    expect(rendered.getByRole("tab", { name: /four\.ts/ })).toBeTruthy();
  });

  it("renames a tab without losing its dirty buffer and migrates the native watch", async () => {
    const rendered = await openOne();
    fireEvent.click(rendered.getByRole("button", { name: "edit /tmp/one.ts" }));
    renameFileActionMock.mockResolvedValue({
      path: "/tmp/renamed.ts",
      mtimeNanos: "1",
      size: 6,
      contentHash: "hash-1",
    });
    const prompt = vi.spyOn(window, "prompt").mockReturnValue("renamed.ts");

    fireEvent.contextMenu(rendered.getByRole("tab", { name: /one\.ts/ }), { clientX: 10, clientY: 10 });
    fireEvent.click(rendered.getByRole("menuitem", { name: "이름 변경" }));

    await waitFor(() => expect(renameFileActionMock).toHaveBeenCalledWith({
      path: "/tmp/one.ts",
      mtimeNanos: "1",
      size: 6,
      contentHash: "hash-1",
    }, "renamed.ts"));
    await waitFor(() => expect(rendered.getByRole("tab", { name: /renamed\.ts/ })).toBeTruthy());
    expect(rendered.getByTestId("doc-text-/tmp/renamed.ts").textContent).toBe("before!");
    expect(unwatchFileMock).toHaveBeenCalledWith("/tmp/one.ts");
    expect(watchFileMock).toHaveBeenCalledWith("/tmp/renamed.ts");
    prompt.mockRestore();
  });

  it("requires explicit deletion confirmation and keeps the tab when the snapshot is stale", async () => {
    const rendered = await openOne();
    const confirm = vi.spyOn(window, "confirm").mockReturnValueOnce(false).mockReturnValueOnce(true);

    fireEvent.contextMenu(rendered.getByRole("tab", { name: /one\.ts/ }), { clientX: 10, clientY: 10 });
    fireEvent.click(rendered.getByRole("menuitem", { name: "삭제" }));
    expect(deleteFileActionMock).not.toHaveBeenCalled();

    deleteFileActionMock.mockRejectedValueOnce(new Error("파일을 삭제할 수 없습니다."));
    fireEvent.contextMenu(rendered.getByRole("tab", { name: /one\.ts/ }), { clientX: 10, clientY: 10 });
    fireEvent.click(rendered.getByRole("menuitem", { name: "삭제" }));
    await waitFor(() => expect(rendered.getByRole("alert").textContent).toContain("파일을 삭제할 수 없습니다."));
    expect(rendered.getByRole("tab", { name: /one\.ts/ })).toBeTruthy();
    expect(confirm.mock.calls[1][0]).toContain("미저장 변경 사항도 복구할 수 없습니다");
    confirm.mockRestore();
  });

  it("closes and unwatches a tab only after the validated delete succeeds", async () => {
    const rendered = await openOne();
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    fireEvent.contextMenu(rendered.getByRole("tab", { name: /one\.ts/ }), { clientX: 10, clientY: 10 });
    fireEvent.click(rendered.getByRole("menuitem", { name: "삭제" }));

    await waitFor(() => expect(deleteFileActionMock).toHaveBeenCalledWith({
      path: "/tmp/one.ts",
      mtimeNanos: "1",
      size: 6,
      contentHash: "hash-1",
    }));
    await waitFor(() => expect(rendered.queryByRole("tab", { name: /one\.ts/ })).toBeNull());
    expect(unwatchFileMock).toHaveBeenCalledWith("/tmp/one.ts");
    confirm.mockRestore();
  });

  it("copies the canonical tab path and delegates reveal to the validated backend action", async () => {
    const rendered = await openOne();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });

    fireEvent.contextMenu(rendered.getByRole("tab", { name: /one\.ts/ }), { clientX: 10, clientY: 10 });
    fireEvent.click(rendered.getByRole("menuitem", { name: "경로 복사" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("/tmp/one.ts"));

    fireEvent.contextMenu(rendered.getByRole("tab", { name: /one\.ts/ }), { clientX: 10, clientY: 10 });
    fireEvent.click(rendered.getByRole("menuitem", { name: "탐색기에서 열기" }));
    await waitFor(() => expect(revealFileActionMock).toHaveBeenCalledWith("/tmp/one.ts"));
  });

  it("guards explicit encoding reopen behind the dirty-choice dialog", async () => {
    const rendered = await openOne();
    fireEvent.click(rendered.getByRole("button", { name: "edit /tmp/one.ts" }));
    const reopen = rendered.getByRole("combobox", { name: "인코딩 다시 열기" });
    fireEvent.change(reopen, { target: { value: "utf16-le" } });

    expect(rendered.getByRole("dialog", { name: "인코딩 다시 열기" })).toBeTruthy();
    expect(openFileMock).toHaveBeenCalledTimes(1);
    fireEvent.click(rendered.getByRole("button", { name: "취소" }));
    expect(rendered.queryByRole("dialog", { name: "인코딩 다시 열기" })).toBeNull();

    fireEvent.change(reopen, { target: { value: "utf16-le" } });
    openFileMock.mockResolvedValue({ ...openedFile(), encoding: { encodingKind: "utf16Le", bom: false } });
    fireEvent.click(rendered.getByRole("button", { name: "변경 내용 버리고 다시 열기" }));
    await waitFor(() => expect(openFileMock).toHaveBeenCalledTimes(2));
    expect(openFileMock.mock.calls[1][1]).toEqual({ encodingKind: "utf16Le", bom: false });
  });

  it("validates an encoding conversion before changing save metadata", async () => {
    const rendered = await openOne();
    const conversion = rendered.getByRole("combobox", { name: "저장 인코딩" });
    fireEvent.change(conversion, { target: { value: "cp949" } });
    await waitFor(() => expect(validateEncodingMock).toHaveBeenCalledWith(
      "before",
      { encodingKind: "cp949", bom: false },
    ));
    expect((rendered.getByRole("combobox", { name: "줄바꿈 변환" }) as HTMLSelectElement).value).toBe("lf");
    fireEvent.change(rendered.getByRole("combobox", { name: "줄바꿈 변환" }), { target: { value: "crlf" } });
    expect((rendered.getByRole("combobox", { name: "줄바꿈 변환" }) as HTMLSelectElement).value).toBe("crlf");
  });

  it("exposes only the negotiated LSP editor capabilities", async () => {
    configureDiagnosticsApp({ definition: false, rename: false, formatting: false });
    const rendered = await openOne();

    expect((rendered.getByRole("button", { name: "정의" }) as HTMLButtonElement).disabled).toBe(true);
    expect((rendered.getByRole("button", { name: "참조" }) as HTMLButtonElement).disabled).toBe(false);
    expect((rendered.getByRole("button", { name: "이름 변경" }) as HTMLButtonElement).disabled).toBe(true);
    expect((rendered.getByRole("button", { name: "포맷" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("renders filtered definition and reference results with rejected workspace locations", async () => {
    configureDiagnosticsApp();
    const location = {
      uri: "file:///tmp/target.ts",
      range: { start: { line: 2, character: 0 }, end: { line: 2, character: 4 } },
      selectionRange: { start: { line: 2, character: 1 }, end: { line: 2, character: 4 } },
    };
    const response = {
      metadata: { uri: "file:///tmp/one.ts", version: 1 },
      value: { locations: [location], rejected: 1 },
      stale: false,
    };
    requestLspDefinitionMock.mockResolvedValue(response);
    requestLspReferencesMock.mockResolvedValue(response);
    const rendered = await openOne();

    fireEvent.click(rendered.getByRole("button", { name: "정의" }));
    expect(await rendered.findByRole("region", { name: "정의 결과" })).toBeTruthy();
    expect(rendered.getByText("작업 폴더 밖 결과 1개 제외")).toBeTruthy();
    expect(rendered.getByText("/tmp/target.ts")).toBeTruthy();
    fireEvent.click(rendered.getByRole("button", { name: "닫기" }));

    // The definition request owns the busy guard until its status resolves;
    // wait for the toolbar to re-enable before starting the next request.
    await waitFor(() => expect((rendered.getByRole("button", { name: "참조" }) as HTMLButtonElement).disabled).toBe(false));

    fireEvent.click(rendered.getByRole("button", { name: "참조" }));
    expect(await rendered.findByRole("region", { name: "참조 결과" })).toBeTruthy();
    expect(rendered.getByText("/tmp/target.ts")).toBeTruthy();
    expect(requestLspDefinitionMock).toHaveBeenCalledTimes(1);
    expect(requestLspReferencesMock).toHaveBeenCalledTimes(1);
  });

  it("applies formatting and rename buffers without autosaving", async () => {
    configureDiagnosticsApp();
    requestLspFormattingMock.mockResolvedValue({
      documents: [{ uri: "file:///tmp/one.ts", version: 2, text: "formatted" }],
    });
    requestLspRenameMock.mockResolvedValue({
      planId: "rename-1",
      files: [{
        path: "one.ts",
        ranges: [{
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 2 } },
          newText: "renamed",
        }],
        before: "formatted",
        after: "renamed",
      }],
    });
    applyLspRenameMock.mockResolvedValue({
      planId: "rename-1",
      success: true,
      rolledBack: false,
      files: [{
        path: "one.ts",
        status: "applied",
        mtimeNanos: "2",
        size: 7,
        contentHash: "hash-2",
        error: null,
      }],
      documents: [{ path: "one.ts", version: 3, text: "renamed" }],
      error: null,
    });
    const rendered = await openOne();
    const prompt = vi.spyOn(window, "prompt").mockReturnValue("renamed");

    fireEvent.click(rendered.getByRole("button", { name: "포맷" }));
    await waitFor(() => expect(rendered.getByTestId("doc-text-/tmp/one.ts").textContent).toBe("formatted"));
    fireEvent.click(rendered.getByRole("button", { name: "이름 변경" }));
    expect(await rendered.findByRole("dialog", { name: "여러 파일 이름 변경 미리보기" })).toBeTruthy();
    expect(rendered.getByText("one.ts")).toBeTruthy();
    fireEvent.click(rendered.getByRole("button", { name: /전체 적용/ }));
    await waitFor(() => expect(rendered.getByTestId("doc-text-/tmp/one.ts").textContent).toBe("renamed"));

    expect(saveFileMock).not.toHaveBeenCalled();
    expect(applyLspRenameMock).toHaveBeenCalledWith("rename-1");
    prompt.mockRestore();
  });

  it("reports a conflicting rename without advancing the LSP mirror", async () => {
    configureDiagnosticsApp();
    changeLspDocumentMock.mockResolvedValue({
      uri: "file:///tmp/one.ts",
      version: 2,
      contentChanges: [{ text: "before!" }],
    });
    requestLspRenameMock.mockResolvedValue({
      planId: "rename-1",
      files: [{
        path: "one.ts",
        ranges: [{
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 2 } },
          newText: "renamed",
        }],
        before: "before",
        after: "renamed",
      }],
    });
    applyLspRenameMock.mockResolvedValue({
      planId: "rename-1",
      success: false,
      rolledBack: false,
      files: [{
        path: "one.ts",
        status: "conflict",
        mtimeNanos: null,
        size: null,
        contentHash: null,
        error: "적용 전 파일이 변경되었습니다",
      }],
      documents: [],
      error: "적용 전 파일이 변경되어 이름 변경을 중단했습니다",
    });
    const rendered = await openOne();
    const prompt = vi.spyOn(window, "prompt").mockReturnValue("renamed");
    fireEvent.click(rendered.getByRole("button", { name: "이름 변경" }));
    await waitFor(() => expect(requestLspRenameMock).toHaveBeenCalledTimes(1));
    expect(await rendered.findByRole("dialog", { name: "여러 파일 이름 변경 미리보기" })).toBeTruthy();

    fireEvent.click(rendered.getByRole("button", { name: "edit /tmp/one.ts" }));
    fireEvent.click(rendered.getByRole("button", { name: /전체 적용/ }));
    await waitFor(() => expect(rendered.getByRole("dialog").textContent).toContain("충돌"));
    expect(rendered.getByTestId("doc-text-/tmp/one.ts").textContent).toBe("before!");
    expect(applyLspRenameMock).toHaveBeenCalledWith("rename-1");
    await waitFor(() => expect(changeLspDocumentMock).toHaveBeenCalledWith(
      "typescript",
      "file:///tmp/one.ts",
      "before!",
      true,
    ));

    // The rejected disk mutation must not make a version-2 diagnostic stale
    // after the user's local edit has already advanced the mirror.
    lspDiagnosticsHandlerRef.current?.({ payload: diagnosticEvent(2, "after-conflict") });
    const diagnostics = rendered.getAllByTestId(/^lsp-diagnostics-/u)[0];
    await waitFor(() => expect(diagnostics.textContent).toBe("after-conflict"));
    prompt.mockRestore();
  });

  it("discards a rename preview when the active document changes while the request is pending", async () => {
    configureDiagnosticsApp();
    const renameResponse = deferred<LspRenamePreview>();
    requestLspRenameMock.mockReturnValue(renameResponse.promise);
    changeLspDocumentMock.mockResolvedValue({
      uri: "file:///tmp/one.ts",
      version: 2,
      contentChanges: [{ text: "before!" }],
    });
    const rendered = await openOne();
    const prompt = vi.spyOn(window, "prompt").mockReturnValue("renamed");

    fireEvent.click(rendered.getByRole("button", { name: "이름 변경" }));
    await waitFor(() => expect(requestLspRenameMock).toHaveBeenCalledTimes(1));
    fireEvent.click(rendered.getByRole("button", { name: "edit /tmp/one.ts" }));
    renameResponse.resolve({
      planId: "rename-stale",
      files: [{
        path: "one.ts",
        ranges: [],
        before: "before",
        after: "renamed",
      }],
    });

    await waitFor(() => expect(discardLspRenameMock).toHaveBeenCalledWith("rename-stale"));
    expect(rendered.queryByRole("dialog", { name: "여러 파일 이름 변경 미리보기" })).toBeNull();
    expect(rendered.getByRole("alert").textContent).toContain("폐기했습니다");
    prompt.mockRestore();
  });

  it("does not apply a rename when cancellation wins during the mirror flush", async () => {
    configureDiagnosticsApp();
    const changeResponse = deferred<{
      uri: string;
      version: number;
      contentChanges: Array<{ text: string }>;
    }>();
    changeLspDocumentMock.mockReturnValue(changeResponse.promise);
    requestLspRenameMock.mockResolvedValue({
      planId: "rename-cancelled",
      files: [{
        path: "one.ts",
        ranges: [],
        before: "before",
        after: "renamed",
      }],
    });
    discardLspRenameMock.mockResolvedValue(true);
    const rendered = await openOne();
    const prompt = vi.spyOn(window, "prompt").mockReturnValue("renamed");

    fireEvent.click(rendered.getByRole("button", { name: "이름 변경" }));
    expect(await rendered.findByRole("dialog", { name: "여러 파일 이름 변경 미리보기" })).toBeTruthy();
    fireEvent.click(rendered.getByRole("button", { name: "edit /tmp/one.ts" }));
    await waitFor(() => expect(changeLspDocumentMock).toHaveBeenCalledTimes(1));
    fireEvent.click(rendered.getByRole("button", { name: /전체 적용/u }));
    fireEvent.click(rendered.getByRole("button", { name: "취소" }));
    expect(cancelLspRenameMock).toHaveBeenCalledWith("rename-cancelled");
    changeResponse.resolve({
      uri: "file:///tmp/one.ts",
      version: 2,
      contentChanges: [{ text: "before!" }],
    });

    await waitFor(() => expect(discardLspRenameMock).toHaveBeenCalledWith("rename-cancelled"));
    expect(applyLspRenameMock).not.toHaveBeenCalled();
    expect(rendered.queryByRole("dialog", { name: "여러 파일 이름 변경 미리보기" })).toBeNull();
    prompt.mockRestore();
  });

  it("keeps current CodeMirror diagnostics when a lower-version push arrives late", async () => {
    configureDiagnosticsApp();
    const rendered = await openOne();
    await waitFor(() => expect(lspDiagnosticsHandlerRef.current).not.toBeNull());
    const output = rendered.getAllByTestId(/^lsp-diagnostics-/u)[0];
    lspDiagnosticsHandlerRef.current?.({ payload: diagnosticEvent(1, "current") });
    await waitFor(() => expect(output.textContent).toBe("current"));

    lspDiagnosticsHandlerRef.current?.({ payload: diagnosticEvent(0, "late") });
    await waitFor(() => expect(output.textContent).toBe("current"));
    expect(rendered.queryByText(/최신 문서 상태와 맞지 않아 갱신 중입니다/)).toBeNull();
  });

  it("does not display a stale-only diagnostic event without a current cache", async () => {
    configureDiagnosticsApp();
    const rendered = await openOne();
    await waitFor(() => expect(lspDiagnosticsHandlerRef.current).not.toBeNull());
    const output = rendered.getAllByTestId(/^lsp-diagnostics-/u)[0];
    lspDiagnosticsHandlerRef.current?.({ payload: diagnosticEvent(0, "stale-only") });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(output.textContent).toBe("");
  });
});
