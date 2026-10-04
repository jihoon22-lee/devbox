import vm from "node:vm";
import { summarizeEvidence } from "./suite-user-flow-evidence.mjs";
import * as runner from "./windows-api-mcp-auth.mjs";
import { scenarioModuleContract } from "./windows-api-user-flow-contract.test-support.mjs";
import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, mkdir, rm, readFile } from "node:fs/promises";
import path from "node:path";
import { tmpdir } from "node:os";
test("OAuth refresh follows completed disconnect and performs each input once", async () => {
  const events = [];
  let ready = false;
  await runner.disconnectAndRefreshGrants({
    ui: {
      waitForTarget: async (target) => {
        assert.equal(target.name, "OAuth grant 새로 고침");
        assert.deepEqual(events, ["연결 해제"]);
        ready = true;
      },
      click: async (target) => {
        if (target.name === "OAuth grant 새로 고침") assert.equal(ready, true);
        events.push(target.name);
      },
    },
  });
  assert.deepEqual(events, ["연결 해제", "OAuth grant 새로 고침"]);
});
test("OAuth fixture reseeding restarts the native owner before any grant refresh", async () => {
  const scratch = await mkdtemp(path.join(tmpdir(), "devbox-oauth-cache-"));
  const file = path.join(scratch, "mcp-grants.json");
  const events = [];
  let nativeCache = "old grants";
  try {
    await runner.reseedOAuthGrantFixture(
      {
        restart: async () => {
          events.push("restart");
          nativeCache = await readFile(file, "utf8");
        },
      },
      file,
      "fresh synthetic grants",
    );
    assert.equal(nativeCache, "fresh synthetic grants");
    assert.deepEqual(events, ["restart"]);
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
});
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
  let grantReady = false;
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
            assert.equal(grantReady, true);
            throw new Error("fixture stop at one grant selection");
          }
          if (target.name === "MCP") assert.equal(ready, true, "MCP is absent while Protocols lazy route loads");
        },
        waitForTarget: async (target) => {
          if (target.name === "OAuth grant") {
            grantReady = true;
            events.push("grant-ready");
            return;
          }
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
    assert.deepEqual(events, ["프로토콜", "ready", "MCP", "OAuth grant 새로 고침", "grant-ready", "OAuth grant"]);
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

test("disconnect after refresh waits for enabled control before one input", async () => {
  const events = [];
  await runner.disconnectMcpWhenReady({
    ui: {
      waitForTarget: async (target) => {
        assert.equal(target.name, "연결 해제");
        events.push("ready");
      },
      click: async (target) => {
        assert.equal(target.name, "연결 해제");
        assert.deepEqual(events, ["ready"]);
        events.push("click");
      },
    },
  });
  assert.deepEqual(events, ["ready", "click"]);
});

test("Grant selection observes enabled control after refresh before resolving current option", async () => {
  const events = [];
  let ready = false;
  await runner.selectOAuthGrant(
    {
      ui: {
        waitForTarget: async (target) => {
          assert.deepEqual(target, { role: "combobox", name: "OAuth grant" });
          events.push("ready");
          ready = true;
        },
        click: async () => {
          assert.equal(ready, true);
          events.push("click");
        },
        press: async (key) => events.push(key),
      },
      cdp: {
        evaluate: async () => {
          assert.equal(ready, true);
          events.push("current-options");
          return 1;
        },
      },
    },
    "owned-grant-B",
  );
  assert.deepEqual(events, ["ready", "current-options", "click", "Home", "ArrowDown", "Enter"]);
});

test("Candidate API continues independent modules after AUTH failure while final gate rejects", async () => {
  const matrix = JSON.parse(await readFile(new URL("./suite-user-flow-matrix.json", import.meta.url), "utf8")).filter(
    (row) => /^windows-api-(http-semantics|mcp-auth|environments|webhooks|grpc|transforms)\.mjs$/.test(row.module),
  );
  const source = await readFile(new URL("./windows-api-user-flows.mjs", import.meta.url), "utf8");
  const body = source
    .slice(source.indexOf("export async function runApiUserFlows"), source.indexOf("if (process.argv[1]"))
    .replace("export async function", "async function")
    .replace('new URL("./suite-user-flow-matrix.json", import.meta.url)', '"matrix"')
    .replace("await import(new URL(module, import.meta.url))", "await loadRunner(module)");
  const visited = [];
  const firstFailure = { name: "Error", message: "owned AUTH first failure" };
  const context = {
    diagnosticOnly: false,
    httpCompletedMs: 1,
    sourceSha: "a".repeat(40),
    fixtureSha: "a".repeat(40),
    artifactDigests: { fixture: "b".repeat(64) },
    ui: { closeOwnedWindow: async () => {} },
    close: async () => {},
  };
  let saved;
  const result = vm.runInNewContext(`${body} runApiUserFlows()`, {
    assert,
    readFile: async () => JSON.stringify(matrix),
    createApiUserFlowContext: async () => context,
    observeProductPerformance: async ({ workload }) => workload(),
    loadRunner: async (module) => ({
      run: async () => {
        visited.push(module);
        return matrix
          .filter((row) => row.module === module)
          .map((row) => ({
            ...context,
            ui: undefined,
            close: undefined,
            id: row.id,
            evidenceKind: "packaged-ui",
            status: row.id === "AUTH-02" ? "FAIL" : "PASS",
            assertions: ["owned observation"],
            screenshotPaths: ["/owned/image.png"],
            failureCode: row.id === "AUTH-02" ? "AUTH-02-assertion-failed" : null,
            error: row.id === "AUTH-02" ? firstFailure : undefined,
          }));
      },
    }),
    writeUserFlowResults: async (_, rows) => {
      saved = rows;
    },
    summarizeEvidence,
    preserveUserFlowFailure: async () => {},
    console,
  });
  await assert.rejects(result, /"ready":false/);
  assert.equal(visited.length, 6);
  assert.equal(saved.length, 11);
  assert.equal(saved.find((row) => row.id === "AUTH-02").error, firstFailure);
  assert.equal(saved.find((row) => row.id === "AUTH-02").status, "FAIL");
  assert.ok(saved.some((row) => row.id === "TRANSFORM-01"));
});
