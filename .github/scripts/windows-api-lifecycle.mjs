// Owned, disposable Windows processes only. Proves temporary UI listeners and
// explicit service-profile workers have separate lifetimes and port ownership.
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, copyFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { randomUUID } from "node:crypto";
import { Cdp, unusedPort, waitForCdp, windowsProcessIsElevated, inspectElevatedCdpPolicy, installElevatedCdpPolicy, restoreElevatedCdpPolicy } from "./windows-packaged-smoke.mjs";
assert.equal(process.platform, "win32"); assert.equal(process.env.GITHUB_ACTIONS, "true"); assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
const root = mkdtempSync(path.join(tmpdir(), "devbox-api-migration-fixture-lifecycle-"));
const executable = path.join(root, `api-lifecycle-${randomUUID()}.exe`); copyFileSync(path.resolve("target/debug/devbox-api-studio.exe"), executable);
const profile = path.join(root, "webview"); const live = new Set(); const policies = new Set();
const evidence = { source: process.env.GITHUB_SHA, environment: "github-hosted-windows", step: "start", result: "failed" };
mkdirSync("product-foundation-evidence", { recursive: true });
function progress(step) { evidence.step = step; writeFileSync("product-foundation-evidence/api-lifecycle.json", JSON.stringify(evidence, null, 2)); }
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
function environment(extra = {}) {
  const env = { ...process.env, DEVBOX_API_MIGRATION_FIXTURE_ROOT: root, ...extra };
  for (const key of Object.keys(env)) if (/TOKEN|SECRET|PASSWORD|PRIVATE_KEY|API_KEY/i.test(key)) delete env[key];
  return env;
}
// Debug executables use the console subsystem. Hide that inherited console so
// MainWindowHandle measures an interactive product window, not a debug console.
async function child(args, env) { const value = spawn(executable, args, { env, stdio: "ignore", windowsHide: true }); live.add(value); await once(value, "spawn"); return value; }
async function until(check, label, timeout = 15000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) { if (await check()) return; await delay(100); }
  throw new Error(label);
}
async function exited(value) { await until(() => value.exitCode !== null || value.signalCode !== null, "owned process did not exit"); live.delete(value); }
function powershell(value, action) {
  assert.ok(Number.isSafeInteger(value.pid) && value.pid > 0);
  const script = `$p=[Diagnostics.Process]::GetProcessById(${value.pid}); ${action}`;
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], { encoding: "utf8", windowsHide: true });
  if (result.status !== 0) throw new Error("owned process window query failed"); return result.stdout.trim();
}
const handle = value => powershell(value, "$p.MainWindowHandle.ToInt64()");
function windowDetails(value) {
  return JSON.parse(powershell(value, `
Add-Type -TypeDefinition 'using System; using System.Text; using System.Runtime.InteropServices; public struct FixtureRect { public int Left, Top, Right, Bottom; } public static class FixtureWindow { [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out FixtureRect rect); [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int index); [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder name, int length); [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h); }';
$h=$p.MainWindowHandle; $name=New-Object Text.StringBuilder 256; $rect=New-Object FixtureRect; $null=[FixtureWindow]::GetClassName($h,$name,256); $null=[FixtureWindow]::GetWindowRect($h,[ref]$rect); @{ handle=$h.ToInt64(); class=$name.ToString(); visible=[FixtureWindow]::IsWindowVisible($h); width=($rect.Right-$rect.Left); height=($rect.Bottom-$rect.Top); extendedStyle=[FixtureWindow]::GetWindowLong($h,-20) } | ConvertTo-Json -Compress`));
}
const closeWindow = value => powershell(value, "$null=$p.CloseMainWindow()");
async function startUi() {
  const port = await unusedPort();
  const policy = windowsProcessIsElevated() ? inspectElevatedCdpPolicy(path.basename(executable), port) : null;
  if (policy) { installElevatedCdpPolicy(policy); policies.add(policy); }
  const env = environment({ WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`, WEBVIEW2_USER_DATA_FOLDER: profile });
  const process = await child([], env); const target = await waitForCdp(port, "Devbox API Studio");
  const cdp = new Cdp(target.webSocketDebuggerUrl); await cdp.connect();
  await until(() => cdp.evaluate('!!document.querySelector(".url-input")'), "interactive startup did not finish", 30000);
  return { process, cdp, env, policy };
}
async function command(ui, method, args = {}) {
  return ui.cdp.evaluate(`(async()=>{const invoke=window.__TAURI_INTERNALS__.invoke;const d=await invoke("plugin:product-shell|describe");const header={protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+5000,route:"webhooks"};return invoke("plugin:api-studio|execute",{request:{header,component:"api-studio.webhooks",method:${JSON.stringify(method)},args:${JSON.stringify(args)}}});})()`);
}
async function success(ui, method, args) { const result = await command(ui, method, args); assert.equal(result.operation.outcome.state, "succeeded", method); return result.value; }
async function responding(port) { try { const response = await fetch(`http://127.0.0.1:${port}/lifecycle-fixture`, { signal: AbortSignal.timeout(500) }); await response.arrayBuffer(); return true; } catch { return false; } }
function restore(ui) { ui.cdp.close(); if (ui.policy) { restoreElevatedCdpPolicy(ui.policy); policies.delete(ui.policy); } }
let ui;
try {
  ui = await startUi();
  assert.equal((await success(ui, "lifecycle_status")).policy, "stop-on-close");
  const servicePort = await unusedPort(); await success(ui, "start_server", { bind: "127.0.0.1", port: servicePort, allowLan: false });
  await until(() => responding(servicePort), "temporary listener did not respond");
  const definition = await success(ui, "export_run_service_definition");
  assert.equal(definition.services.length, 1); const service = definition.services[0];
  assert.equal(service.enabled, false); assert.equal(service.autoStart, false); assert.equal(service.restartPolicy, "never");
  assert.match(service.command, /--service-profile [a-f0-9-]{36}$/); assert.ok(service.command.includes(executable));
  progress("port-collision");
  const collision = await child(["--service-profile", service.id], environment()); await exited(collision); assert.notEqual(collision.exitCode, 0);
  assert.equal((await success(ui, "server_status")).running, true);
  progress("default-close");
  closeWindow(ui.process); await exited(ui.process); restore(ui); ui = null;
  await until(async () => !await responding(servicePort), "default close left the temporary listener running");
  progress("owned-service");
  const worker = await child(["--service-profile", service.id], environment());
  await until(() => responding(servicePort), "explicit service did not start");
  evidence.serviceWindow = windowDetails(worker);
  assert.equal(evidence.serviceWindow.handle, 0, "headless service worker must not create any window");
  ui = await startUi(); assert.equal((await success(ui, "server_status")).running, false);
  const refused = await command(ui, "start_server", { bind: "127.0.0.1", port: servicePort, allowLan: false }); assert.equal(refused.operation.outcome.state, "failed"); assert.equal(await responding(servicePort), true);
  const temporaryPort = await unusedPort(); await success(ui, "start_server", { bind: "127.0.0.1", port: temporaryPort, allowLan: false });
  assert.equal((await success(ui, "lifecycle_status")).trayAvailable, true);
  await success(ui, "set_close_policy", { policy: "keep-listening" });
  progress("keep-hidden");
  closeWindow(ui.process); await until(() => handle(ui.process) === "0", "keep-listening did not hide the window");
  assert.equal(ui.process.exitCode, null); assert.equal(await responding(temporaryPort), true); assert.equal(await responding(servicePort), true);
  const second = await child([], ui.env); await exited(second); assert.equal(second.exitCode, 0);
  await until(() => handle(ui.process) !== "0", "single-instance relaunch did not restore hidden window");
  assert.equal((await success(ui, "server_status")).running, true);
  progress("full-quit");
  // Full quit overrides keep-listening. The process may exit before CDP returns
  // its final response, so require normal native exit and released sockets too.
  const quitting = command(ui, "quit_product").catch(() => null);
  await exited(ui.process); assert.equal(ui.process.exitCode, 0);
  const quitResult = await quitting;
  if (quitResult) assert.equal(quitResult.operation.outcome.state, "succeeded");
  restore(ui); ui = null;
  await until(async () => !await responding(temporaryPort), "full quit left its listener running");
  assert.equal(await responding(servicePort), true, "interactive exit stopped the Runtime-owned service");
  worker.kill(); await exited(worker);
  await until(async () => !await responding(servicePort), "service process exit retained its socket");
  Object.assign(evidence, { defaultCloseStops: true, explicitHiddenListening: true, explicitFullQuit: true, trayAvailable: true, relaunchRestoresWindow: true, inactiveExport: true, serviceHasNoInteractiveWindow: true, dualPortOwnerRejected: true, serviceOwnershipIndependent: true, allSocketsReleased: true, result: "pass" });
  progress("complete");
} catch (error) { evidence.error = error.message; throw error; }
finally {
  ui?.cdp.close();
  for (const value of live) { if (value.exitCode === null && value.signalCode === null) value.kill(); }
  for (const policy of policies) restoreElevatedCdpPolicy(policy);
  writeFileSync("product-foundation-evidence/api-lifecycle.json", JSON.stringify(evidence, null, 2));
}
