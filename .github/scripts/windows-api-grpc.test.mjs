import { mkdtemp, writeFile, rm } from "node:fs/promises";
import * as runner from "./windows-api-grpc.mjs";
import { scenarioModuleContract } from "./windows-api-user-flow-contract.test-support.mjs";
scenarioModuleContract(runner, ["GRPC-01"], "windows-api-grpc.mjs");

import assert from "node:assert/strict";
import test from "node:test";
import path from "node:path";
import { tmpdir } from "node:os";
test("lazy route readiness precedes exactly one first target input", async () => {
  const root = path.join(tmpdir(), "devbox-suite-delivery-readiness", "Suite UI Fixture");
  const events = [];
  let ready = false;
  const result = await runner.run({
    root,
    fixtureRoot: root,
    installationKey: "a".repeat(64),
    namespace: path.join(path.dirname(root), `com.devbox.v08.apistudio.i${"a".repeat(64)}`),
    sourceSha: "b".repeat(40),
    fixtureSha: "b".repeat(40),
    artifactDigests: Object.fromEntries(Array.from({ length: 7 }, (_, i) => [`fixture${i}`, "c".repeat(64)])),
    cdp: { evaluate: async () => true },
    nativeCall: async () => {},
    ui: {
      press: async () => {},
      confirmDialog: async () => {},
      closeOwnedWindow: async () => {},
      screenshot: async () => "/owned/failure.png",
      waitForTarget: async (target) => {
        assert.deepEqual(target, { role: "tab", name: "gRPC" });
        events.push("ready");
        ready = true;
      },
      click: async (target) => {
        if (target.name === "프로토콜") {
          events.push("navigate");
          return;
        }
        assert.equal(target.name, "gRPC");
        assert.equal(ready, true);
        events.push("input");
        throw new Error("fixture stop after first input");
      },
      fill: async (target) => {
        throw new Error("unexpected fill");
      },
    },
  });
  assert.equal(result[0].status, "FAIL");
  assert.deepEqual(events, ["navigate", "ready", "input"]);
});

test("exact export read preserves ENOENT even when an alternate default file exists", async () => {
  const original = Object.assign(new Error("owned exact output missing"), { code: "ENOENT" });
  const diagnostic = { expectedExists: false, parentExists: true, fileCount: 1, defaultNameExists: true };
  let observed;
  await assert.rejects(
    runner.readExactGrpcExport(
      "/owned/expected.json",
      (value) => {
        observed = value;
      },
      async () => {
        throw original;
      },
      async () => diagnostic,
    ),
    (error) => error === original,
  );
  assert.equal(observed, diagnostic);
});
test("export inspection failure never replaces original exact read error", async () => {
  const original = Object.freeze(new Error("owned original"));
  let observed;
  await assert.rejects(
    runner.readExactGrpcExport(
      "/owned/expected.json",
      (value) => {
        observed = value;
      },
      async () => {
        throw original;
      },
      async () => {
        throw new Error("inspection unavailable");
      },
    ),
    (error) => error === original,
  );
  assert.deepEqual(observed, { unavailable: true });
});

test("owned export observation exposes only fixed booleans and bounded file count", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "devbox-grpc-export-"));
  try {
    await writeFile(path.join(root, "grpc-exchange.json"), "synthetic-private-content");
    const observed = await runner.grpcExportObservation(path.join(root, "expected.json"));
    assert.deepEqual(observed, { expectedExists: false, parentExists: true, fileCount: 1, defaultNameExists: true });
    assert.ok(!JSON.stringify(observed).includes("synthetic"));
    await Promise.all(Array.from({ length: 32 }, (_, i) => writeFile(path.join(root, `${i}.json`), "owned")));
    assert.equal((await runner.grpcExportObservation(path.join(root, "expected.json"))).fileCount, null);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("failure observation callback cannot replace the original read error", async () => {
  const original = Object.freeze(new Error("owned read failure"));
  await assert.rejects(
    runner.readExactGrpcExport(
      "/owned/expected.json",
      () => {
        throw new Error("observer failed");
      },
      async () => {
        throw original;
      },
      async () => ({ expectedExists: false }),
    ),
    (error) => error === original,
  );
});

test("method transition waits for matching editor shape and prior result reset before one input", async () => {
  const events = [];
  const observations = [false, false, true, true];
  await runner.prepareGrpcInvocation(
    {
      cdp: {
        evaluate: async () => {
          events.push("observe");
          return observations.shift();
        },
      },
      ui: {
        fill: async () => events.push("fill"),
        waitForTarget: async () => events.push("enabled"),
        click: async () => events.push("click"),
      },
    },
    "Client",
  );
  assert.deepEqual(events, ["observe", "observe", "observe", "fill", "observe", "enabled", "click"]);
});

test("selected Client alone is insufficient while Unary template/result remain", () => {
  const nodes = {
    '[aria-label="gRPC method"]': { value: "fixture.Partial.Client" },
    ".grpc-method-card code": { textContent: "fixture.Partial/Client" },
    '[aria-label="gRPC ProtoJSON request"]': { value: '{"text":"fixture"}' },
    ".grpc-result": {},
  };
  const document = { querySelector: (selector) => nodes[selector] };
  assert.equal(runner.grpcMethodReady(document, "Client"), false);
  delete nodes[".grpc-result"];
  assert.equal(runner.grpcMethodReady(document, "Client"), false);
  nodes['[aria-label="gRPC ProtoJSON request"]'].value = '[{"text":""}]';
  assert.equal(runner.grpcMethodReady(document, "Client"), true);
  nodes['[aria-label="gRPC method"]'].value = "fixture.Partial.Bidi";
  assert.equal(runner.grpcMethodReady(document, "Client"), false);
});
