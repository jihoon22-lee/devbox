import type { Transport } from "@codemirror/lsp-client";
import type { ClientStream, StreamHandlers } from "../../../rpc/client-streams";
import type { OpenLsp } from "../../../rpc/gen/rpc";

export interface StreamClient {
  openStream(method: "lsp.open", params: OpenLsp, handlers: StreamHandlers): Promise<ClientStream>;
}
const MAX_PACKET = 4 * 1024 * 1024;
const MAX_BODY = 2 * 1024 * 1024;
const encoder = new TextEncoder();

/** One immutable stream generation. Reconnection creates a new transport and client. */
export class LspTransport implements Transport {
  fullText?: (uri: string, version: number) => string | undefined;
  intercept?: (message: Record<string, unknown>) => boolean;
  private handlers = new Set<(json: string) => void>();
  private stream?: ClientStream;
  private live = true;
  private decoder = new TextDecoder("utf-8", { fatal: true });
  private buffer = "";
  private bufferedBytes = 0;
  private constructor(
    private lease: (uri: string) => string | undefined,
    private ended: () => void,
  ) {}

  static async open(rpc: StreamClient, params: OpenLsp, lease: (uri: string) => string | undefined, ended: () => void) {
    const transport = new LspTransport(lease, ended);
    try {
      const stream = await rpc.openStream("lsp.open", params, {
        onData(bytes, ack) {
          try {
            transport.receive(bytes);
          } catch {
            transport.close();
          } finally {
            ack();
          }
        },
        onEnd() {
          transport.close();
        },
      });
      if (!transport.live) {
        stream.end();
        throw new Error("LSP stream unavailable");
      }
      transport.stream = stream;
      return transport;
    } catch {
      transport.close();
      throw new Error("LSP stream unavailable");
    }
  }
  subscribe(handler: (json: string) => void) {
    this.handlers.add(handler);
  }
  unsubscribe(handler: (json: string) => void) {
    this.handlers.delete(handler);
  }
  close() {
    if (!this.live) return;
    this.live = false;
    this.buffer = "";
    this.handlers.clear();
    this.stream?.end();
    this.ended();
  }
  send(json: string) {
    if (!this.live || !this.stream) throw new Error("LSP stream unavailable");
    try {
      if (encoder.encode(json).length > MAX_BODY) throw new Error();
      const message = JSON.parse(json);
      const uri = message.params?.textDocument?.uri;
      if (message.method === "textDocument/didChange" && this.fullText) {
        const text = this.fullText(uri, message.params.textDocument.version);
        if (text === undefined) throw new Error();
        message.params.contentChanges = [{ text }];
        json = JSON.stringify(message);
        if (encoder.encode(json).length > MAX_BODY) throw new Error();
      }
      const leaseId = typeof uri === "string" ? this.lease(uri) : undefined;
      const bytes = encoder.encode(`${JSON.stringify({ json, ...(leaseId ? { leaseId } : {}) })}\n`);
      if (bytes.length > MAX_PACKET) throw new Error();
      this.stream.write(bytes, { throwOnFailure: true });
    } catch {
      this.close();
      throw new Error("LSP stream unavailable");
    }
  }
  private receive(bytes: Uint8Array) {
    if (!this.live) return;
    // Split on ASCII newline before UTF-8 decoding to bound every frame independently.
    let start = 0;
    for (let i = 0; i <= bytes.length; i++) {
      if (i !== bytes.length && bytes[i] !== 10) continue;
      const piece = bytes.subarray(start, i);
      this.bufferedBytes += piece.length;
      if (this.bufferedBytes > MAX_PACKET) throw new Error();
      this.buffer += this.decoder.decode(piece, { stream: true });
      if (i < bytes.length) {
        this.buffer += this.decoder.decode();
        const packet = JSON.parse(this.buffer);
        if (typeof packet?.json !== "string" || encoder.encode(packet.json).length > MAX_BODY) throw new Error();
        // Validate without ever copying parsing exceptions or protocol bodies to logs.
        const message = JSON.parse(packet.json);
        if (!message || typeof message !== "object" || Array.isArray(message)) throw new Error();
        this.buffer = "";
        this.bufferedBytes = 0;
        // Third-party editor handlers may log rejected requests. Keep error bodies out.
        const delivered = message.error
          ? JSON.stringify({
              ...message,
              error: {
                code: Number.isInteger(message.error.code) ? message.error.code : -32603,
                message: "Language server request failed",
              },
            })
          : packet.json;
        const consumed = this.intercept?.(message);
        for (const handler of consumed ? [] : this.handlers) {
          if (!this.live) break;
          handler(delivered);
        }
      }
      start = i + 1;
    }
  }
}
