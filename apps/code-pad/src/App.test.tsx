import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  changeLspDocument,
  closeLspDocument,
  languageServerStatuses,
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
  saveFile,
  saveLspDocument,
  startLanguageServer,
  stopLanguageServer,
  unwatchFile,
  validateEncoding,
  watchFile,
} from "./api";
import type { LanguageServerStatus, LspDiagnosticsEvent, LspStatusEvent } from "./types";

const fileChangedHandlerRef: {
  current: ((event: { payload: { path: string; mtimeNanos: string; contentHash: string; size: number } }) => void) | null;
} = { current: null };
const lspDiagnosticsHandlerRef: {
  current: ((event: { payload: LspDiagnosticsEvent }) => void) | null;
} = { current: null };
const lspStatusHandlerRef: {
  current: ((event: { payload: LspStatusEvent }) => void) | null;
} = { current: null };

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, handler: unknown) => {
    if (event === "file-changed") fileChangedHandlerRef.current = handler as typeof fileChangedHandlerRef.current;
    if (event === "lsp/diagnostics") lspDiagnosticsHandlerRef.current = handler as typeof lspDiagnosticsHandlerRef.current;
    if (event === "lsp/status") lspStatusHandlerRef.current = handler as typeof lspStatusHandlerRef.current;
    return () => {
      if (event === "file-changed" && fileChangedHandlerRef.current === handler) fileChangedHandlerRef.current = null;
      if (event === "lsp/diagnostics" && lspDiagnosticsHandlerRef.current === handler) lspDiagnosticsHandlerRef.current = null;
      if (event === "lsp/status" && lspStatusHandlerRef.current === handler) lspStatusHandlerRef.current = null;
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
  canonicalizeWorkspace: vi.fn(),
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
  languageServerStatuses: vi.fn().mockResolvedValue([]),
  startLanguageServer: vi.fn().mockResolvedValue(undefined),
  stopLanguageServer: vi.fn().mockResolvedValue(undefined),
  openLspDocument: vi.fn(),
  reloadLspDocument: vi.fn(),
  changeLspDocument: vi.fn(),
  saveLspDocument: vi.fn(),
  closeLspDocument: vi.fn(),
  pullLspDiagnostics: vi.fn().mockResolvedValue({ metadata: { uri: "", version: 1 }, value: { uri: "", version: 1, diagnostics: [], origin: "pull" }, stale: false }),
  requestLspCompletion: vi.fn().mockResolvedValue({ metadata: { uri: "", version: 1 }, value: { isIncomplete: false, items: [] }, stale: false }),
  requestLspDefinition: vi.fn().mockResolvedValue({ metadata: { uri: "", version: 1 }, value: { locations: [], rejected: 0 }, stale: false }),
  requestLspFormatting: vi.fn().mockResolvedValue({ documents: [] }),
  requestLspHover: vi.fn().mockResolvedValue({ metadata: { uri: "", version: 1 }, value: null, stale: false }),
  requestLspReferences: vi.fn().mockResolvedValue({ metadata: { uri: "", version: 1 }, value: { locations: [], rejected: 0 }, stale: false }),
  requestLspRename: vi.fn().mockResolvedValue({ documents: [] }),
  restartLanguageServer: vi.fn().mockResolvedValue(undefined),
}));

const openFileMock = vi.mocked(openFile);
const saveFileMock = vi.mocked(saveFile);
const validateEncodingMock = vi.mocked(validateEncoding);
const loadSessionMock = vi.mocked(loadSession);
const watchFileMock = vi.mocked(watchFile);
const unwatchFileMock = vi.mocked(unwatchFile);
const loadLspConfigMock = vi.mocked(loadLspConfig);
const languageServerStatusesMock = vi.mocked(languageServerStatuses);
const startLanguageServerMock = vi.mocked(startLanguageServer);
const stopLanguageServerMock = vi.mocked(stopLanguageServer);
const openLspDocumentMock = vi.mocked(openLspDocument);
const reloadLspDocumentMock = vi.mocked(reloadLspDocument);
const changeLspDocumentMock = vi.mocked(changeLspDocument);
const saveLspDocumentMock = vi.mocked(saveLspDocument);
const closeLspDocumentMock = vi.mocked(closeLspDocument);
const pullLspDiagnosticsMock = vi.mocked(pullLspDiagnostics);
const requestLspDefinitionMock = vi.mocked(requestLspDefinition);
const requestLspFormattingMock = vi.mocked(requestLspFormatting);
const requestLspReferencesMock = vi.mocked(requestLspReferences);
const requestLspRenameMock = vi.mocked(requestLspRename);

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function openedFile(text = "before") {
  return {
    path: "/tmp/one.ts",
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
  startLanguageServerMock.mockReset().mockResolvedValue(undefined);
  stopLanguageServerMock.mockReset().mockResolvedValue(undefined);
  openLspDocumentMock.mockReset();
  reloadLspDocumentMock.mockReset();
  changeLspDocumentMock.mockReset();
  saveLspDocumentMock.mockReset();
  closeLspDocumentMock.mockReset();
  requestLspDefinitionMock.mockReset().mockResolvedValue({ metadata: { uri: "", version: 1 }, value: { locations: [], rejected: 0 }, stale: false });
  requestLspFormattingMock.mockReset().mockResolvedValue({ documents: [] });
  requestLspReferencesMock.mockReset().mockResolvedValue({ metadata: { uri: "", version: 1 }, value: { locations: [], rejected: 0 }, stale: false });
  requestLspRenameMock.mockReset().mockResolvedValue({ documents: [] });
  fileChangedHandlerRef.current = null;
  lspDiagnosticsHandlerRef.current = null;
  lspStatusHandlerRef.current = null;
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

describe("App editor shell operations", () => {
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
    await waitFor(() => expect(rendered.getByRole("alert").textContent).toContain("disk is read-only"));
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
    fireEvent.click(rendered.getByRole("button", { name: "/tmp/one.ts 닫기" }));

    const dialog = rendered.getByRole("dialog", { name: "저장되지 않은 변경 사항" });
    expect(dialog).toBeTruthy();
    expect(rendered.queryByRole("button", { name: "취소" })).toBeTruthy();
    fireEvent.click(rendered.getByRole("button", { name: "취소" }));
    expect(rendered.queryByRole("dialog")).toBeNull();

    fireEvent.click(rendered.getByRole("button", { name: "/tmp/one.ts 닫기" }));
    fireEvent.click(rendered.getByRole("button", { name: "변경 내용 버리고 닫기" }));
    await waitFor(() => expect(rendered.queryByRole("tab", { name: /one\.ts/ })).toBeNull());
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
      documents: [{ uri: "file:///tmp/one.ts", version: 3, text: "renamed" }],
    });
    const rendered = await openOne();
    const prompt = vi.spyOn(window, "prompt").mockReturnValue("renamed");

    fireEvent.click(rendered.getByRole("button", { name: "포맷" }));
    await waitFor(() => expect(rendered.getByTestId("doc-text-/tmp/one.ts").textContent).toBe("formatted"));
    fireEvent.click(rendered.getByRole("button", { name: "이름 변경" }));
    await waitFor(() => expect(rendered.getByTestId("doc-text-/tmp/one.ts").textContent).toBe("renamed"));

    expect(saveFileMock).not.toHaveBeenCalled();
    prompt.mockRestore();
  });

  it("rejects a conflicting rename without advancing the LSP mirror", async () => {
    configureDiagnosticsApp();
    changeLspDocumentMock.mockResolvedValue({
      uri: "file:///tmp/one.ts",
      version: 2,
      contentChanges: [{ text: "before!" }],
    });
    const rename = deferred<{ documents: Array<{ uri: string; version: number; text: string }> }>();
    requestLspRenameMock.mockReturnValue(rename.promise);
    const rendered = await openOne();
    const prompt = vi.spyOn(window, "prompt").mockReturnValue("renamed");
    fireEvent.click(rendered.getByRole("button", { name: "이름 변경" }));
    await waitFor(() => expect(requestLspRenameMock).toHaveBeenCalledTimes(1));

    fireEvent.click(rendered.getByRole("button", { name: "edit /tmp/one.ts" }));
    rename.resolve({
      documents: [{ uri: "file:///tmp/one.ts", version: 3, text: "renamed" }],
    });
    await waitFor(() => expect(rendered.getByRole("alert").textContent).toContain("문서가 변경되었습니다"));
    expect(rendered.getByTestId("doc-text-/tmp/one.ts").textContent).toBe("before!");
    await waitFor(() => expect(changeLspDocumentMock).toHaveBeenCalledWith(
      "typescript",
      "file:///tmp/one.ts",
      "before!",
      true,
    ));

    // The rejected version-3 mutation must not make a version-2 diagnostic
    // stale after the user's local edit has already advanced the mirror.
    lspDiagnosticsHandlerRef.current?.({ payload: diagnosticEvent(2, "after-conflict") });
    const diagnostics = rendered.getAllByTestId(/^lsp-diagnostics-/u)[0];
    await waitFor(() => expect(diagnostics.textContent).toBe("after-conflict"));
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
