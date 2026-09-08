// Actual pinned v0.7 executables create the source schemas. Native profiles must
// be absent before this disposable hosted-runner fixture claims them.
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, copyFileSync, existsSync, readdirSync, lstatSync, rmSync } from "node:fs";
import path from "node:path";
import { tmpdir } from "node:os";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { createHash, randomUUID } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import { Cdp, unusedPort, waitForCdp, windowsLocalAppData, windowsProcessIsElevated, inspectElevatedCdpPolicy, installElevatedCdpPolicy, restoreElevatedCdpPolicy } from "./windows-packaged-smoke.mjs";
assert.equal(process.platform, "win32"); assert.equal(process.env.GITHUB_ACTIONS, "true"); assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
const directory = mkdtempSync(path.join(tmpdir(), "devbox-knowledge-migration-fixture-"));
const base = windowsLocalAppData();
const report = "product-foundation-evidence/knowledge-migration.json";
mkdirSync(path.dirname(report), { recursive: true });
const evidence = { source: process.env.GITHUB_SHA, environment: "github-hosted-windows", step: "baseline", result: "failed" };
const historyFixture = JSON.parse(readFileSync("apps/devbox-knowledge/src-tauri/fixtures/legacy-activity-history.json", "utf8"));
const baseline = JSON.parse(readFileSync(".github/scripts/product-foundation-baseline.json", "utf8"));
const digest = file => createHash("sha256").update(readFileSync(file)).digest("hex");
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const progress = step => { evidence.step = step; writeFileSync(report, JSON.stringify(evidence, null, 2)); };
const profiles = [], live = new Set(), databases = new Set();
function openDatabase(file, options = {}) { const db = new DatabaseSync(file, options); databases.add(db); return db; }
const elevated = windowsProcessIsElevated();
const apps = [
  { app: "knowledge-base", identifier: "com.devbox.knowledgebase", title: "Knowledge Base", source: "notes" },
  { app: "life-log", identifier: "com.devbox.lifelog", title: "Life Log", source: "activity" },
  { app: "everything-plus", identifier: "com.devbox.everythingplus", title: "Everything+", source: "search" },
];
function gh(args) {
  const result = spawnSync("gh", args, { encoding: "utf8", maxBuffer: 2 * 1024 * 1024, windowsHide: true });
  if (result.status !== 0) throw new Error("pinned baseline identity or download failed"); return result.stdout;
}
function absent(file) {
  try { lstatSync(file); } catch (error) { if (error.code === "ENOENT") return; throw error; }
  throw new Error("migration fixture requires absent legacy native profiles");
}
function claim(app) {
  absent(path.join(base, app.identifier)); absent(path.join(base, app.identifier.replace("com.devbox.", "com.workbench.")));
  const root = path.join(base, app.identifier); mkdirSync(root);
  const owner = randomUUID(), marker = path.join(root, `.knowledge-fixture-${owner}`);
  writeFileSync(marker, owner, { flag: "wx" });
  const profile = { ...app, root, owner, marker, identity: lstatSync(root, { bigint: true }) }; profiles.push(profile); return profile;
}
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
async function legacy(item, method, args = {}) {
  return item.cdp.evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(method)}, ${JSON.stringify(args)})`);
}
async function command(item, component, method, args = {}) {
  const route = component === "knowledge.activity" ? "activity" : component.includes("search") ? "search" : "notes";
  return item.cdp.evaluate(`(async () => { const invoke=window.__TAURI_INTERNALS__.invoke; const d=await invoke("plugin:product-shell|describe");
    const header={protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+5000,route:${JSON.stringify(route)}};
    return invoke("plugin:knowledge|execute",{request:{header,component:${JSON.stringify(component)},method:${JSON.stringify(method)},args:${JSON.stringify(args)}}}); })()`);
}
async function job(item, method, args) {
  const started = await command(item, "knowledge.migration", method, args); assert.equal(started.operation.outcome.state, "succeeded");
  for (let i = 0; i < 400; i++) {
    const result = await command(item, "knowledge.migration", "import_job", { jobId: started.value.jobId });
    assert.equal(result.operation.outcome.state, "succeeded");
    if (result.value.state !== "running") return result.value;
    await delay(150);
  }
  throw new Error("migration job deadline exceeded");
}
function logicalSources() {
  const values = {};
  for (const profile of profiles) {
    const db = openDatabase(path.join(profile.root, "data.db"), { readOnly: true });
    const tables = profile.source === "notes" ? ["settings", "note_templates"] : profile.source === "activity" ? ["settings", "sessions", "knowledge_draft_history"] : ["roots", "saved_queries", "meta"];
    values[profile.source] = tables.map(table => ({ table, schema: db.prepare("SELECT sql FROM sqlite_master WHERE name=?").get(table).sql, rows: db.prepare(`SELECT * FROM ${table} ORDER BY 1`).all() })); db.close();
  }
  return createHash("sha256").update(JSON.stringify(values)).digest("hex");
}
async function product(executable, profile) {
  const before = new Set(readdirSync(base).filter(name => name.startsWith("com.devbox.v08.knowledge.i")));
  const item = await start(executable, "Devbox Knowledge", profile);
  await wait(item.cdp, '!!document.querySelector(".knowledge-startup") || !!document.querySelector(".knowledge-migration") || !!document.querySelector(".knowledge-feature-notes .app")', "Knowledge startup missing");
  const added = readdirSync(base).filter(name => name.startsWith("com.devbox.v08.knowledge.i") && !before.has(name));
  if (added.length) { assert.equal(added.length, 1); item.dataRoot = path.join(base, added[0]); }
  return item;
}
async function prepare(item) {
  const result = await job(item, "prepare_import", { sources: ["notes", "activity", "search"] });
  assert.equal(result.state, "succeeded"); return result.value.plan;
}
const vault = path.join(directory, "original-vault"), indexed = path.join(directory, "indexed-files"), unused = path.join(directory, "deleted-root");
let originalNote, originalSecond, originalImage;
try {
  for (const id of ["com.devbox.activitytimeline", "com.workbench.activitytimeline"]) absent(path.join(base, id));
  for (const app of apps) claim(app);
  for (const relative of ["Projects", "Notes/assets", "Journal", "Reference", "Archive"]) mkdirSync(path.join(vault, relative), { recursive: true });
  mkdirSync(indexed); mkdirSync(unused);
  writeFileSync(path.join(vault, "Notes/original.md"), "# Original\n\n[[second]]\n\n![](assets/pixel.png)\n", { flag: "wx" });
  writeFileSync(path.join(vault, "Notes/second.md"), "# Second\n", { flag: "wx" });
  writeFileSync(path.join(vault, "Notes/assets/pixel.png"), Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a3ioAAAAASUVORK5CYII=", "base64"), { flag: "wx" });
  writeFileSync(path.join(indexed, "allowed.txt"), "knowledge migration searchable fixture", { flag: "wx" });
  writeFileSync(path.join(indexed, ".env"), "FIXTURE_TOKEN=synthetic-excluded", { flag: "wx" });
  originalNote = digest(path.join(vault, "Notes/original.md")); originalSecond = digest(path.join(vault, "Notes/second.md")); originalImage = digest(path.join(vault, "Notes/assets/pixel.png"));
  // Seed only the two legacy settings needed to avoid default document writes
  // and mask collection from the first poll. The pinned apps create all schemas.
  for (const profile of profiles.filter(profile => profile.source !== "search")) {
    const db = openDatabase(path.join(profile.root, "data.db"));
    db.exec("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL); PRAGMA journal_mode=WAL;");
    if (profile.source === "notes") db.prepare("INSERT INTO settings VALUES('root',?)").run(vault);
    else db.prepare("INSERT INTO settings VALUES('privacy_rules',?)").run(JSON.stringify({ excludedProcesses: [], excludedTitlePatterns: [], redactTitlePatterns: [], maskAllTitles: true }));
    db.close();
  }
  let object = JSON.parse(gh(["api", `repos/jihoon22-lee/devbox/git/ref/tags/${baseline.tag}`])).object;
  for (let depth = 0; object.type === "tag" && depth < 3; depth++) object = JSON.parse(gh(["api", `repos/jihoon22-lee/devbox/git/tags/${object.sha}`])).object;
  assert.equal(object.type, "commit"); assert.equal(object.sha, baseline.commit);
  const assets = path.join(directory, "assets"); mkdirSync(assets);
  gh(["release", "download", baseline.tag, "--repo", "jihoon22-lee/devbox", "--pattern", "release-manifest.json", "--dir", assets]);
  assert.equal(digest(path.join(assets, "release-manifest.json")), baseline.manifestSha256);
  const manifest = JSON.parse(readFileSync(path.join(assets, "release-manifest.json"), "utf8")); evidence.baseline = { commit: baseline.commit, binaries: [] };
  let oldNotes;
  for (const profile of profiles) {
    const entry = manifest.apps.find(app => app.id === profile.app).portable;
    gh(["release", "download", baseline.tag, "--repo", "jihoon22-lee/devbox", "--pattern", entry.name, "--dir", assets]);
    const binary = path.join(assets, entry.name); assert.equal(digest(binary), entry.sha256); assert.equal(lstatSync(binary).size, entry.size);
    evidence.baseline.binaries.push({ app: profile.app, sha256: entry.sha256 });
    const executable = path.join(directory, `legacy-${profile.app}-${randomUUID()}.exe`); copyFileSync(binary, executable);
    const item = await start(executable, profile.title, path.join(directory, `webview-${profile.app}`));
    await wait(item.cdp, '!!window.__TAURI_INTERNALS__ && !!document.querySelector(".app")', "legacy UI did not initialize");
    if (profile.source === "notes") {
      assert.equal(path.normalize(await legacy(item, "get_root")), path.normalize(vault));
      const removed = await legacy(item, "create_template", { draft: { name: "discarded fixture", content: "discarded" } });
      const kept = await legacy(item, "create_template", { draft: { name: "imported fixture", content: "# {{title}}\nlegacy template" } });
      await legacy(item, "delete_template", { id: removed.id }); evidence.sourceTemplateId = kept.id; oldNotes = item;
    } else if (profile.source === "activity") {
      await legacy(item, "stop_tracking"); assert.equal(await legacy(item, "is_tracking"), false);
      // The legacy close policy hides to tray. Terminate only this stopped,
      // disposable collector before adding exact synthetic historical rows.
      await stop(item, true);
      const db = openDatabase(path.join(profile.root, "data.db"));
      db.exec("DELETE FROM sessions; INSERT INTO sessions VALUES(71,'synthetic-editor.exe','',1709164800000,1709164860000,60000);");
      db.prepare("INSERT INTO settings(key,value) VALUES('product_activity_collection_v1','true')").run();
      db.prepare("INSERT INTO knowledge_draft_history VALUES(9,'0123456789abcdef0123456789abcdef','knowledge-draft/v1','pending',?,?,1,2,3,NULL)").run(JSON.stringify(historyFixture.summary), JSON.stringify(historyFixture.sources)); db.close();
    } else {
      await legacy(item, "add_root", { path: unused, indexContent: false });
      const first = (await legacy(item, "list_roots"))[0];
      await legacy(item, "remove_root", { path: unused });
      await legacy(item, "add_root", { path: indexed, indexContent: true });
      const root = (await legacy(item, "list_roots"))[0];
      await legacy(item, "save_saved_query", { request: { id: null, name: "live fixture", query: "allowed", filter: { sourceRootId: root.id } } });
      await legacy(item, "save_saved_query", { request: { id: null, name: "deleted fixture", query: "allowed", filter: { sourceRootId: first.id } } });
      evidence.sourceRootId = root.id; evidence.deletedSourceRootId = first.id; await stop(item);
    }
  }
  progress("legacy-writer-quiesce");
  const executable = path.join(directory, `knowledge-product-${randomUUID()}.exe`); copyFileSync(path.resolve("target/debug/devbox-knowledge.exe"), executable);
  const profile = path.join(directory, "product-webview"); let item = await product(executable, profile); const dataRoot = item.dataRoot; assert.ok(dataRoot);
  await click(item.cdp, "기존 앱 데이터 가져오기");
  await click(item.cdp, "가져오기 미리보기 준비");
  await wait(item.cdp, '!!document.querySelector("#import-preview-title")', "migration preview was not rendered");
  assert.equal(existsSync(path.join(dataRoot, "active-stores.json")), false);
  const firstPlan = (await command(item, "knowledge.migration", "list_imports")).value.find(plan => plan.phase === "prepared"); assert.ok(firstPlan);
  await click(item.cdp, "미리보기를 확인하고 적용");
  await wait(item.cdp, 'Array.from(document.querySelectorAll("[role=alert]")).some(alert => alert.textContent.includes("이전 Knowledge 앱을 완전히 종료"))', "active legacy writer was not rejected");
  assert.equal(existsSync(path.join(dataRoot, "active-stores.json")), false);
  await stop(oldNotes); await job(item, "discard_import", { planId: firstPlan.id });
  const frozen = logicalSources();
  progress("activate-and-read");
  await click(item.cdp, "시작 화면으로"); await click(item.cdp, "기존 앱 데이터 가져오기"); await click(item.cdp, "가져오기 미리보기 준비");
  await wait(item.cdp, '!!document.querySelector("#import-preview-title")', "new migration preview missing");
  const plan = (await command(item, "knowledge.migration", "list_imports")).value.find(plan => plan.phase === "prepared");
  assert.ok(plan.sources.some(source => source.report.reservedRootIds === 1));
  await click(item.cdp, "미리보기를 확인하고 적용");
  await wait(item.cdp, '!!document.querySelector(".knowledge-feature-notes .app")', "imported Notes did not mount after explicit UI approval");
  assert.equal((await command(item, "knowledge.migration", "status")).value.active, true);
  const templates = (await command(item, "knowledge.notes", "list_templates")).value;
  assert.equal(templates.length, 1); assert.equal(templates[0].content, "# {{title}}\nlegacy template"); assert.notEqual(templates[0].id, evidence.sourceTemplateId);
  assert.equal((await command(item, "knowledge.notes", "read_file", { rel: "Notes/original.md" })).value, "# Original\n\n[[second]]\n\n![](assets/pixel.png)\n");
  assert.equal((await command(item, "knowledge.activity", "get_privacy_rules")).value.maskAllTitles, true);
  const history = (await command(item, "knowledge.activity", "knowledge_draft_history")).value;
  assert.equal(history.length, 1); assert.equal(history[0].status, "expired"); assert.equal(history[0].summary.startDate, "2024-02-29");
  assert.equal((await command(item, "knowledge.activity", "is_tracking")).value, false);
  const roots = (await command(item, "knowledge.search", "list_roots")).value;
  const queries = (await command(item, "knowledge.search", "list_saved_queries")).value;
  assert.equal(roots.length, 1); assert.equal(queries.length, 2);
  const liveQuery = queries.find(query => query.name === "live fixture"), deletedQuery = queries.find(query => query.name === "deleted fixture");
  assert.equal(liveQuery.filter.sourceRootId, roots[0].id); assert.notEqual(deletedQuery.filter.sourceRootId, roots[0].id);
  const active = JSON.parse(readFileSync(path.join(dataRoot, "active-stores.json"), "utf8"));
  const activityDb = openDatabase(path.join(dataRoot, "stores", active.generation, "activity/data.db"), { readOnly: true });
  assert.equal(activityDb.prepare("SELECT duration_ms FROM sessions WHERE app='synthetic-editor.exe'").get().duration_ms, 60000);
  assert.equal(activityDb.prepare("SELECT status FROM knowledge_draft_history").get().status, "expired"); activityDb.close();
  await command(item, "knowledge.notes", "update_template", { id: templates[0].id, draft: { name: "edited product fixture", content: "new product edit" } });
  assert.equal((await command(item, "knowledge.notes", "create_file", { rel: "Notes/new-product.md", content: "# Keep new product note" })).operation.outcome.state, "succeeded");
  const additional = path.join(directory, "new-product-root"); mkdirSync(additional);
  assert.equal((await command(item, "knowledge.search-settings", "add_root", { path: additional, indexContent: false })).operation.outcome.state, "succeeded");
  const afterRoots = (await command(item, "knowledge.search", "list_roots")).value;
  assert.ok(afterRoots.every(root => root.id !== deletedQuery.filter.sourceRootId));
  assert.equal(logicalSources(), frozen);
  progress("activity-summary-preview");
  const markdownFiles = () => readdirSync(vault, { recursive: true }).filter(file => file.endsWith(".md")).sort();
  const beforeSummary = markdownFiles();
  await click(item.cdp, "활동");
  await wait(item.cdp, '!!document.querySelector(".knowledge-feature-activity input[type=date]")', "Activity date missing");
  await item.cdp.evaluate(`(() => { const input=document.querySelector(".knowledge-feature-activity input[type=date]");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set.call(input,"2024-02-29");
    input.dispatchEvent(new Event("input",{bubbles:true})); input.dispatchEvent(new Event("change",{bubbles:true})); })()`);
  await wait(item.cdp, `!!document.querySelector('[aria-label="2024-02-29 선택된 날짜"]') && document.querySelector(".knowledge-feature-activity").innerText.includes("synthetic-editor.exe")`, "selected-day Activity digest missing");
  await click(item.cdp, "Knowledge로 보내기");
  await wait(item.cdp, '!!document.querySelector(".knowledge-feature-notes:not([hidden]) [role=dialog]") && document.body.innerText.includes("Life Log 초안 미리보기")', "in-product summary preview did not activate Notes");
  assert.deepEqual(markdownFiles(), beforeSummary);
  const sentHistory = (await command(item, "knowledge.activity", "knowledge_draft_history")).value;
  const cancelledDraft = sentHistory.find(entry => entry.status === "sent"); assert.ok(cancelledDraft);
  assert.equal(cancelledDraft.summary.startDate, "2024-02-29");
  await click(item.cdp, "취소");
  await wait(item.cdp, '!document.querySelector(".knowledge-feature-notes [role=dialog]")', "summary cancellation did not release preview");
  assert.deepEqual(markdownFiles(), beforeSummary);
  await click(item.cdp, "활동"); await click(item.cdp, "Knowledge로 보내기");
  await wait(item.cdp, '!!document.querySelector(".knowledge-feature-notes:not([hidden]) [role=dialog]")', "regenerated summary preview missing");
  assert.deepEqual(markdownFiles(), beforeSummary);
  await click(item.cdp, "초안 저장");
  await wait(item.cdp, 'document.body.innerText.includes("Knowledge 초안을 저장했습니다")', "explicit summary save did not complete");
  const consumedHistory = (await command(item, "knowledge.activity", "knowledge_draft_history")).value;
  const consumed = consumedHistory.find(entry => entry.status === "consumed"); assert.ok(consumed); assert.notEqual(consumed.handoffId, cancelledDraft.handoffId);
  const afterSummary = markdownFiles(); assert.equal(afterSummary.length, beforeSummary.length + 1);
  const savedSummary = afterSummary.find(file => !beforeSummary.includes(file));
  assert.ok(readFileSync(path.join(vault, savedSummary), "utf8").includes("2024-02-29"));
  assert.equal((await command(item, "knowledge.notes", "save_knowledge_draft", { id: consumed.handoffId })).operation.outcome.state, "failed");
  assert.deepEqual(markdownFiles(), afterSummary); assert.equal(running(oldNotes), false);
  assert.equal((await command(item, "knowledge.activity", "is_tracking")).value, false);
  evidence.summaryPreviewDidNotWrite = true; evidence.summaryCancellationPreservedVault = true;
  evidence.summaryExplicitSaveConsumedOnce = true; evidence.summaryStayedInProduct = true;
  progress("second-installation-owner");
  const secondExe = path.join(directory, `knowledge-second-${randomUUID()}.exe`); copyFileSync(executable, secondExe);
  const second = await product(secondExe, path.join(directory, "second-webview"));
  const secondPlan = await prepare(second); const secondResult = await job(second, "activate_import", { planId: secondPlan.id });
  assert.equal(secondResult.state, "failed"); assert.equal(secondResult.issue, "vault_owner_busy");
  await job(second, "discard_import", { planId: secondPlan.id }); await stop(second);
  progress("restart-repeat-and-recovery");
  assert.equal((await command(item, "knowledge.migration", "schedule_import")).value.scheduled, true);
  await stop(item); item = await product(executable, profile);
  await wait(item.cdp, '!!document.querySelector("#migration-title")', "scheduled import did not pause engine startup");
  assert.equal((await command(item, "knowledge.migration", "status")).value.active, false);
  const rollback = await job(item, "rollback_import", { planId: plan.id }); assert.equal(rollback.state, "failed"); assert.equal(rollback.issue, "preview_stale");
  const repeat = await prepare(item); assert.ok(repeat.sources.every(source => source.report.imported === 0));
  const repeated = await job(item, "activate_import", { planId: repeat.id }); assert.equal(repeated.state, "succeeded");
  assert.equal((await command(item, "knowledge.notes", "list_templates")).value[0].content, "new product edit");
  assert.equal((await command(item, "knowledge.notes", "read_file", { rel: "Notes/new-product.md" })).value, "# Keep new product note");
  assert.equal((await command(item, "knowledge.search", "list_saved_queries")).value.length, 2);
  assert.equal((await command(item, "knowledge.activity", "is_tracking")).value, false);
  assert.equal(logicalSources(), frozen); assert.equal(digest(path.join(vault, "Notes/original.md")), originalNote); assert.equal(digest(path.join(vault, "Notes/second.md")), originalSecond); assert.equal(digest(path.join(vault, "Notes/assets/pixel.png")), originalImage);
  progress("explicit-close-policy");
  assert.equal((await command(item, "knowledge.activity", "set_close_policy", { closeToTray: true })).value.closeToTray, true);
  const hidden = closeWindow(item); await delay(500); assert.equal(running(item), true); assert.equal(visible(hidden), false);
  assert.equal((await command(item, "knowledge.activity", "is_tracking")).value, false);
  // Crash/restart checks persisted preference; this is explicitly not a tray
  // Quit test. Default close below must terminate the process normally.
  await stop(item, true); item = await product(executable, profile);
  assert.equal((await command(item, "knowledge.activity", "get_close_policy")).value.closeToTray, true);
  assert.equal((await command(item, "knowledge.activity", "set_close_policy", { closeToTray: false })).value.closeToTray, false);
  await stop(item);
  evidence.sourceLogicalSchemaPreserved = true; evidence.markdownAndAssetsPreserved = true; evidence.legacyWriterBlocked = true; evidence.secondInstallationBlocked = true;
  evidence.templateAndRootIdsRemapped = true; evidence.deletedRootNeverReused = true; evidence.collectionConsentNotImported = true; evidence.privacyPreserved = true;
  evidence.pendingDeliveryRetired = true; evidence.repeatNoDuplicates = true; evidence.newProductEditsPreserved = true; evidence.rollbackNewEditsRejected = true;
  evidence.closeToTrayKeepsProcess = true; evidence.closePreferenceSurvivesCrash = true; evidence.defaultCloseExits = true; evidence.result = "pass"; progress("complete");
} catch (error) { evidence.error = String(error.message).slice(0, 2000); throw error; }
finally {
  for (const item of live) { try { await stop(item, true); } catch { item.child?.kill(); if (item.policy) restoreElevatedCdpPolicy(item.policy); } }
  for (const db of databases) { try { db.close(); } catch { /* Already closed. */ } }
  let cleanupFailed = false;
  for (const profile of profiles) {
    try {
      const actual = lstatSync(profile.root, { bigint: true }); const marker = lstatSync(profile.marker);
      assert.ok(actual.isDirectory() && !actual.isSymbolicLink() && actual.dev === profile.identity.dev && actual.ino === profile.identity.ino);
      assert.ok(marker.isFile() && !marker.isSymbolicLink() && marker.size === profile.owner.length); assert.equal(readFileSync(profile.marker, "utf8"), profile.owner);
      rmSync(profile.root, { recursive: true, maxRetries: 20, retryDelay: 50 });
    } catch { cleanupFailed = true; }
  }
  evidence.ownedLegacyProfilesCleaned = !cleanupFailed;
  if (cleanupFailed) evidence.result = "failed";
  writeFileSync(report, JSON.stringify(evidence, null, 2));
  if (cleanupFailed) throw new Error("owned legacy migration profile cleanup failed");
}
