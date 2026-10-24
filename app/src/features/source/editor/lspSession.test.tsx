import { undo } from "@codemirror/commands";
import { EditorView } from "@codemirror/view";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { StreamHandlers } from "../../../rpc/client-streams";
import type { Methods } from "../../../rpc/gen/rpc";
import { EditorHost } from "../../documents/EditorHost";
import { memoryJournal } from "../../documents/offlineJournal";
import { useDocuments } from "../../documents/store";
import { FakeDocumentsDaemon } from "../../documents/test-daemon";
import { useSource } from "../store";
import { useEditorLsp } from "./lsp";
import { connectLsp, type EditorLspSession } from "./lspSession";
import { projectUris } from "./lspWorkspace";

const holder = vi.hoisted(() => ({ client: null as unknown, state: "ready" }));
vi.mock("../../../rpc/RpcProvider", () => ({ useRpcSession: () => holder }));
const key = { root: "project:p", path: "a.ts" };
const uris = projectUris("p", "/owned");
let stop = () => {},
  session: EditorLspSession | undefined;
beforeAll(() =>
  Object.defineProperties(Range.prototype, {
    getClientRects: { configurable: true, value: () => [] },
    getBoundingClientRect: { configurable: true, value: () => new DOMRect() },
  }),
);
afterEach(() => {
  cleanup();
  session?.close();
  session = undefined;
  stop();
});
class LspDaemon extends FakeDocumentsDaemon {
  saveGate?: Promise<void>;
  override async call<M extends keyof Methods>(method: M, params: Methods[M]["params"]): Promise<Methods[M]["result"]> {
    if (method === "projects.list")
      return {
        rev: "1",
        items: [{ id: "p", path: "/owned", name: "p", favorite: false, addedMs: 0 }],
      } as Methods[M]["result"];
    const result = await super.call(method, params);
    if (method === "documents.save") await this.saveGate;
    return result;
  }
  streams: { handlers: StreamHandlers; sent: { json: string; leaseId?: string }[]; end: () => void }[] = [];
  results = new Map<string, unknown>();
  deferred = new Set<string>();
  async openStream(_method: "lsp.open", _params: unknown, handlers: StreamHandlers) {
    const sent: { json: string; leaseId?: string }[] = [];
    const index = this.streams.length;
    const stream = { handlers, sent, end: () => handlers.onEnd() };
    this.streams.push(stream);
    return {
      streamId: index + 1,
      result: { streamId: index + 1 },
      end: stream.end,
      write: (bytes: Uint8Array) => {
        const packet = JSON.parse(new TextDecoder().decode(bytes));
        sent.push(packet);
        const request = JSON.parse(packet.json);
        if (request.id === undefined || request.method === undefined || this.deferred.has(request.method)) return;
        const result =
          request.method === "initialize"
            ? {
                capabilities: {
                  textDocumentSync: 2,
                  documentFormattingProvider: true,
                  renameProvider: true,
                  definitionProvider: true,
                },
              }
            : this.results.get(request.method);
        queueMicrotask(() => this.reply(index, { jsonrpc: "2.0", id: request.id, result }));
      },
    };
  }
  reply(index: number, value: unknown) {
    this.streams[index]!.handlers.onData(
      new TextEncoder().encode(`${JSON.stringify({ json: JSON.stringify(value) })}\n`),
      () => {},
    );
  }
  messages(index = 0) {
    return this.streams[index]!.sent.map((p) => ({ ...JSON.parse(p.json), leaseId: p.leaseId }));
  }
}
async function setup() {
  const daemon = new LspDaemon();
  daemon.write(key, "😀a" + " ".repeat(2000));
  stop = useDocuments.getState().connect(daemon, { journal: memoryJournal() });
  useDocuments.getState().open(key, { kind: "code" });
  await useDocuments.getState().settled();
  session = await connectLsp(
    daemon,
    { projectId: "p", language: "typescript" },
    uris,
    () => {},
    () => {},
  );
  const rendered = render(<EditorHost docKey={key} extensions={session.extension(key, "typescript")} />);
  const input = await screen.findByRole("textbox");
  await waitFor(() => expect(input).toHaveAttribute("contenteditable", "true"));
  return { daemon, input, rendered, view: EditorView.findFromDOM(input)! };
}
test("initialize precedes didOpen; reconnect reopens current lease and ignores old replies", async () => {
  const { daemon, rendered } = await setup();
  await waitFor(() => expect(daemon.messages().map((m) => m.method)).toContain("textDocument/didOpen"));
  expect(
    daemon
      .messages()
      .slice(0, 3)
      .map((m) => m.method),
  ).toEqual(["initialize", "initialized", "textDocument/didOpen"]);
  daemon.streams[0]!.end();
  expect(session!.current()).toBe(false);
  daemon.disconnect();
  daemon.reconnect();
  await useDocuments.getState().settled();
  session = await connectLsp(
    daemon,
    { projectId: "p", language: "typescript" },
    uris,
    () => {},
    () => {},
  );
  rendered.rerender(<EditorHost docKey={key} extensions={session.extension(key, "typescript")} />);
  await waitFor(() => expect(daemon.messages(1).some((m) => m.method === "textDocument/didOpen")).toBe(true));
  const open = daemon.messages(1).find((m) => m.method === "textDocument/didOpen");
  expect(open.leaseId).toBe(useDocuments.getState().find(key)!.leaseId);
  expect(open.leaseId).not.toBe("L1");
  daemon.reply(0, {
    method: "textDocument/publishDiagnostics",
    params: {
      uri: uris.uri(key),
      diagnostics: [
        { range: { start: { line: 0, character: 0 }, end: { line: 0, character: 2 } }, severity: 1, message: "stale" },
      ],
    },
  });
  expect(document.querySelector(".cm-lintRange-error")).toBeNull();
});
test("ACK A emits A while full-text didChange retains current B and never sends ranges", async () => {
  const { daemon, view } = await setup();
  await act(async () => {
    view.dispatch({ changes: { from: 2, to: 3, insert: "A" } });
    useDocuments.getState().save(key);
    view.dispatch({ changes: { from: 2, to: 3, insert: "B" } });
    await useDocuments.getState().settled();
  });
  const save = daemon.messages().find((m) => m.method === "textDocument/didSave");
  expect(save.params.text).toBe("😀A" + " ".repeat(2000));
  const change = daemon
    .messages()
    .filter((m) => m.method === "textDocument/didChange")
    .at(-1);
  expect(change.params.contentChanges).toEqual([{ text: "😀B" + " ".repeat(2000) }]);
  expect(useDocuments.getState().find(key)).toMatchObject({ text: "😀B" + " ".repeat(2000), state: "Dirty" });
});
test("real formatting and rename keymaps apply through the document store without saving", async () => {
  const { daemon, input } = await setup();
  const edits = [{ range: { start: { line: 0, character: 2 }, end: { line: 0, character: 3 } }, newText: "A" }];
  daemon.results.set("textDocument/formatting", edits);
  fireEvent.keyDown(input, { key: "F", code: "KeyF", keyCode: 70, altKey: true, shiftKey: true });
  await waitFor(() => expect(useDocuments.getState().find(key)?.text.startsWith("😀A")).toBe(true));
  daemon.results.set("textDocument/rename", {
    documentChanges: [{ textDocument: { uri: uris.uri(key), version: 1 }, edits: [{ ...edits[0], newText: "B" }] }],
  });
  const view = EditorView.findFromDOM(input)!;
  act(() => view.dispatch({ selection: { anchor: 2 } }));
  fireEvent.keyDown(input, { key: "F2", code: "F2" });
  const name = await screen.findByRole("textbox", { name: /새 이름/ });
  fireEvent.change(name, { target: { value: "B" } });
  fireEvent.submit(name.closest("form")!);
  await waitFor(() => expect(useDocuments.getState().find(key)?.text.startsWith("😀B")).toBe(true));
  expect(daemon.count("documents.save")).toBe(0);
});

test("current diagnostics underline the editor and disconnect clears old-generation marks", async () => {
  const { daemon } = await setup();
  await act(async () =>
    daemon.reply(0, {
      method: "textDocument/publishDiagnostics",
      params: {
        uri: uris.uri(key),
        version: 0,
        diagnostics: [
          {
            range: { start: { line: 0, character: 2 }, end: { line: 0, character: 3 } },
            severity: 1,
            message: "current diagnostic",
          },
        ],
      },
    }),
  );
  await waitFor(() => expect(document.querySelector(".cm-lintRange-error")).not.toBeNull());
  act(() => session!.close());
  expect(document.querySelector(".cm-lintRange-error")).toBeNull();
});
test.each(["lease", "closed", "generation"])("late successful ACK after %s change sends no didSave", async (kind) => {
  const { daemon, view } = await setup();
  let release = () => {};
  daemon.saveGate = new Promise<void>((resolve) => {
    release = resolve;
  });
  await act(async () => {
    view.dispatch({ changes: { from: 2, to: 3, insert: "A" } });
    useDocuments.getState().save(key);
    await Promise.resolve();
  });
  act(() => {
    if (kind === "lease")
      daemon.emit("documents.lease_lost", { key, leaseId: useDocuments.getState().find(key)!.leaseId! });
    if (kind === "closed") useDocuments.getState().close(key, "discard");
    if (kind === "generation") session!.close();
  });
  await act(async () => {
    release();
    await useDocuments.getState().settled();
  });
  expect(daemon.messages().filter((m) => m.method === "textDocument/didSave")).toHaveLength(0);
});
test("F12 opens the returned project file and a late formatting reply cannot edit a closed generation", async () => {
  const { daemon, input } = await setup();
  const target = { ...key, path: "other.ts" };
  daemon.write(target, "const target = 1");
  daemon.results.set("textDocument/definition", {
    uri: uris.uri(target),
    range: { start: { line: 0, character: 6 }, end: { line: 0, character: 12 } },
  });
  fireEvent.keyDown(input, { key: "F12", code: "F12" });
  await waitFor(() => expect(useSource.getState().layout.tabs.some((t) => t.key.path === "other.ts")).toBe(true));
  daemon.deferred.add("textDocument/formatting");
  fireEvent.keyDown(input, { key: "F", keyCode: 70, code: "KeyF", shiftKey: true, altKey: true });
  await waitFor(() => expect(daemon.messages().some((m) => m.method === "textDocument/formatting")).toBe(true));
  const request = daemon.messages().find((m) => m.method === "textDocument/formatting")!;
  session!.close();
  daemon.reply(0, {
    id: request.id,
    result: [{ range: { start: { line: 0, character: 2 }, end: { line: 0, character: 3 } }, newText: "stale" }],
  });
  expect(useDocuments.getState().find(key)?.text.startsWith("😀a")).toBe(true);
});
test("the actual hook automatically creates a fresh client after stream termination", async () => {
  const daemon = new LspDaemon();
  daemon.write(key, "const a = 1;");
  holder.client = daemon;
  stop = useDocuments.getState().connect(daemon, { journal: memoryJournal() });
  useDocuments.getState().open(key, { kind: "code" });
  await useDocuments.getState().settled();
  function Editor() {
    const lsp = useEditorLsp(key);
    return <EditorHost docKey={key} extensions={lsp.extension} />;
  }
  render(<Editor />);
  await waitFor(() => expect(daemon.streams.length).toBe(1));
  await waitFor(() => expect(daemon.messages().some((m) => m.method === "textDocument/didOpen")).toBe(true));
  act(() => daemon.streams[0]!.end());
  await waitFor(() => expect(daemon.streams.length).toBe(2), { timeout: 3000 });
  await waitFor(() => expect(daemon.messages(1).map((m) => m.method)).toContain("textDocument/didOpen"));
  expect(
    daemon
      .messages(1)
      .slice(0, 3)
      .map((m) => m.method),
  ).toEqual(["initialize", "initialized", "textDocument/didOpen"]);
});

test("cached editor diagnostics clear on generation close while undo and new diagnostics survive remount", async () => {
  const { daemon, view, rendered } = await setup();
  act(() => {
    view.dispatch({ changes: { from: 2, to: 3, insert: "A" } });
    session!.client.sync();
  });
  const diagnostics = (version: number) => ({
    method: "textDocument/publishDiagnostics",
    params: {
      uri: uris.uri(key),
      version,
      diagnostics: [
        {
          range: { start: { line: 0, character: 2 }, end: { line: 0, character: 3 } },
          severity: 1,
          message: "current",
        },
      ],
    },
  });
  await act(async () => daemon.reply(0, diagnostics(1)));
  await waitFor(() => expect(document.querySelector(".cm-lintRange-error")).not.toBeNull());
  rendered.unmount();
  session!.close();
  session = await connectLsp(
    daemon,
    { projectId: "p", language: "typescript" },
    uris,
    () => {},
    () => {},
  );
  render(<EditorHost docKey={key} extensions={session.extension(key, "typescript")} />);
  const input = await screen.findByRole("textbox");
  expect(document.querySelector(".cm-lintRange-error")).toBeNull();
  await act(async () => daemon.reply(1, diagnostics(0)));
  await waitFor(() => expect(document.querySelector(".cm-lintRange-error")).not.toBeNull());
  act(() => {
    expect(undo(EditorView.findFromDOM(input)!)).toBe(true);
  });
  expect(useDocuments.getState().find(key)?.text.startsWith("😀a")).toBe(true);
});

test("queued full-text notifications keep each emitted version bound to its own text", async () => {
  const { daemon, view } = await setup();
  await act(async () => {
    view.dispatch({ changes: { from: 2, to: 3, insert: "A" } });
    session!.client.sync();
    view.dispatch({ changes: { from: 2, to: 3, insert: "B" } });
    session!.client.sync();
  });
  const changes = daemon.messages().filter((m) => m.method === "textDocument/didChange");
  expect(changes.map((m) => [m.params.textDocument.version, m.params.contentChanges[0].text.slice(0, 3)])).toEqual([
    [1, "😀A"],
    [2, "😀B"],
  ]);
  expect(session!.current()).toBe(true);
});

test("R1 daemon-canonical punctuation URI reaches installed-client diagnostics and workspace edits", async () => {
  const daemon = new LspDaemon();
  const sourceKey = { root: "project:p", path: "a'!()*.ts" };
  const scoped = projectUris("p", "/owned/한😀 'root!");
  const canonical = "file:///owned/%ED%95%9C%F0%9F%98%80%20%27root%21/a%27%21%28%29%2A.ts";
  daemon.write(sourceKey, "😀a");
  stop = useDocuments.getState().connect(daemon, { journal: memoryJournal() });
  useDocuments.getState().open(sourceKey, { kind: "code" });
  await useDocuments.getState().settled();
  session = await connectLsp(
    daemon,
    { projectId: "p", language: "typescript" },
    scoped,
    () => {},
    () => {},
  );
  render(<EditorHost docKey={sourceKey} extensions={session.extension(sourceKey, "typescript")} />);
  await screen.findByRole("textbox");
  await act(async () =>
    daemon.reply(0, {
      method: "textDocument/publishDiagnostics",
      params: {
        uri: canonical,
        version: 0,
        diagnostics: [
          {
            range: { start: { line: 0, character: 2 }, end: { line: 0, character: 3 } },
            severity: 1,
            message: "canonical diagnostic",
          },
        ],
      },
    }),
  );
  await waitFor(() => expect(document.querySelector(".cm-lintRange-error")).not.toBeNull());
  expect(daemon.messages().find((m) => m.method === "textDocument/didOpen").params.textDocument.uri).toBe(canonical);
  await act(async () =>
    daemon.reply(0, {
      id: "canonical-edit",
      method: "workspace/applyEdit",
      params: {
        edit: {
          changes: {
            [canonical]: [
              { range: { start: { line: 0, character: 2 }, end: { line: 0, character: 3 } }, newText: "b" },
            ],
          },
        },
      },
    }),
  );
  await waitFor(() => expect(useDocuments.getState().find(sourceKey)?.text).toBe("😀b"));
  await waitFor(() => expect(daemon.messages().find((m) => m.id === "canonical-edit")?.result.applied).toBe(true));
  expect(daemon.count("documents.save")).toBe(0);
});
