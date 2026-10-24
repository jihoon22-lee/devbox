/** 실제 데몬 RPC로 만든 `ApiBackend`. 변경 RPC는 호출마다 새 clientRequestId를 쓴다. */
import { type RpcClient, RpcError } from "../../../rpc/client";
import type { ClientStream, StreamHandlers } from "../../../rpc/client-streams";
import type { GrpcEvent, McpEvent, McpInput, RealtimeEvent, WsInput } from "../../../rpc/gen/rpc";
import type { ApiBackend, LineHandlers } from "./backend";
import { SendFailure } from "./backend";
import { SendCollector } from "./sendEvents";

const failure = (error: unknown) =>
  error instanceof RpcError ? new SendFailure(error.code, error.detail) : new SendFailure("internal");

async function rpc<T>(run: () => Promise<T>): Promise<T> {
  try {
    return await run();
  } catch (error) {
    throw failure(error);
  }
}

/** NDJSON 줄을 이벤트로. 잘못된 줄은 버리지 않고 실패 이벤트로 알린다. */
function lineReader<E>(onEvent: (event: E) => void) {
  const decoder = new TextDecoder();
  let pending = "";
  return (bytes: Uint8Array) => {
    pending += decoder.decode(bytes, { stream: true });
    let at = pending.indexOf("\n");
    while (at >= 0) {
      const line = pending.slice(0, at).trim();
      pending = pending.slice(at + 1);
      if (line) {
        let event: E;
        try {
          event = JSON.parse(line) as E;
        } catch {
          event = { type: "failed", error: { code: "internal" } } as E;
        }
        onEvent(event);
      }
      at = pending.indexOf("\n");
    }
  };
}

/** 스트림 하나를 세션으로. 입력은 NDJSON 한 줄씩 쓴다. */
function realtime<E extends RealtimeEvent | GrpcEvent | McpEvent, I = WsInput>(
  open: (handlers: StreamHandlers) => Promise<ClientStream>,
  handlers: LineHandlers<E>,
): Promise<{ send(input: I): void; close(): void }> {
  const read = lineReader(handlers.onEvent);
  const encoder = new TextEncoder();
  return rpc(async () => {
    const stream = await open({
      onData(bytes, ack) {
        try {
          read(bytes);
        } finally {
          ack();
        }
      },
      onEnd: handlers.onEnd,
    });
    return {
      send: (input) => stream.write(encoder.encode(`${JSON.stringify(input)}\n`)),
      close: () => stream.end(),
    };
  });
}

export function rpcBackend(client: RpcClient): ApiBackend {
  const id = () => ({ clientRequestId: crypto.randomUUID() });
  return {
    list: (root) => rpc(() => client.call("api.collections", { root })),
    read: (root, path) => rpc(() => client.call("api.collection_read", { root, path })),
    write: (root, path, text, expect) =>
      rpc(() => client.call("api.collection_write", { root, path, text, expect }, id())),
    remove: async (root, path, digest) => {
      await rpc(() => client.call("api.collection_delete", { root, path, digest }, id()));
    },
    review: async (root, path, digest) => {
      await rpc(() => client.call("api.collection_review", { root, path, digest }, id()));
    },
    history: (origin, limit) => rpc(() => client.call("api.history", { before: null, limit, origin })),
    historyBody: async (historyId) => {
      const entry = await rpc(() => client.call("api.history_get", { id: historyId }));
      const binary = atob(entry.bodyBase64);
      return new TextDecoder().decode(Uint8Array.from(binary, (c) => c.charCodeAt(0)));
    },
    secretNames: async () => (await rpc(() => client.call("secrets.list", {}))).map((s) => s.name),
    setSecret: async (name, value) => {
      await rpc(() => client.call("secrets.set", { name, value }, id()));
    },
    openWs: (params, handlers) => realtime((h) => client.openStream("api.ws_open", params, h), handlers),
    openSse: (params, handlers) => realtime((h) => client.openStream("api.sse_open", params, h), handlers),
    grpcDescribe: (params) => rpc(() => client.call("api.grpc_describe", params)),
    openGrpc: (params, handlers) => realtime((h) => client.openStream("api.grpc_call", params, h), handlers),
    openMcp: (params, handlers) =>
      realtime<McpEvent, McpInput>((h) => client.openStream("api.mcp_connect", params, h), handlers),
    send: (request, signal) =>
      new Promise((resolve, reject) => {
        const collector = new SendCollector();
        let stream: ClientStream | undefined;
        let settled = false;
        const settle = () => {
          if (settled) return;
          settled = true;
          signal.removeEventListener("abort", abort);
          if (signal.aborted) reject(new SendFailure("cancelled"));
          else if (collector.failure) reject(collector.failure);
          else if (!collector.ended) reject(new SendFailure("connection_lost"));
          else resolve(collector.finish());
        };
        // 화면이 스트림을 닫으면 데몬이 요청 future를 버린다(`api.cancel` 없음).
        const abort = () => {
          stream?.end();
          settle();
        };
        signal.addEventListener("abort", abort, { once: true });
        client
          .openStream("api.send", request, {
            onData(bytes, ack) {
              try {
                collector.push(bytes);
              } catch {
                collector.failure = new SendFailure("internal");
              } finally {
                ack();
              }
            },
            onEnd: settle,
          })
          .then((opened) => {
            stream = opened;
            if (signal.aborted || settled) opened.end();
          })
          .catch((error) => {
            settled = true;
            signal.removeEventListener("abort", abort);
            reject(failure(error));
          });
      }),
  };
}
