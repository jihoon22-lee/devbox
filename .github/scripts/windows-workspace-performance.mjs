// Single-installation measurements before any project, editor or LSP workload.
import assert from "node:assert/strict";
import {lstatSync, readFileSync, writeFileSync} from "node:fs";
import {spawn} from "node:child_process";
import {once} from "node:events";
import {setTimeout as delay} from "node:timers/promises";
import {allWindowsProcesses, displaceOwnedWindow, waitForOwnedRestoredWindow} from "./windows-packaged-smoke.mjs";
import {ownedDescendantsFromSnapshot} from "./windows-process-identity.mjs";
import {measureInput, measureIdle, evaluateBudgets, performanceHost} from "./product-foundation-performance.mjs";

export async function measureWorkspaceStartup({cdp, child, executable, env, started, startupMs}) {
  assert.equal(process.platform, "win32");
  assert.equal(process.env.GITHUB_ACTIONS, "true");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  const config = JSON.parse(readFileSync(new URL("./product-foundation-performance.json", import.meta.url), "utf8"));
  assert.equal(config.schemaVersion, 1); assert.equal(config.idleSampleMs, 5000);
  const measured = {
    source: process.env.GITHUB_SHA, environment: "github-hosted-windows", fixtureVersion: 1,
    host: performanceHost(), build: "hidden Windows debug executable; not a packaged release comparison",
    conditions: {
      cold: "new process and isolated empty onboarding profile; OS cache not flushed",
      input: "inert F24 CDP event acknowledgement immediately after onboarding readiness",
      idle: "ten-second startup survival then five-second parent and owned-descendant sample; before any project workload or second installation",
      warm: "second invocation of the same installation; owned visible primary and renderer health checked; displacement mode recorded separately",
      comparison: "existing baseline budgets only; legacy baseline runs in a different CI job, so this is not a same-machine comparison",
    },
    coldRendererReadyMs: startupMs, result: "failed", stage: "input",
  };
  const record = () => writeFileSync("product-foundation-evidence/workspace-performance.json", JSON.stringify(measured, null, 2));
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
    measured.displacement = await displaceOwnedWindow(child.pid, "Devbox Workspace", false, true);
    const warmStarted = performance.now();
    secondary = spawn(executable, [], {env, stdio: "ignore"});
    await once(secondary, "spawn");
    while (secondary.exitCode === null && secondary.signalCode === null && performance.now() - warmStarted < config.budgets.warmExistingWindowMs) await delay(50);
    assert.equal(secondary.exitCode, 0, "second invocation retained another owner");
    measured.restoredWindow = await waitForOwnedRestoredWindow(child.pid, "Devbox Workspace", Math.max(1, config.budgets.warmExistingWindowMs - (performance.now() - warmStarted)));
    assert.equal(child.exitCode, null); assert.equal(child.signalCode, null);
    const after = await cdp.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe")');
    assert.deepEqual(after.handshake, before.handshake);
    assert.equal(await cdp.evaluate('Array.from(document.querySelectorAll(".workspace-registry button")).some(button => button.textContent.trim() === "빈 Workspace 시작" && !button.disabled)'), true);
    measured.warmExistingWindowMs = Math.round(performance.now() - warmStarted);
    identities();
    measured.budget = evaluateBudgets(measured, config, "workspace");
    assert.equal(measured.budget.passed, true, `Workspace performance budget failed: ${measured.budget.violations.join(", ")}`);
    measured.stage = "complete"; measured.result = "pass";
    return measured;
  } finally {
    if (secondary?.exitCode === null && secondary.signalCode === null) secondary.kill();
    record();
  }
}
