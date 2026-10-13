import { createConnection } from "node:net";
import packageJson from "../package.json" with { type: "json" };
import { RpcClient, type Transport, type TransportState } from "../src/rpc/client";

/** Fixture setup uses the same framed client over its owned Unix socket. UI uses the gateway. */
export async function connectFixtureRpc(socketPath: string, instance: string) {
  const frames = new Set<(frame: Uint8Array) => void>();
  const states = new Set<(state: TransportState) => void>();
  let socket: ReturnType<typeof createConnection> | undefined;
  const emit = (state: TransportState) => {
    for (const listener of states) listener(state);
  };
  const transport: Transport = {
    connect() {
      let buffer = Buffer.alloc(0);
      socket = createConnection(socketPath);
      socket.on("connect", () => emit("open"));
      socket.on("close", () => emit("closed"));
      socket.on("error", () => emit("closed"));
      socket.on("data", (data) => {
        buffer = Buffer.concat([buffer, data]);
        while (buffer.length >= 4) {
          const size = buffer.readUInt32LE(0) + 4;
          if (size > 16 * 1024 * 1024 + 9 || size < 9) {
            socket?.destroy();
            return;
          }
          if (buffer.length < size) return;
          const frame = buffer.subarray(0, size);
          buffer = buffer.subarray(size);
          for (const listener of frames) listener(frame);
        }
      });
    },
    close() {
      socket?.destroy();
    },
    send(frame) {
      if (!socket || socket.destroyed) throw new Error("Fixture socket closed");
      socket.write(frame);
    },
    onFrame(listener) {
      frames.add(listener);
      return () => {
        frames.delete(listener);
      };
    },
    onState(listener) {
      states.add(listener);
      return () => {
        states.delete(listener);
      };
    },
  };
  const client = new RpcClient(transport, { version: packageJson.version, instance, client: "test" });
  try {
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => {
        off();
        reject(new Error("Fixture RPC Welcome timed out"));
      }, 5000);
      const off = client.onState((state) => {
        if (state === "ready") {
          clearTimeout(timer);
          off();
          resolve();
        } else if (state === "versionMismatch") {
          clearTimeout(timer);
          off();
          reject(new Error("Fixture RPC version mismatch"));
        }
      });
      client.start();
    });
    return client;
  } catch (error) {
    client.stop();
    throw error;
  }
}
