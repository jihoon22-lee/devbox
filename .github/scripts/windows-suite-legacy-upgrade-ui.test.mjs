import assert from "node:assert/strict";
import test from "node:test";
import {
  validateOwnedLegacyRun,
  runLegacyUpgradeUserFlow,
  legacyOperationFailure,
} from "./windows-suite-legacy-upgrade-ui.mjs";
test("legacy subprocess evidence exposes bounded operation and issue without arbitrary stdout", () => {
  const error = legacyOperationFailure(
    "prepare-pinned-import",
    1,
    null,
    JSON.stringify({ issue: "bootstrap_owner_invalid", token: "synthetic-private-token" }),
    "Bearer synthetic-private-token",
  );
  assert.equal(error.operation.issue, "bootstrap_owner_invalid");
  assert.equal(error.operation.exitCode, 1);
  assert.equal(error.operation.operation, "prepare-pinned-import");
  assert.equal(JSON.stringify(error.operation).includes("synthetic-private-token"), false);
  const timeout = legacyOperationFailure("cleanup-owned-installation", null, "SIGTERM", "private-stdout", "");
  assert.equal(timeout.operation.signal, "SIGTERM");
  assert.equal(timeout.operation.issue, null);
});
for (const guard of [validateOwnedLegacyRun, runLegacyUpgradeUserFlow])
  test(`${guard.name} rejects an unowned legacy fixture before changing installation environment`, async () => {
    const previous = process.env.DEVBOX_USER_FLOW_INSTALL_ROOT;
    await assert.rejects(guard(), /win32|true|undefined/);
    assert.equal(process.env.DEVBOX_USER_FLOW_INSTALL_ROOT, previous);
  });

import {
  executeReviewedDeliveryAction,
  selectCurrentGenerationSnapshot,
  waitDeliveryInventoryReady,
} from "./windows-delivery-review.mjs";
test("review waits for each ready target before its single input", async () => {
  const calls = [];
  let release;
  const pending = new Promise((resolve) => {
    release = resolve;
  });
  const ui = {
    waitForTarget: async (target) => {
      calls.push(`wait:${target.name}`);
      if (target.name === "snapshot") await pending;
    },
    click: async (target) => {
      calls.push(`click:${target.name}`);
    },
  };
  const action = executeReviewedDeliveryAction(ui, { role: "button", name: "snapshot" }, async () =>
    calls.push("evidence"),
  );
  await Promise.resolve();
  assert.deepEqual(calls, ["wait:snapshot"]);
  release();
  await action;
  assert.deepEqual(calls, [
    "wait:snapshot",
    "click:snapshot",
    "wait:선택한 작업과 제품 종료를 확인했습니다.",
    "click:선택한 작업과 제품 종료를 확인했습니다.",
    "wait:Control Center를 닫고 실행",
    "evidence",
    "click:Control Center를 닫고 실행",
  ]);
});
test("snapshot selection requires one new current-generation receipt and unchanged old checkpoint", () => {
  const old = { id: "pinned", compatibility: "differentGeneration", bytes: 42 };
  const current = { id: "new", compatibility: "currentGeneration", bytes: 43 };
  const before = { checkpoints: [old] };
  assert.deepEqual(selectCurrentGenerationSnapshot(before, { checkpoints: [old, current] }, old.id), current);
  for (const checkpoints of [
    [old],
    [old, current, { ...current, id: "other" }],
    [old, { ...current, compatibility: "differentGeneration" }],
    [{ ...old, bytes: 0 }, current],
  ]) {
    assert.throws(() => selectCurrentGenerationSnapshot(before, { checkpoints }, old.id));
  }
});

test("loaded inventory during restore health does not require the disabled snapshot action", async () => {
  const inventory = { activeOperation: "restore-health", checkpoints: [] };
  const observed = [];
  await waitDeliveryInventoryReady({
    waitForTarget: async (target) => {
      assert.ok(inventory.activeOperation);
      assert.deepEqual(target, { role: "heading", name: "제품 데이터 보존본" });
      observed.push(target.name);
    },
    click: async () => assert.fail("Inventory observation must not input"),
  });
  assert.deepEqual(observed, ["제품 데이터 보존본"]);
});

test("legacy WAL proof writes only an existing schema setting and survives a consistent snapshot", async () => {
  const { legacyWalWrite, legacyWalRead } = await import("./windows-suite-legacy-upgrade-ui.mjs");
  const { mkdtemp, rm, stat } = await import("node:fs/promises");
  const { tmpdir } = await import("node:os");
  const { spawnSync } = await import("node:child_process");
  const path = (await import("node:path")).default;
  const folder = await mkdtemp(path.join(tmpdir(), "devbox-legacy-wal-test-"));
  const db = path.join(folder, "notes.db"),
    snapshot = path.join(folder, "snapshot.db");
  const python = process.platform === "win32" ? "python" : "python3";
  const run = (source, args = []) => {
    const result = spawnSync(python, ["-c", source, ...args], { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    return result.stdout.trim();
  };
  try {
    run(
      "import sqlite3,sys;c=sqlite3.connect(sys.argv[1]);c.execute('CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT)');c.execute(\"INSERT INTO settings VALUES('root',?)\",(sys.argv[2],));c.commit()",
      [db, folder],
    );
    run(legacyWalWrite, [db, "owned-wal-token"]);
    assert.ok((await stat(`${db}-wal`)).size > 32);
    assert.equal(JSON.parse(run(legacyWalRead, [db])), "owned-wal-token");
    run(
      "import sqlite3,sys,pathlib;c=sqlite3.connect(pathlib.Path(sys.argv[1]).as_uri()+'?mode=ro',uri=True);assert c.execute(\"SELECT name FROM sqlite_master WHERE type='table'\").fetchall()==[('settings',)];c.backup(sqlite3.connect(sys.argv[2]))",
      [db, snapshot],
    );
    assert.equal(JSON.parse(run(legacyWalRead, [snapshot])), "owned-wal-token");
  } finally {
    await rm(folder, { recursive: true, force: true });
  }
});

test("committed upgrade proves all four registered shortcuts without a direct-launch fallback", async () => {
  const { verifyRegisteredShortcutLaunches } = await import("./windows-suite-legacy-upgrade-ui.mjs");
  const products = [];
  const proofs = await verifyRegisteredShortcutLaunches(async (product) => {
    products.push(product);
    return {
      product,
      registeredLink: true,
      freshProcess: true,
      imageVerified: true,
      ownedWindowReady: true,
      screenshot: `${product}.png`,
    };
  });
  assert.deepEqual(products, ["workspace", "api-studio", "knowledge", "control-center"]);
  assert.equal(proofs.length, 4);
  const attempted = [];
  await assert.rejects(
    verifyRegisteredShortcutLaunches(async (product) => {
      attempted.push(product);
      throw new Error("registered link failed");
    }),
    /registered link failed/,
  );
  assert.deepEqual(attempted, ["workspace"]);
});

test("shortcut readiness requires the exact product window and usable navigation", async () => {
  const { shortcutWindowReady } = await import("./windows-suite-legacy-upgrade-ui.mjs");
  const view = {
    name: "Devbox Workspace",
    enabled: true,
    selectedWindowCount: 1,
    buttons: [{ name: "개요", enabled: true, visible: true }],
  };
  assert.equal(shortcutWindowReady("workspace", view), true);
  for (const bad of [
    { ...view, name: "foreign" },
    { ...view, selectedWindowCount: 0 },
    { ...view, buttons: [] },
  ])
    assert.equal(shortcutWindowReady("workspace", bad), false);
});

test("shortcut subprocess preserves fixed source stage and command while omitting arbitrary values", () => {
  const failure = legacyOperationFailure(
    "registered-shortcut-launch",
    1,
    null,
    JSON.stringify({
      issue: "shortcut_command_unavailable",
      stage: "retained-payload",
      command: "Get-FileHash",
      line: 91,
      path: "private",
    }),
    "",
  );
  assert.deepEqual(failure.operation.shortcut, { stage: "retained-payload", command: "Get-FileHash", line: 91 });
  assert.ok(!JSON.stringify(failure.operation).includes("private"));
});

import { readLegacyRestoreInventory } from "./windows-suite-legacy-upgrade-ui.mjs";
test("only exact read-only inventory busy is observed again; deadline keeps original error", async () => {
  const busy = new Error("Read-only delivery restore_inventory failed: unavailable/suite_update_busy");
  let calls = 0;
  const center = {
    delivery: async (method) => {
      assert.equal(method, "restore_inventory");
      if (++calls === 1) throw busy;
      return { checkpoints: [] };
    },
  };
  assert.deepEqual(
    await readLegacyRestoreInventory(center, async (check) => {
      assert.equal(await check(), false);
      assert.equal(await check(), true);
    }),
    { checkpoints: [] },
  );
  await assert.rejects(
    readLegacyRestoreInventory(
      {
        delivery: async () => {
          throw busy;
        },
      },
      async (check) => {
        await check();
        throw new Error("Owned UI observation timed out: owned restore inventory lock available");
      },
    ),
    (error) => error === busy,
  );
  const other = new Error("Read-only delivery restore_inventory failed: unavailable/suite_journal_invalid");
  await assert.rejects(
    readLegacyRestoreInventory(
      {
        delivery: async () => {
          throw other;
        },
      },
      async (check) => check(),
    ),
    (error) => error === other,
  );
});

import { finishLegacyCleanup } from "./windows-suite-legacy-upgrade-ui.mjs";
test("legacy removal is attempted after close fails while first error survives", async () => {
  const original = Object.freeze(new Error("journey")),
    closeError = new Error("close"),
    removeError = new Error("remove");
  let removes = 0;
  for (const first of [original, null]) {
    await assert.rejects(
      finishLegacyCleanup(
        async () => {
          throw closeError;
        },
        async () => {
          removes++;
          throw removeError;
        },
        first,
      ),
      (error) => error === (first ?? closeError),
    );
  }
  assert.equal(removes, 2);
});

test("reviewed restore waits for a fresh Center identity and coherent unblocked activation", async () => {
  const { legacyReviewReopenReady } = await import("./windows-suite-legacy-upgrade-ui.mjs");
  const center = {
    executable: "C:\\owned\\devbox-control-center.exe",
    processIdentity: { Pid: 7, Created: "2026-10-05T08:00:00.0000000Z" },
    manifest: { installationId: "owned", generation: "generation" },
  };
  const original = { ...center.processIdentity, Path: center.executable };
  const fresh = { Pid: 8, Created: "2026-10-05T08:00:01.0000000Z", Path: center.executable };
  const marker = { installationId: "owned", generation: "generation", phase: "health" };
  const ready = (processes, activation = marker, restoreBlocked = false) =>
    legacyReviewReopenReady(center, "restore", { processes, activation, restoreBlocked });
  assert.equal(ready([original]), false);
  assert.equal(ready([{ ...fresh, Created: original.Created }]), false);
  assert.equal(ready([{ ...fresh, Pid: original.Pid }]), false);
  assert.equal(ready([fresh], { ...marker, phase: "recover" }), false);
  assert.equal(ready([fresh], { ...marker, generation: "other" }), false);
  assert.equal(ready([fresh], marker, true), false);
  assert.equal(ready([fresh], marker, null), false);
  assert.equal(ready([fresh]), true);
  assert.equal(
    legacyReviewReopenReady(center, "snapshot", { processes: [fresh], activation: marker, restoreBlocked: false }),
    false,
  );
});

test("review failure is preserved before cleanup even when helper observation fails", async () => {
  const { preserveLegacyReviewFailure } = await import("./windows-suite-legacy-upgrade-ui.mjs");
  const original = Object.freeze(new Error("owned process failed"));
  let called = 0;
  await assert.rejects(
    preserveLegacyReviewFailure({}, original, async (_center, error, id) => {
      assert.equal(error, original);
      assert.match(id, /^[a-f0-9-]{36}$/);
      called++;
      throw new Error("observation failed");
    }),
    (error) => error === original,
  );
  assert.equal(called, 1);
});

test("shortcut close waits for captured WebView lifetimes, ignoring Agent and recycled PIDs", async () => {
  const { closeLegacyShortcut } = await import("./windows-suite-legacy-upgrade-ui.mjs");
  const webview = {
    Pid: 10,
    Created: "2026-10-06T00:00:00Z",
    Name: "msedgewebview2.exe",
    Path: "C:\\WebView\\msedgewebview2.exe",
  };
  const agent = {
    Pid: 20,
    Created: "2026-10-06T00:00:00Z",
    Name: "devbox-agent.exe",
    Path: "C:\\Suite\\devbox-agent.exe",
  };
  let stopped = false,
    observed = false;
  let processes = [webview, agent];
  await closeLegacyShortcut(
    {},
    "fixture",
    {},
    {
      stop: async () => {
        stopped = true;
        return { descendants: [webview, agent] };
      },
      read: () => processes,
      wait: async (ready) => {
        observed = true;
        assert.equal(stopped, true);
        assert.equal(ready(), false, "parent exit does not release live WebView files");
        processes = [{ ...webview, Created: "2026-10-06T00:00:01Z" }, agent];
        assert.equal(ready(), true, "do not wait for an unrelated recycled process or the installed Agent");
      },
    },
  );
  assert.equal(observed, true);
});
