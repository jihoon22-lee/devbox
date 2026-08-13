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
  reloadLspDocument,
  saveFile,
  saveLspDocument,
  startLanguageServer,
  stopLanguageServer,
  unwatchFile,
  validateEncoding,
  watchFile,
} from "./api";

const fileChangedHandlerRef: {
  current: ((event: { payload: { path: string; mtimeNanos: string; contentHash: string; size: number } }) => void) | null;
} = { current: null };

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_event: string, handler: typeof fileChangedHandlerRef.current) => {
    fileChangedHandlerRef.current = handler;
    return () => {
      if (fileChangedHandlerRef.current === handler) fileChangedHandlerRef.current = null;
    };
  }),
}));

vi.mock("./components/DocHost", () => ({
  default: (props: {
    docs: Array<{ id: string; path: string; text: string }>;
    onChange: (docId: string, text: string) => void;
    onReplaceCommandReady?: (docId: string, command: (() => boolean) | null) => void;
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
  fileChangedHandlerRef.current = null;
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
});
