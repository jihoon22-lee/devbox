import { hoverTooltips, LSPClient, serverCompletion, serverDiagnostics, signatureHelp } from "@codemirror/lsp-client";
import type { RpcClient } from "../../../rpc/client";
import type { DocKey, OpenLsp } from "../../../rpc/gen/rpc";
import { useDocuments } from "../../documents/store";
import { commandContext, lspCommands } from "./lspCommands";
import { contentConfig } from "./lspContent";
import { SourceLspFiles } from "./lspFiles";
import { LspTransport, type StreamClient } from "./lspTransport";
import { applyWorkspaceEdit, type EditContext, type ProjectUris } from "./lspWorkspace";

export async function connectLsp(
  rpc: Pick<RpcClient, "call"> & StreamClient,
  params: OpenLsp,
  uris: ProjectUris,
  onEnd: () => void,
  report: (message: string) => void,
  signal?: AbortSignal,
) {
  let live = true;
  let transport: LspTransport | undefined;
  let client: LSPClient | undefined;
  let files: SourceLspFiles | undefined;
  let offSaved: (() => void) | undefined;
  const close = () => {
    if (!live) return;
    live = false;
    signal?.removeEventListener("abort", close);
    offSaved?.();
    files?.dispose();
    client?.disconnect();
    transport?.close();
    onEnd();
  };
  signal?.addEventListener("abort", close, { once: true });
  if (signal?.aborted) {
    close();
    throw new Error("LSP stream unavailable");
  }
  const getDoc = (uri: string) => {
    try {
      return useDocuments.getState().find(uris.key(uri));
    } catch {
      return undefined;
    }
  };
  try {
    transport = await LspTransport.open(rpc, params, (uri) => getDoc(uri)?.leaseId, close);
    if (!live) {
      transport.close();
      throw new Error("LSP stream unavailable");
    }
    const content = contentConfig((message) => {
      if (live) report(message);
    });
    client = new LSPClient({
      ...content,
      rootUri: uris.rootUri,
      workspace: (owner) => {
        files = new SourceLspFiles(owner, uris, close);
        return files;
      },
      extensions: [
        ...(content.extensions ?? []),
        serverCompletion(),
        hoverTooltips(),
        signatureHelp(),
        serverDiagnostics(),
      ],
    });
    const owner = client,
      stream = transport,
      workspace = files!;
    const context: EditContext = { rpc, uris, current: () => live, version: (uri) => workspace.getFile(uri)?.version };
    // The library defers notifications through initializing.then(). Capture full text
    // now, before a subsequent sync advances the workspace to another version.
    owner.notification = (method: string, params: unknown) => {
      if (!live) return;
      try {
        let body = params;
        if (method === "textDocument/didChange") {
          const change = params as { textDocument: { uri: string; version: number } };
          const file = workspace.getFile(change.textDocument.uri);
          if (!file || file.version !== change.textDocument.version) throw new Error();
          body = { ...change, contentChanges: [{ text: file.doc.toString() }] };
        }
        const json = JSON.stringify({ jsonrpc: "2.0", method, params: body });
        void owner.initializing
          .then(() => {
            if (live) stream.send(json);
          })
          .catch(close);
      } catch {
        close();
      }
    };
    stream.intercept = (message) => {
      if (message.method !== "workspace/applyEdit" || message.id === undefined) return false;
      const incoming = message.params as { edit?: unknown } | undefined;
      const snapshot = commandContext(context);
      void applyWorkspaceEdit(snapshot, incoming?.edit)
        .then(
          () => ({ applied: true }),
          () => ({ applied: false, failureReason: "파일 상태가 바뀌었거나 지원하지 않는 변경입니다." }),
        )
        .then((result) => {
          if (live) {
            try {
              stream.send(JSON.stringify({ jsonrpc: "2.0", id: message.id, result }));
            } catch {
              close();
            }
          }
        });
      return true;
    };
    client.connect(transport);
    await client.initializing;
    if (!live) throw new Error("LSP stream unavailable");
    offSaved = useDocuments.getState().subscribeSaved((ack) => {
      const doc = useDocuments.getState().docs[ack.id];
      if (!live || !doc || doc.leaseId !== ack.leaseId || doc.policy.kind !== "code") return;
      let uri: string;
      try {
        uri = uris.uri(doc.key);
      } catch {
        return;
      }
      if (!workspace.getFile(uri)) return;
      try {
        owner.sync();
        owner.notification("textDocument/didSave", { textDocument: { uri }, text: ack.text });
      } catch {
        close();
      }
    });
    return {
      client: owner,
      files: workspace,
      close,
      current: () => live,
      extension(key: DocKey, language: string) {
        const uri = uris.uri(key);
        return [
          owner.plugin(uri, language),
          lspCommands(owner, uri, context, (message) => {
            if (live) report(message);
          }),
        ];
      },
    };
  } catch {
    close();
    throw new Error("LSP stream unavailable");
  }
}
export type EditorLspSession = Awaited<ReturnType<typeof connectLsp>>;
