import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { loadSession, openFile, saveFile, unwatchFile, watchFile } from "./api";

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
}));

const openFileMock = vi.mocked(openFile);
const saveFileMock = vi.mocked(saveFile);
const loadSessionMock = vi.mocked(loadSession);
const watchFileMock = vi.mocked(watchFile);
const unwatchFileMock = vi.mocked(unwatchFile);

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
});
