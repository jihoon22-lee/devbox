import assert from "node:assert/strict";
import test from "node:test";
import { installFileIpcDiagnostic } from "./suite-file-ipc-diagnostic.mjs";
test("observer preserves invoke identity, arguments, response and rejection while retaining only closed evidence", async () => {
  const response = {
    operation: {
      provenance: { product: "workspace", component: "workspace.commands", requestId: "secret-id", revision: 19 },
      outcome: { state: "succeeded" },
    },
    value: { path: "secret-path", context: null },
  };
  const failure = { code: "secret-credential", message: "secret-document" };
  const input = {
    request: {
      method: { kind: "openReceivedFile", reference: "secret-reference" },
      header: { requestId: "secret-id", context: null },
    },
  };
  const calls = [];
  const bridge = {
    invoke: async function (...args) {
      calls.push({ receiver: this, args });
      if (calls.length === 2) throw failure;
      return response;
    },
  };
  globalThis.window = { __TAURI_INTERNALS__: bridge };
  const original = bridge.invoke;
  try {
    installFileIpcDiagnostic(19);
    assert.equal(await bridge.invoke("plugin:suite|connection", input), response);
    await assert.rejects(bridge.invoke("plugin:suite|connection", input), (error) => error === failure);
    assert.equal(calls[0].receiver, bridge);
    assert.equal(calls[0].args[1], input);
    const records = window.__devboxFileIpcDiagnostic.records;
    assert.deepEqual(records[0], {
      result: "returned",
      state: "succeeded",
      code: null,
      provenanceValid: true,
      pathShapeValid: true,
      contextMatchesRequest: true,
    });
    assert.deepEqual(records[1], { result: "rejected", code: "unknown" });
    assert.ok(!JSON.stringify(records).includes("secret"));
    for (let index = 0; index < 20; index++) await bridge.invoke("plugin:suite|connection", input);
    assert.equal(records.length, 16);
    window.__devboxFileIpcDiagnostic.restore();
    assert.equal(bridge.invoke, original);
    assert.equal(window.__devboxFileIpcDiagnostic, undefined);
  } finally {
    delete globalThis.window;
  }
});

test("diagnostic projection failure cannot replace an original successful response", async () => {
  const response = Object.defineProperty({}, "operation", {
    get() {
      throw new Error("secret projection");
    },
  });
  const bridge = { invoke: async () => response };
  globalThis.window = { __TAURI_INTERNALS__: bridge };
  try {
    installFileIpcDiagnostic(19);
    assert.equal(
      await bridge.invoke("plugin:suite|connection", { request: { method: { kind: "openReceivedFile" }, header: {} } }),
      response,
    );
    assert.deepEqual(window.__devboxFileIpcDiagnostic.records, []);
    window.__devboxFileIpcDiagnostic.restore();
  } finally {
    delete globalThis.window;
  }
});
