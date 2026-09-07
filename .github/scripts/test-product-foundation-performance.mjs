import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { summarizeIdle, evaluateBudgets, loadPerformanceConfig, measureWorkload } from "./product-foundation-performance.mjs";

const configFile = new URL("./product-foundation-performance.json", import.meta.url);
const config = JSON.parse(readFileSync(configFile, "utf8"));
assert.throws(() => loadPerformanceConfig(configFile, config.baselineTag, config.baselineCommit, false));
assert.throws(() => loadPerformanceConfig(configFile, config.baselineTag, "f".repeat(40), true));
loadPerformanceConfig(configFile, config.baselineTag, config.baselineCommit, true);
const before = [{ pid: 1, created: "fixture-1", cpuMs: 10, workingSetBytes: 1024 }];
const idle = summarizeIdle(before, [{ ...before[0], cpuMs: 110 }], 5000, 4);
assert.equal(idle.cpuPercentOfMachine, 0.5);
assert.equal(idle.cohortChanged, false);
assert.throws(() => summarizeIdle(before, [{ ...before[0], cpuMs: 0 }], 5000, 4));
assert.throws(() => summarizeIdle([{ ...before[0], cpuMs: NaN }], before, 5000, 4));
assert.throws(() => summarizeIdle(before, before, 0, 4));
const reused = summarizeIdle(before, [{ ...before[0], created: "new-process", cpuMs: 0 }], 5000, 4);
assert.equal(reused.cohortChanged, true);
const measured = { coldRendererReadyMs: 2000, firstKeyboardEventMs: 20, warmExistingWindowMs: 1000, idle, workload: null };
assert.equal(evaluateBudgets(measured, config).passed, true);
assert.equal(evaluateBudgets({ ...measured, idle: reused }, config).passed, false);
assert.equal(evaluateBudgets({ ...measured, coldRendererReadyMs: NaN }, config).passed, false);
assert.equal(evaluateBudgets({ ...measured, workload: { searchMs: [1001] } }, config).passed, false);
assert.match(evaluateBudgets(measured, config).r24, /not-complete/);
const cdp = (invoke) => ({ evaluate: (expression) => runInNewContext(expression, { window: { __TAURI_INTERNALS__: { invoke } } }) });
let profile;
const terminal = await measureWorkload({ id: 'wsl-desktop' }, cdp(async (command, args) => {
  if (command === 'list_sessions') return [];
  if (command === 'save_workspace_profile') { profile = { ...args.profile, id: 'fixture-profile' }; return profile; }
  if (command === 'list_workspace_profiles') return [profile];
  if (command === 'list_distros') throw 'private native environment detail';
  assert.fail('unexpected command');
}), '/synthetic');
assert.equal(terminal.distributions.status, 'unavailable');
assert.equal(terminal.distributions.count, null);
assert.match(terminal.livePtyRestore, /not-run/);
await assert.rejects(measureWorkload({ id: 'run-manager' }, cdp(async () => { throw 'private path and token'; }), '/synthetic'),
  (error) => error.message === 'baseline native command failed: create_job (native-rejected)');
console.log("Performance sampler rejects stale cohorts, invalid metrics and budget violations: PASS");
