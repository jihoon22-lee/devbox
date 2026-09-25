// Current product vault ownership, rebinding and window lifetime.
import assert from "node:assert/strict";
import { stageKnowledge } from "./workspace-wsl-artifact.mjs";
import { exerciseKnowledgeWsl } from "./windows-knowledge-wsl.mjs";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, copyFileSync, renameSync, existsSync, readdirSync, lstatSync, rmSync } from "node:fs";
import path from "node:path";
import { tmpdir } from "node:os";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { createHash, randomUUID } from "node:crypto";
import { Cdp, unusedPort, waitForCdp, windowsLocalAppData, windowsProcessIsElevated, inspectElevatedCdpPolicy, installElevatedCdpPolicy, restoreElevatedCdpPolicy, allWindowsProcesses } from "./windows-packaged-smoke.mjs";
import { ownedDescendantsFromSnapshot } from "./windows-process-identity.mjs";
import { measureInput, measureIdle, evaluateBudgets, performanceHost } from "./product-foundation-performance.mjs";
const performanceConfig = JSON.parse(readFileSync(new URL("./product-foundation-performance.json", import.meta.url), "utf8"));
assert.equal(performanceConfig.schemaVersion, 1); assert.equal(performanceConfig.idleSampleMs, 5000);
assert.equal(process.platform, "win32"); assert.equal(process.env.GITHUB_ACTIONS, "true"); assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
const directory = mkdtempSync(path.join(tmpdir(), "devbox-knowledge-lifecycle-fixture-"));
const base = windowsLocalAppData();
const report = "product-foundation-evidence/knowledge-lifecycle.json";
mkdirSync(path.dirname(report), { recursive: true });
const evidence = { source: process.env.GITHUB_SHA, environment: "github-hosted-windows", step: "startup", result: "failed" };
const digest = file => createHash("sha256").update(readFileSync(file)).digest("hex");
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const progress = step => { evidence.step = step; writeFileSync(report, JSON.stringify(evidence, null, 2)); };
const profiles = [], live = new Set();
const elevated = windowsProcessIsElevated();
function childEnvironment(extra) {
  const env = { ...process.env, ...extra };
  for (const key of Object.keys(env)) if (/TOKEN|SECRET|PASSWORD|PRIVATE_KEY|API_KEY/i.test(key)) delete env[key];
  return env;
}
async function start(executable, title, profile) {
  const port = await unusedPort();
  const policy = elevated ? inspectElevatedCdpPolicy(path.basename(executable), port) : null;
  const item = { executable, policy, child: null, cdp: null }; live.add(item);
  if (policy) installElevatedCdpPolicy(policy);
  item.launchedAt = performance.now();
  item.child = spawn(executable, [], { env: childEnvironment({ WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`, WEBVIEW2_USER_DATA_FOLDER: profile }), stdio: "ignore" });
  await once(item.child, "spawn"); const target = await waitForCdp(port, title);
  item.cdp = new Cdp(target.webSocketDebuggerUrl); await item.cdp.connect(); return item;
}
function closeWindow(item) {
  assert.ok(Number.isInteger(item.child.pid) && item.child.pid > 0);
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", `$p=[Diagnostics.Process]::GetProcessById(${item.child.pid}); $window=$p.MainWindowHandle.ToInt64(); $requested=$p.CloseMainWindow(); @{window=$window;requested=$requested}|ConvertTo-Json -Compress`], { encoding: "utf8", windowsHide: true });
  assert.equal(result.status, 0); const closed = JSON.parse(result.stdout);
  assert.equal(closed.requested, true); assert.ok(Number.isSafeInteger(closed.window) && closed.window > 0); return closed.window;
}
function visible(window) {
  assert.ok(Number.isSafeInteger(window) && window > 0);
  const code = `Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class KnowledgeWindowProbe { [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window); }'; [KnowledgeWindowProbe]::IsWindowVisible([IntPtr]${window})`;
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", code], { encoding: "utf8", windowsHide: true });
  assert.equal(result.status, 0); assert.ok(["True", "False"].includes(result.stdout.trim())); return result.stdout.trim() === "True";
}
function running(item) { return item.child?.exitCode === null && item.child?.signalCode === null; }
async function stop(item, force = false) {
  item.cdp?.close();
  if (running(item)) {
    if (force) item.child.kill(); else closeWindow(item);
    for (let i = 0; i < 100 && running(item); i++) await delay(100);
    if (running(item)) { item.child.kill(); throw new Error("owned fixture process did not exit"); }
  }
  if (item.policy) restoreElevatedCdpPolicy(item.policy); live.delete(item);
}
async function wait(cdp, expression, label, timeout = 60_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) { if (await cdp.evaluate(expression)) return; await delay(150); }
  evidence.ui = await cdp.evaluate('(document.body?.innerText ?? "").slice(0, 10000)'); throw new Error(label);
}
async function click(cdp, label) {
  await wait(cdp, `Array.from(document.querySelectorAll("button")).some(button => button.textContent.trim() === ${JSON.stringify(label)} && !button.disabled)`, `control unavailable: ${label}`);
  await cdp.evaluate(`Array.from(document.querySelectorAll("button")).find(button => button.textContent.trim() === ${JSON.stringify(label)} && !button.disabled).click()`);
}
async function command(item, component, method, args = {}) {
  const route = component === "knowledge.activity" ? "activity" : (component.includes("search") || component === "knowledge.opener") ? "search" : "notes";
  return item.cdp.evaluate(`(async () => { const invoke=window.__TAURI_INTERNALS__.invoke; const d=await invoke("plugin:product-shell|describe");
    const header={protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+5000,route:${JSON.stringify(route)}};
    return invoke("plugin:knowledge|execute",{request:{header,component:${JSON.stringify(component)},method:${JSON.stringify(method)},args:${JSON.stringify(args)}}}); })()`);
}
async function sourceQuery(item, source, query, filter = {}, limit = 200, mode = "name") {
  let result = await command(item, "knowledge.search", "source_query", { source, query, mode, limit, filter });
  assert.equal(result.operation.outcome.state, "succeeded"); const generation = result.value.generation;
  for (let i = 0; i < 60 && result.value.state === "running"; i++) {
    await delay(80); result = await command(item, "knowledge.search", "source_poll", { generation });
    assert.equal(result.operation.outcome.state, "succeeded"); assert.equal(result.value.generation, generation);
  }
  assert.notEqual(result.value.state, "running"); assert.equal(result.value.source, source);
  return result.value;
}
async function product(executable, profile) {
  const before = new Set(readdirSync(base).filter(name => name.startsWith("com.devbox.v08.knowledge.i")));
  const item = await start(executable, "Devbox Knowledge", profile);
  await wait(item.cdp, '!!document.querySelector(".knowledge-startup") || !!document.querySelector("#vault-setup-title") || !!document.querySelector(".knowledge-feature-notes .app")', "Knowledge startup missing");
  const added = readdirSync(base).filter(name => name.startsWith("com.devbox.v08.knowledge.i") && !before.has(name));
  if (added.length) { assert.equal(added.length, 1); item.dataRoot = path.join(base, added[0]);
    const owner=randomUUID(), marker=path.join(item.dataRoot,`.knowledge-fixture-${owner}`);
    writeFileSync(marker,owner,{flag:"wx"});profiles.push({root:item.dataRoot,owner,marker,identity:lstatSync(item.dataRoot,{bigint:true})}); }
  return item;
}
try {
  const executable=path.join(directory,`knowledge-product-${randomUUID()}.exe`);
  copyFileSync(path.resolve("target/debug/devbox-knowledge.exe"),executable);
  stageKnowledge(process.env.GITHUB_SHA,path.resolve("apps/devbox-knowledge/src-tauri/resources/wsl"),path.join(directory,"resources/wsl"));
  const profile=path.join(directory,"product-webview");let item=await product(executable,profile);
  await wait(item.cdp,'!!document.querySelector(".knowledge-feature-notes .app")',"automatic Notes startup missing");
  const vault=(await command(item,"knowledge.notes","get_root")).value;
  assert.equal((await command(item,"knowledge.activity","is_tracking")).value,false);
  await command(item,"knowledge.notes","create_file",{rel:"Notes/original.md",content:"# Original"});
  const originalNote=digest(path.join(vault,"Notes/original.md"));
  await command(item,"knowledge.notes","create_template",{draft:{name:"product fixture",content:"new product edit"}});
  evidence.automaticStartupReady=true;
  progress("reviewed-vault-rebinding");
  const selectedVault = path.join(directory, "selected-vault"); mkdirSync(selectedVault); mkdirSync(path.join(selectedVault, "Notes"));
  writeFileSync(path.join(selectedVault, "Notes/existing.md"), "# Existing selected note", { flag: "wx" });
  const selectedOriginal = digest(path.join(selectedVault, "Notes/existing.md"));
  const selectedBefore = readdirSync(selectedVault, { recursive: true }).sort();
  const sameDirectory = (left, right) => { const a=lstatSync(left,{bigint:true}), b=lstatSync(right,{bigint:true}); return a.dev===b.dev && a.ino===b.ino; };
  // Reload observes the current prepared store through the product UI.
  await item.cdp.send("Page.reload");
  await wait(item.cdp, '!!document.querySelector(".knowledge-feature-notes .app")', "Notes did not observe current activation");
  const scheduleVault = async () => {
    await click(item.cdp, "노트 폴더");
    await wait(item.cdp, '!!document.querySelector("#vault-settings-title") && !!document.querySelector(\'[aria-label="연결할 노트 폴더"]:not(:disabled)\')', "vault settings did not finish loading");
    await item.cdp.evaluate(`(() => { const input=document.querySelector('[aria-label="연결할 노트 폴더"]'); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set.call(input,${JSON.stringify(selectedVault)}); input.dispatchEvent(new Event("input",{bubbles:true})); })()`);
    await click(item.cdp, "다음 시작에서 폴더 확인");
    await wait(item.cdp, 'document.body.innerText.includes("다음 시작에서 확인할 폴더")', "vault choice was not scheduled");
    assert.ok(sameDirectory((await command(item,"knowledge.notes","get_root")).value,vault));
  };
  await scheduleVault();
  assert.equal((await command(item,"knowledge.notes","create_file",{rel:"Notes/still-old.md",content:"# Still in original vault"})).operation.outcome.state,"succeeded");
  assert.ok(existsSync(path.join(vault,"Notes/still-old.md"))); assert.equal(existsSync(path.join(selectedVault,"Notes/still-old.md")),false);
  await stop(item); item=await product(executable,profile);
  await wait(item.cdp, '!!document.querySelector("#vault-setup-title")', "next-start vault review missing");
  assert.equal((await command(item,"knowledge.migration","status")).value.active,false);
  await click(item.cdp,"폴더 연결 미리보기");
  await wait(item.cdp,'!!document.querySelector("#vault-preview-title")',"vault preview missing");
  assert.deepEqual(readdirSync(selectedVault,{recursive:true}).sort(),selectedBefore);
  await click(item.cdp,"현재 폴더 유지하고 계속");
  await wait(item.cdp,'!!document.querySelector(".knowledge-feature-notes .app")',"cancel did not restore original editor");
  assert.ok(sameDirectory((await command(item,"knowledge.notes","get_root")).value,vault));
  await scheduleVault();
  const vaultPlan=(await command(item,"knowledge.migration","vault_change_status")).value.schedule;
  await stop(item); item=await product(executable,profile);
  await wait(item.cdp,'!!document.querySelector("#vault-setup-title")',"second vault review missing");
  await click(item.cdp,"폴더 연결 미리보기");
  await wait(item.cdp,'!!document.querySelector("#vault-preview-title")',"second vault preview missing");
  assert.deepEqual(readdirSync(selectedVault,{recursive:true}).sort(),selectedBefore);
  await click(item.cdp,"이 폴더로 변경하고 시작");
  await wait(item.cdp,'!!document.querySelector(".knowledge-feature-notes .app")',"explicit vault activation missing");
  assert.ok(sameDirectory((await command(item,"knowledge.notes","get_root")).value,selectedVault));
  assert.equal((await command(item,"knowledge.notes","list_templates")).value[0].content,"new product edit");
  assert.equal((await command(item,"knowledge.notes","read_file",{rel:"Notes/existing.md"})).value.content,"# Existing selected note");
  assert.equal((await command(item,"knowledge.notes","create_file",{rel:"Notes/new-selected.md",content:"# Selected vault"})).operation.outcome.state,"succeeded");
  assert.ok(existsSync(path.join(selectedVault,"Notes/new-selected.md"))); assert.equal(existsSync(path.join(vault,"Notes/new-selected.md")),false);
  let selectedQuery;
  for(let i=0;i<40;i++){ selectedQuery=await sourceQuery(item,"notes","Selected"); if(selectedQuery.rows.some(row=>row.rootIdentity===`notes:${vaultPlan.id}`)) break; await delay(150); }
  assert.ok(selectedQuery.rows.some(row=>row.rootIdentity===`notes:${vaultPlan.id}`));
  assert.equal(digest(path.join(selectedVault,"Notes/existing.md")),selectedOriginal);
  assert.equal(digest(path.join(vault,"Notes/original.md")),originalNote);
  evidence.vaultScheduleKeptLiveBinding=true; evidence.vaultPreviewDidNotWrite=true; evidence.vaultCancellationKeptOriginal=true;
  evidence.vaultExplicitRebindPreservedFilesAndTemplates=true; evidence.vaultSourceIdentityChanged=true;
  progress("second-installation-owner");
  const secondExe=path.join(directory,`knowledge-second-${randomUUID()}.exe`);copyFileSync(executable,secondExe);
  const secondProfile=path.join(directory,"second-webview");let second=await product(secondExe,secondProfile);
  await wait(second.cdp,'!!document.querySelector(".knowledge-feature-notes .app")',"second private store startup missing");
  assert.equal((await command(second,"knowledge.migration","schedule_vault_change",{path:selectedVault})).operation.outcome.state,"succeeded");
  await stop(second);second=await product(secondExe,secondProfile);
  await wait(second.cdp,'!!document.querySelector("#vault-setup-title")',"second vault review missing");
  await click(second.cdp,"폴더 연결 미리보기");
  await wait(second.cdp,'!!document.querySelector("[role=alert]")',"shared vault owner was not rejected");
  assert.equal((await command(second,"knowledge.migration","status")).value.active,false);
  evidence.secondInstallationBlocked=true;await stop(second);
  progress("configured-notes-performance");
  await stop(item); item = await product(executable, profile);
  await wait(item.cdp, '!!document.querySelector(".knowledge-feature-notes .app")', "configured Notes did not become ready");
  evidence.performance = {
    host: performanceHost(), build: process.env.DEVBOX_FIXTURE_PROFILE === "release" ? "exact candidate release executable" : "hidden Windows debug executable",
    conditions: {
      cold: "new process with the configured synthetic vault profile; OS cache not flushed",
      input: "inert F24 event acknowledgement, using the baseline harness",
      idle: "ten seconds after configured Notes startup; collection OFF; five-second owned-process sample before the 500-file workload",
      warm: "second invocation restores the same hidden product window; no second owner",
      search: "500 generated UTF-8 text files, native file source with exact root filter; ten exact filename queries, including async polling and identity verification",
    },
    coldRendererReadyMs: Math.round(performance.now() - item.launchedAt),
    firstKeyboardEventMs: await measureInput(item.cdp),
    firstInputObservedMs: Math.round(performance.now() - item.launchedAt),
  };
  assert.equal((await command(item, "knowledge.activity", "is_tracking")).value, false);
  await delay(Math.max(0, 10000 - (performance.now() - item.launchedAt)));
  const processIdentity = allWindowsProcesses().find(process => process.Pid === item.child.pid);
  assert.ok(processIdentity && running(item));
  const measuredExe = lstatSync(processIdentity.Path, { bigint: true }), ownedExe = lstatSync(executable, { bigint: true });
  assert.equal(measuredExe.dev, ownedExe.dev); assert.equal(measuredExe.ino, ownedExe.ino);
  evidence.performance.idle = await measureIdle(() => {
    const current = allWindowsProcesses();
    assert.ok(current.some(process => process.Pid === processIdentity.Pid && process.Created === processIdentity.Created && process.Path === processIdentity.Path && process.Name === processIdentity.Name));
    return [processIdentity, ...ownedDescendantsFromSnapshot(processIdentity, current)];
  }, performanceConfig.idleSampleMs);
  progress("500-file-source-performance");
  const performanceRoot = path.join(directory, "performance-root"); mkdirSync(performanceRoot);
  for (let i = 0; i < 500; i++) writeFileSync(path.join(performanceRoot, `fixturesearch${String(i).padStart(4, "0")}.txt`), "synthetic UTF-8 index fixture\n", { flag: "wx" });
  const priorRoots = new Set((await command(item, "knowledge.search", "list_roots")).value.map(root => root.id));
  const indexStarted = performance.now();
  assert.equal((await command(item, "knowledge.search-settings", "add_root", { path: performanceRoot, indexContent: false })).operation.outcome.state, "succeeded");
  const addedRoots = (await command(item, "knowledge.search", "list_roots")).value.filter(root => !priorRoots.has(root.id)); assert.equal(addedRoots.length, 1);
  const performanceFilter = { sourceRootId: addedRoots[0].id };
  let indexedCount = 0;
  while (performance.now() - indexStarted < performanceConfig.budgets.index500FilesMs) {
    const status = (await command(item, "knowledge.search", "index_status")).value;
    if (!status.indexing) {
      const counted = await sourceQuery(item, "files", "fixturesearch", performanceFilter, 2000);
      indexedCount = counted.rows.length;
      await command(item, "knowledge.search", "source_cancel", { generation: counted.generation });
      if (indexedCount === 500) break;
    }
    await delay(100);
  }
  assert.equal(indexedCount, 500, "500-file native index did not complete");
  evidence.performance.workload = { kind: "500-file-native-index-and-10-source-searches", result: "measured", indexMs: Math.round(performance.now() - indexStarted), searchMs: [] };
  for (let i = 0; i < 10; i++) {
    const query = `fixturesearch${String(i).padStart(4, "0")}`, started = performance.now();
    const found = await sourceQuery(item, "files", query, performanceFilter);
    evidence.performance.workload.searchMs.push(Math.round(performance.now() - started));
    assert.equal(found.state, "complete"); assert.equal(found.rows.length, 1);
    assert.equal(found.rows[0].value.name, `${query}.txt`); assert.equal(found.rows[0].availability, "available");
    await command(item, "knowledge.search", "source_cancel", { generation: found.generation });
  }
  progress("explicit-close-policy");
  assert.equal((await command(item, "knowledge.activity", "set_close_policy", { closeToTray: true })).value.closeToTray, true);
  const hidden = closeWindow(item); await delay(500); assert.equal(running(item), true); assert.equal(visible(hidden), false);
  assert.equal((await command(item, "knowledge.activity", "is_tracking")).value, false);
  const warmStarted = performance.now();
  const secondary = { executable, policy: null, cdp: null, child: spawn(executable, [], { env: childEnvironment({ WEBVIEW2_USER_DATA_FOLDER: profile }), stdio: "ignore" }) }; live.add(secondary);
  await once(secondary.child, "spawn");
  while (running(secondary) && performance.now() - warmStarted < performanceConfig.budgets.warmExistingWindowMs) await delay(50);
  assert.equal(secondary.child.exitCode, 0, "relaunch created another live owner");
  let restored = visible(hidden);
  while (!restored && performance.now() - warmStarted < performanceConfig.budgets.warmExistingWindowMs) { await delay(100); restored = visible(hidden); }
  assert.ok(restored && running(item));
  evidence.performance.warmExistingWindowMs = Math.round(performance.now() - warmStarted);
  evidence.performance.budget = evaluateBudgets(evidence.performance, performanceConfig, "knowledge");
  assert.equal(evidence.performance.budget.passed, true, `Knowledge performance budget failed: ${evidence.performance.budget.violations.join(", ")}`);
  // Crash/restart checks persisted preference; this is explicitly not a tray
  // Quit test. Default close below must terminate the process normally.
  await stop(item, true); item = await product(executable, profile);
  assert.equal((await command(item, "knowledge.activity", "get_close_policy")).value.closeToTray, true);
  assert.equal((await command(item, "knowledge.activity", "set_close_policy", { closeToTray: false })).value.closeToTray, false);
  item = await exerciseKnowledgeWsl({ item, executable, profile, command, sourceQuery, product, stop, wait, click, delay, evidence, progress });
  await stop(item);
  evidence.closeToTrayKeepsProcess = true; evidence.closePreferenceSurvivesCrash = true; evidence.defaultCloseExits = true; evidence.result = "pass"; progress("complete");
} catch (error) { evidence.error = String(error.message).slice(0, 2000); throw error; }
finally {
  for (const item of live) { try { await stop(item, true); } catch { item.child?.kill(); if (item.policy) restoreElevatedCdpPolicy(item.policy); } }
  let cleanupFailed = false;
  for (const profile of profiles) {
    try {
      const actual = lstatSync(profile.root, { bigint: true }); const marker = lstatSync(profile.marker);
      assert.ok(actual.isDirectory() && !actual.isSymbolicLink() && actual.dev === profile.identity.dev && actual.ino === profile.identity.ino);
      assert.ok(marker.isFile() && !marker.isSymbolicLink() && marker.size === profile.owner.length); assert.equal(readFileSync(profile.marker, "utf8"), profile.owner);
      rmSync(profile.root, { recursive: true, maxRetries: 20, retryDelay: 50 });
    } catch { cleanupFailed = true; }
  }
  evidence.ownedProductProfilesCleaned = !cleanupFailed;
  if (cleanupFailed) evidence.result = "failed";
  writeFileSync(report, JSON.stringify(evidence, null, 2));
  if (cleanupFailed) throw new Error("owned product profile cleanup failed");
}
