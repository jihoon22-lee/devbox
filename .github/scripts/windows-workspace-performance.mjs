// Single-installation measurements before any project, editor or LSP workload.
import assert from "node:assert/strict";
import {lstatSync, readFileSync, writeFileSync} from "node:fs";
import {spawn} from "node:child_process";
import {once} from "node:events";
import {setTimeout as delay} from "node:timers/promises";
import {allWindowsProcesses, displaceOwnedWindow, waitForOwnedRestoredWindow} from "./windows-packaged-smoke.mjs";
import {ownedDescendantsFromSnapshot} from "./windows-process-identity.mjs";
import {measureInput, measureIdle, evaluateBudgets, performanceHost} from "./product-foundation-performance.mjs";

export async function measureWorkspaceStartup({cdp, child, executable, env, started, startupMs, product = {id:"workspace",label:"Devbox Workspace"}}) {
  assert.equal(process.platform, "win32");
  assert.equal(process.env.GITHUB_ACTIONS, "true");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  const config = JSON.parse(readFileSync(new URL("./product-foundation-performance.json", import.meta.url), "utf8"));
  assert.equal(config.schemaVersion, 1); assert.equal(config.idleSampleMs, 5000);
  const measured = {
    source: process.env.GITHUB_SHA, environment: "github-hosted-windows", fixtureVersion: 1,
    host: performanceHost(), product: product.id, build: process.env.DEVBOX_FIXTURE_PROFILE === "release" ? "exact candidate release portable" : "hidden Windows debug executable",
    conditions: {
      cold: "new process and isolated empty onboarding profile; OS cache not flushed",
      input: "inert F24 CDP event acknowledgement immediately after onboarding readiness",
      idle: "ten-second startup survival then five-second parent and owned-descendant sample; before any project workload or second installation",
      warm: "second invocation of the same installation; owned visible primary and renderer health checked; displacement mode recorded separately",
      comparison: process.env.DEVBOX_FIXTURE_PROFILE === "release" ? "same-job pinned anchor measurements in performance-baseline/runtime.json; unchanged B01 budgets" : "existing budgets; baseline is a different CI job",
    },
    coldRendererReadyMs: startupMs, result: "failed", stage: "input",
  };
  const record = () => writeFileSync(`product-foundation-evidence/${product.id}-performance.json`, JSON.stringify(measured, null, 2));
  let secondary;
  try {
    measured.firstKeyboardEventMs = await measureInput({...cdp, send: cdp.command});
    measured.firstInputObservedMs = Math.round(performance.now() - started);
    measured.stage = "idle"; record();
    await delay(Math.max(0, 10_000 - (performance.now() - started)));
    const identity = allWindowsProcesses().find(process => process.Pid === child.pid);
    assert.ok(identity && child.exitCode === null && child.signalCode === null);
    const actual = lstatSync(identity.Path, {bigint: true}), owned = lstatSync(executable, {bigint: true});
    assert.equal(actual.dev, owned.dev); assert.equal(actual.ino, owned.ino);
    const identities = () => {
      const current = allWindowsProcesses();
      assert.ok(current.some(process => process.Pid === identity.Pid && process.Created === identity.Created && process.Path === identity.Path && process.Name === identity.Name));
      return [identity, ...ownedDescendantsFromSnapshot(identity, current)];
    };
    measured.idle = await measureIdle(identities, config.idleSampleMs);
    measured.stage = "warm"; record();
    const before = await cdp.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe")');
    identities();
    measured.displacement = await displaceOwnedWindow(child.pid, product.label, false, true);
    const warmStarted = performance.now();
    secondary = spawn(executable, [], {env, stdio: "ignore"});
    await once(secondary, "spawn");
    while (secondary.exitCode === null && secondary.signalCode === null && performance.now() - warmStarted < config.budgets.warmExistingWindowMs) await delay(50);
    assert.equal(secondary.exitCode, 0, "second invocation retained another owner");
    measured.restoredWindow = await waitForOwnedRestoredWindow(child.pid, product.label, Math.max(1, config.budgets.warmExistingWindowMs - (performance.now() - warmStarted)));
    assert.equal(child.exitCode, null); assert.equal(child.signalCode, null);
    const after = await cdp.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe")');
    assert.deepEqual(after.handshake, before.handshake);
    if (product.id === "workspace") assert.equal(await cdp.evaluate('!!document.getElementById("workspace-project-path")'), true);
    measured.warmExistingWindowMs = Math.round(performance.now() - warmStarted);
    identities();
    measured.budget = evaluateBudgets(measured, config, product.id);
    assert.equal(measured.budget.passed, true, `Workspace performance budget failed: ${measured.budget.violations.join(", ")}`);
    measured.stage = "complete"; measured.result = "pass";
    return measured;
  } finally {
    if (secondary?.exitCode === null && secondary.signalCode === null) secondary.kill();
    record();
  }
}
