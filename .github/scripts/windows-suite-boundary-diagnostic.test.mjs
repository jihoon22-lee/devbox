import assert from "node:assert/strict";
import test from "node:test";
import {
  exerciseOwnedTrayBoundary,
  runBoundaryDiagnostic,
  executeBoundaryProbes,
} from "./windows-suite-boundary-diagnostic.mjs";
test("tray probe uses only native connected readiness and actual tray flow, then closes", async () => {
  const context = { root: "owned-root", installationKey: "owned-key" };
  const calls = [];
  const app = {
    ...context,
    close: async () => {
      calls.push("close");
    },
  };
  const result = await exerciseOwnedTrayBoundary(context, app, {
    status: async () => {
      calls.push("status");
      return "connected";
    },
    tray: async (scope, owned) => {
      assert.equal(scope, context);
      assert.equal(owned, app);
      calls.push("tray");
      return { assertions: ["tray"] };
    },
    observe: async (check) => assert.equal(await check(), true),
  });
  assert.deepEqual(result, { assertions: ["tray"] });
  assert.deepEqual(calls, ["status", "tray", "close"]);
});
test("tray probe finally closes and preserves original failure even if close also fails", async () => {
  const original = Object.freeze(new Error("tray unavailable"));
  let closes = 0;
  const scope = { root: "owned-root", installationKey: "owned-key" };
  await assert.rejects(
    exerciseOwnedTrayBoundary(
      scope,
      {
        ...scope,
        close: async () => {
          closes++;
          throw new Error("close");
        },
      },
      {
        status: async () => "connected",
        observe: async (check) => check(),
        tray: async () => {
          throw original;
        },
      },
    ),
    (error) => error === original,
  );
  assert.equal(closes, 1);
});
test("boundary modes reject unknown mode and ordinary local host before probing", async () => {
  await assert.rejects(runBoundaryDiagnostic("all"), /Exact boundary/);
  if (process.platform !== "win32") await assert.rejects(runBoundaryDiagnostic("tray"), /win32/);
});

test("tray failure is preserved before independent checkpoint-only delivery probe", async () => {
  const events = [];
  const identity = { sourceSha: "a".repeat(40), diagnosticOnly: true, promotionEvidence: false };
  const receipts = await executeBoundaryProbes(identity, {
    tray: async () => {
      events.push("tray");
      throw new Error("original tray error");
    },
    delivery: async () => {
      events.push("delivery");
      return [{ id: "CHECKPOINT-DIAGNOSTIC", status: "PASS" }];
    },
    persist: async (receipt) => {
      events.push(receipt.mode + "-saved");
      assert.equal(receipt.diagnosticOnly, true);
      assert.equal(receipt.promotionEvidence, false);
    },
  });
  assert.deepEqual(events, ["tray", "tray-saved", "delivery", "delivery-saved"]);
  assert.deepEqual(
    receipts.map((receipt) => receipt.status),
    ["FAIL", "PASS"],
  );
  assert.equal(receipts[0].error.message, "original tray error");
});

test("boundary rejects promotion identity before any probe", async () => {
  let calls = 0;
  for (const identity of [{}, { diagnosticOnly: true, promotionEvidence: true }]) {
    await assert.rejects(
      executeBoundaryProbes(identity, {
        tray: async () => {
          calls++;
        },
        delivery: async () => {
          calls++;
        },
        persist: async () => {
          calls++;
        },
      }),
    );
  }
  assert.equal(calls, 0);
});

test("delivery requires exactly the complete checkpoint diagnostic PASS receipt", async () => {
  for (const observation of [
    [],
    [{ id: "DELIVERY-02", status: "PASS" }],
    [{ id: "CHECKPOINT-DIAGNOSTIC", status: "FAIL" }],
    [
      { id: "CHECKPOINT-DIAGNOSTIC", status: "PASS" },
      { id: "INSTALL03", status: "PASS" },
    ],
  ]) {
    const receipts = await executeBoundaryProbes(
      { diagnosticOnly: true, promotionEvidence: false },
      {
        modes: ["delivery"],
        delivery: async () => observation,
        persist: async () => {},
      },
    );
    assert.equal(receipts[0].status, "FAIL");
    assert.equal(receipts[0].observation, observation);
  }
});

test("evidence write failure preserves original error and identity while next probe runs", async () => {
  const identity = {
    diagnosticOnly: true,
    promotionEvidence: false,
    sourceSha: "a".repeat(40),
    fixtureSha: "b".repeat(40),
    artifactDigests: { workspace: "c".repeat(64) },
  };
  const events = [];
  const receipts = await executeBoundaryProbes(identity, {
    tray: async (actual) => {
      assert.equal(actual, identity);
      events.push("tray");
      throw new Error("original");
    },
    delivery: async () => {
      events.push("delivery");
      return [{ id: "CHECKPOINT-DIAGNOSTIC", status: "PASS" }];
    },
    persist: async (receipt) => {
      events.push(receipt.mode + "-write");
      if (receipt.mode === "tray") throw new Error("disk unavailable");
    },
  });
  assert.deepEqual(events, ["tray", "tray-write", "delivery", "delivery-write"]);
  assert.equal(receipts[0].error.message, "original");
  assert.equal(receipts[0].evidenceWriteFailure.message, "disk unavailable");
  assert.deepEqual(
    receipts.map((r) => r.status),
    ["FAIL", "PASS"],
  );
  for (const receipt of receipts) {
    assert.equal(receipt.sourceSha, identity.sourceSha);
    assert.equal(receipt.fixtureSha, identity.fixtureSha);
    assert.equal(receipt.artifactDigests, identity.artifactDigests);
  }
});
