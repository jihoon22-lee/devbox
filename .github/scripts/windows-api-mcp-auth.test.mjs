import * as runner from "./windows-api-mcp-auth.mjs";
import { scenarioModuleContract } from "./windows-api-user-flow-contract.test-support.mjs";
import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, mkdir, rm } from "node:fs/promises";
import path from "node:path";
import { tmpdir } from "node:os";
test("Protocols lazy route becomes accessible before the MCP tab is clicked", async () => {
  const scratch = await mkdtemp(path.join(tmpdir(), "devbox-suite-delivery-"));
  const root = path.join(scratch, "Suite UI Fixture");
  await mkdir(root);
  const events = [];
  let ready = false;
  try {
    const result = await runner.run({
      root,
      fixtureRoot: root,
      installationKey: "a".repeat(64),
      namespace: path.join(scratch, `com.devbox.v08.apistudio.i${"a".repeat(64)}`),
      sourceSha: "b".repeat(40),
      fixtureSha: "b".repeat(40),
      artifactDigests: Object.fromEntries(Array.from({ length: 7 }, (_, index) => [`fixture${index}`, "c".repeat(64)])),
      cdp: { evaluate: async () => true },
      nativeCall: async () => "synthetic-ciphertext",
      ui: {
        press: async () => {},
        confirmDialog: async () => {},
        closeOwnedWindow: async () => {},
        click: async (target) => {
          events.push(target.name);
          if (target.name === "MCP") assert.equal(ready, true, "MCP is absent while Protocols lazy route loads");
        },
        waitForTarget: async (target) => {
          assert.equal(target.name, "MCP");
          events.push("ready");
          ready = true;
        },
        fill: async () => {
          throw new Error("fixture stop after ready MCP tab");
        },
        screenshot: async () => "/owned/failure.png",
      },
    });
    assert.equal(result[0].status, "FAIL");
    assert.deepEqual(events, ["프로토콜", "ready", "MCP"]);
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
});
test("OAuth refresh waits for the requested grant option before one selection", async () => {
  const scratch = await mkdtemp(path.join(tmpdir(), "devbox-suite-delivery-"));
  const root = path.join(scratch, "Suite UI Fixture");
  await mkdir(root);
  const events = [];
  let ready = false;
  let reads = 0;
  try {
    const result = await runner.run({
      root,
      fixtureRoot: root,
      installationKey: "a".repeat(64),
      namespace: path.join(scratch, `com.devbox.v08.apistudio.i${"a".repeat(64)}`),
      sourceSha: "b".repeat(40),
      fixtureSha: "b".repeat(40),
      artifactDigests: Object.fromEntries(Array.from({ length: 7 }, (_, index) => [`fixture${index}`, "c".repeat(64)])),
      cdp: { evaluate: async () => (++reads === 1 ? -1 : 0) },
      nativeCall: async () => "synthetic-ciphertext",
      ui: {
        press: async () => {},
        confirmDialog: async () => {},
        closeOwnedWindow: async () => {},
        click: async (target) => {
          events.push(target.name);
          if (target.name === "OAuth grant") {
            assert.equal(reads, 2);
            throw new Error("fixture stop at one grant selection");
          }
          if (target.name === "MCP") assert.equal(ready, true, "MCP is absent while Protocols lazy route loads");
        },
        waitForTarget: async (target) => {
          assert.equal(target.name, "MCP");
          events.push("ready");
          ready = true;
        },
        fill: async () => {
          return;
        },
        screenshot: async () => "/owned/failure.png",
      },
    });
    assert.equal(result[0].status, "FAIL");
    assert.deepEqual(events, ["프로토콜", "ready", "MCP", "OAuth grant 새로 고침", "OAuth grant"]);
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
});
scenarioModuleContract(runner, ["AUTH-01", "AUTH-02"], "windows-api-mcp-auth.mjs");

test("modern MCP tool list fixture includes required private cache metadata", () => {
  const discovery = runner.toolReply({ id: "discover", method: "server/discover" });
  assert.deepEqual(discovery.result.supportedVersions, ["2026-07-28"]);
  const reply = runner.toolReply({ id: "list", method: "tools/list" });
  assert.equal(reply.result.resultType, "complete");
  assert.equal(reply.result.ttlMs, 0);
  assert.equal(reply.result.cacheScope, "private");
  assert.equal(reply.result.tools[0].name, "synthetic_echo");
});
