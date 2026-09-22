import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer, createConnection, Socket } from "node:net";
import { once } from "node:events";
import { observeControlSocket } from "./windows-api-control.mjs";

test("an owned TCP reset retires the socket without terminating the control fixture", { timeout: 5000 }, async () => {
  const server = createServer(); let client, accepted;
  const state = { expectedDisconnects: 0, failure: null };
  try {
    server.listen(0, "127.0.0.1"); await once(server, "listening");
    const connected = new Promise(resolve => server.once("connection", socket => { observeControlSocket(socket, state); resolve(socket); }));
    client = createConnection({ host: "127.0.0.1", port: server.address().port });
    client.on("error", () => {});
    await once(client, "connect"); accepted = await connected;
    const closed = new Promise(resolve => accepted.once("close", resolve));
    client.resetAndDestroy(); await closed;
    assert.equal(state.expectedDisconnects, 1);
    assert.equal(state.failure, null);
  } finally {
    client?.destroy(); accepted?.destroy(); await new Promise(resolve => server.close(resolve));
  }
});

test("unexpected socket faults remain failures rather than successful cancellation", () => {
  const socket = new Socket(), state = { expectedDisconnects: 0, failure: null };
  observeControlSocket(socket, state);
  const failure = Object.assign(new Error("unexpected socket fault"), { code: "EACCES" });
  socket.emit("error", failure);
  assert.equal(state.failure, failure); assert.equal(state.expectedDisconnects, 0);
  socket.destroy();
});
