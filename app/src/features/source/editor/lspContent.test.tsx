import { CompletionContext } from "@codemirror/autocomplete";
import {
  hoverTooltips,
  LSPClient,
  serverCompletionSource,
  showSignatureHelp,
  signatureHelp,
} from "@codemirror/lsp-client";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { contentConfig } from "./lspContent";

const external = vi.hoisted(() => vi.fn());
vi.mock("../../../shell/external", () => ({ openExternal: external }));
const malicious =
  '<img src="https://fixture.invalid/leak" onerror="globalThis.LSP_EXEC=1"><script>globalThis.LSP_EXEC=2</script><iframe src="https://fixture.invalid/frame"></iframe><style>body{background:url(https://fixture.invalid/css)}</style>\n\n[bad](javascript:alert(1)) [cmd](command:run) [good](https://example.com) **normal** `code`';
const markup = { kind: "markdown", value: malicious };
let view: EditorView;
let client: LSPClient;
let incoming: (text: string) => void;
let methods: string[];
beforeAll(() => {
  Object.defineProperties(Range.prototype, {
    getClientRects: { configurable: true, value: () => [new DOMRect(0, 0, 100, 20)] },
    getBoundingClientRect: { configurable: true, value: () => new DOMRect(0, 0, 100, 20) },
  });
});
afterEach(() => {
  view?.destroy();
  client?.disconnect();
  document.body.replaceChildren();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  external.mockClear();
});
let network: ReturnType<typeof vi.fn>, navigation: ReturnType<typeof vi.spyOn>;
async function setup(showMessage = vi.fn()) {
  network = vi.fn();
  vi.stubGlobal("fetch", network);
  navigation = vi.spyOn(window, "open").mockReturnValue(null);
  methods = [];
  client = new LSPClient({
    ...contentConfig(showMessage),
    extensions: [...(contentConfig().extensions ?? []), hoverTooltips({ hoverTime: 0 }), signatureHelp()],
  });
  client.connect({
    subscribe(fn) {
      incoming = fn;
    },
    unsubscribe() {},
    send(text) {
      const request = JSON.parse(text);
      methods.push(request.method);
      if (request.id === undefined) return;
      const result =
        request.method === "initialize"
          ? {
              capabilities: {
                textDocumentSync: 1,
                hoverProvider: true,
                completionProvider: {},
                signatureHelpProvider: {},
              },
            }
          : request.method === "textDocument/completion"
            ? [{ label: "foo", documentation: markup }]
            : request.method === "textDocument/hover"
              ? { contents: markup }
              : { signatures: [{ label: "foo(x)", documentation: markup }] };
      queueMicrotask(() => incoming(JSON.stringify({ jsonrpc: "2.0", id: request.id, result })));
    },
  });
  await client.initializing;
  view = new EditorView({
    parent: document.body,
    state: EditorState.create({ doc: "foo", extensions: [client.plugin("file:///p/a.ts", "typescript")] }),
  });
}
function safe(host: Element) {
  expect(host.querySelector("img,script,iframe,object,style,[onerror],[href]")).toBeNull();
  expect(host.innerHTML).not.toMatch(/javascript:|command:|fixture.invalid/);
  expect(host.querySelector("strong")?.textContent).toBe("normal");
  expect(host.querySelector("code")?.textContent).toBe("code");
  const link = host.querySelector('a[data-href="https://example.com"]')!;
  expect(link).not.toBeNull();
  const event = new MouseEvent("click", { bubbles: true, cancelable: true });
  link.dispatchEvent(event);
  expect(event.defaultPrevented).toBe(true);
  expect(external).toHaveBeenCalledWith("https://example.com");
  expect(network).not.toHaveBeenCalled();
  expect(navigation).not.toHaveBeenCalled();
  expect((globalThis as { LSP_EXEC?: number }).LSP_EXEC).toBeUndefined();
}
test("real completion documentation sink is sanitized and opens allowed links explicitly", async () => {
  await setup();
  const result = await serverCompletionSource(new CompletionContext(view.state, 3, true, view));
  const option = result!.options[0]!;
  expect(typeof option.info).toBe("function");
  const info = await (option.info as Function)(option);
  view.dom.appendChild(info);
  safe(info);
});
test("real signature documentation sink is sanitized", async () => {
  await setup();
  view.focus();
  expect(showSignatureHelp(view)).toBe(true);
  await waitFor(() => expect(document.querySelector(".cm-lsp-signature-documentation")).not.toBeNull());
  safe(document.querySelector(".cm-lsp-signature-documentation")!);
});
test("real hover documentation sink is sanitized", async () => {
  await setup();
  vi.spyOn(view, "posAtCoords").mockReturnValue(1);
  vi.spyOn(view, "coordsAtPos").mockReturnValue({ left: 0, right: 100, top: 0, bottom: 20 });
  vi.spyOn(view.contentDOM, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 0, 100, 20));
  fireEvent.mouseMove(view.contentDOM, { clientX: 10, clientY: 10 });
  await waitFor(() => expect(methods).toContain("textDocument/hover"));
  await waitFor(() => expect(document.querySelector(".cm-lsp-hover-tooltip")).not.toBeNull());
  safe(document.querySelector(".cm-lsp-hover-tooltip")!);
});
test("server log sentinel is consumed and showMessage remains plain text", async () => {
  const showMessage = vi.fn();
  await setup(showMessage);
  const spies = [vi.spyOn(console, "log"), vi.spyOn(console, "warn"), vi.spyOn(console, "error")];
  incoming(JSON.stringify({ method: "window/logMessage", params: { type: 1, message: "PRIVATE_LSP_LOG_SENTINEL" } }));
  for (const spy of spies) expect(spy).not.toHaveBeenCalled();
  incoming(JSON.stringify({ method: "window/showMessage", params: { type: 1, message: malicious } }));
  expect(showMessage).toHaveBeenCalledExactlyOnceWith(malicious);
  expect(document.querySelector("script,img,iframe")).toBeNull();
});
