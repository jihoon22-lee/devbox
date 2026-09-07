// Disposable Windows-only migration acceptance using the actual pinned v0.7 API
// executable. All data, profiles, ports and new product identities are synthetic.
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, copyFileSync, existsSync, readdirSync, lstatSync, opendirSync } from "node:fs";
import path from "node:path";
import { tmpdir } from "node:os";
import { spawn, spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { createServer } from "node:http";
import { once } from "node:events";
import { DatabaseSync } from "node:sqlite";
import { Cdp, unusedPort, waitForCdp, windowsProcessIsElevated, inspectElevatedCdpPolicy, installElevatedCdpPolicy, restoreElevatedCdpPolicy } from "./windows-packaged-smoke.mjs";
assert.equal(process.platform, "win32"); assert.equal(process.env.GITHUB_ACTIONS, "true"); assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
const directory = mkdtempSync(path.join(tmpdir(), "devbox-api-migration-fixture-"));
const evidence = { source: process.env.GITHUB_SHA, environment: "github-hosted-windows", step: "baseline", result: "failed" };
const report = "product-foundation-evidence/api-migration.json"; mkdirSync(path.dirname(report), { recursive: true });
const baseline = JSON.parse(readFileSync(".github/scripts/product-foundation-baseline.json", "utf8"));
const digest = (file) => createHash("sha256").update(readFileSync(file)).digest("hex");
const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
const progress = (step) => { evidence.step = step; writeFileSync(report, JSON.stringify(evidence, null, 2)); };
function gh(args) { const result = spawnSync("gh", args, { encoding: "utf8", maxBuffer: 2 * 1024 * 1024, windowsHide: true }); if (result.status !== 0) throw new Error("pinned baseline download/identity check failed"); return result.stdout; }
function json(args) { return JSON.parse(gh(args)); }
function childEnvironment(extra) {
  const env = { ...process.env, ...extra };
  for (const name of Object.keys(env)) if (/TOKEN|SECRET|PASSWORD|PRIVATE_KEY|API_KEY/i.test(name)) delete env[name];
  return env;
}
const elevated = windowsProcessIsElevated(); const live = new Set();
async function start(executable, title, profile, extra = {}) {
  const port = await unusedPort(); const policy = elevated ? inspectElevatedCdpPolicy(path.basename(executable), port) : null;
  const item = { policy, child: null, cdp: null }; live.add(item);
  if (policy) installElevatedCdpPolicy(policy);
  item.child = spawn(executable, title === "Devbox API Studio" ? ["--import-legacy"] : [], { env: childEnvironment({ WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`, WEBVIEW2_USER_DATA_FOLDER: profile, ...extra }), stdio: "ignore" });
  await once(item.child, "spawn");
  const target = await waitForCdp(port, title); item.cdp = new Cdp(target.webSocketDebuggerUrl); await item.cdp.connect(); return item;
}
async function stop(item) {
  item.cdp?.close();
  if (item.child?.exitCode === null) {
    // Close only the exact fixture process created above, allowing its profile
    // to flush and release LevelDB LOCK before the importer takes its snapshot.
    spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", `$p=[Diagnostics.Process]::GetProcessById(${item.child.pid}); $null=$p.CloseMainWindow()`], { stdio: "ignore", windowsHide: true });
    for (let i = 0; i < 100 && item.child.exitCode === null; i++) await delay(100);
    if (item.child.exitCode === null) { item.child.kill(); throw new Error("fixture process did not close normally"); }
  }
  if (item.policy) restoreElevatedCdpPolicy(item.policy); live.delete(item);
}
async function wait(cdp, expression, label, timeout = 90_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) { if (await cdp.evaluate(expression)) return; await delay(150); }
  evidence.ui = await cdp.evaluate('(document.body?.innerText ?? "").slice(0, 10000)'); throw new Error(label);
}
async function click(cdp, label) {
  // Initial startup markup precedes its async source discovery. Wait for an
  // actionable control, not just the button's label in that loading markup.
  await wait(cdp, `(() => { const button = Array.from(document.querySelectorAll("button")).find(button => button.textContent.trim() === ${JSON.stringify(label)}); return !!button && !button.disabled; })()`, `fixture control is not ready: ${label}`);
  return cdp.evaluate(`(() => { const button = Array.from(document.querySelectorAll("button")).find(button => button.textContent.trim() === ${JSON.stringify(label)}); if (!button || button.disabled) throw new Error("fixture control unavailable"); button.click(); return true; })()`);
}
async function command(cdp, component, method, args = {}) {
  const route = component === "api-studio.webhooks" ? "webhooks" : component === "api-studio.transforms" ? "transforms" : "requests";
  return cdp.evaluate(`(async () => { const invoke=window.__TAURI_INTERNALS__.invoke; const d=await invoke("plugin:product-shell|describe"); const header={protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+5000,route:${JSON.stringify(route)}}; return invoke("plugin:api-studio|execute",{request:{header,component:${JSON.stringify(component)},method:${JSON.stringify(method)},args:${JSON.stringify(args)}}}); })()`);
}
function sourceHashes() {
  const api = path.join(directory, "com.devbox.apiplayground");
  const candidates = ["EBWebView/Default/Local Storage/leveldb", "Default/Local Storage/leveldb"].map(relative => path.join(api, relative)).filter(existsSync);
  assert.equal(candidates.length, 1, "one actual WebView2 LevelDB layout is required");
  const hashes = {};
  for (const name of readdirSync(candidates[0]).sort()) { const file = path.join(candidates[0], name); assert.ok(lstatSync(file).isFile()); hashes[`api/${name}`] = digest(file); }
  for (const relative of ["com.devbox.webhooklab/fixtures.json", "com.devbox.developertoolbox/smart-workflows.json", `com.devbox.webhooklab/service-profiles/${profileId}.json`]) hashes[relative] = digest(path.join(directory, relative));
  return hashes;
}
// Metadata only, inside this run's newly created product root. Never read copy
// contents or follow a link, and never include an absolute path in evidence.
function inspectOwnedStaging(dataRoot) {
  const base = path.join(dataRoot, "imports/staging");
  const result = { entries: 0, files: 0, directories: 0, links: [], errors: [], longestRelativePath: 0, bounded: true };
  const pending = [base];
  while (pending.length && result.entries < 20000) {
    const current = pending.pop(); const relative = path.relative(base, current);
    try {
      const entry = lstatSync(current); result.entries++;
      result.longestRelativePath = Math.max(result.longestRelativePath, relative.length);
      if (entry.isSymbolicLink()) { if (result.links.length < 16) result.links.push(relative); continue; }
      if (!entry.isDirectory()) { result.files++; continue; }
      result.directories++; const directory = opendirSync(current);
      try {
        for (let child; (child = directory.readSync()) !== null;) {
          if (pending.length + result.entries >= 20000) { result.bounded = false; break; }
          pending.push(path.join(current, child.name));
        }
      } finally { directory.closeSync(); }
    } catch (error) { if (result.errors.length < 16) result.errors.push({ relative, code: error.code ?? "unknown" }); }
  }
  if (pending.length) result.bounded = false;
  return result;
}
const nativeFixture = JSON.parse(readFileSync("apps/devbox-api-studio/src-tauri/fixtures/legacy-native.json", "utf8"));
const profileId = nativeFixture.profile.id;
let hits = 0; const server = createServer((_, response) => { hits++; response.setHeader("Content-Type", "application/json"); response.end('{"fixture":true}'); });
let ownedProductDataRoot = null;
server.listen(0, "127.0.0.1"); await once(server, "listening"); const fixtureUrl = `http://127.0.0.1:${server.address().port}/imported`;
try {
  let object = json(["api", `repos/jihoon22-lee/devbox/git/ref/tags/${baseline.tag}`]).object;
  for (let depth = 0; object.type === "tag" && depth < 3; depth++) object = json(["api", `repos/jihoon22-lee/devbox/git/tags/${object.sha}`]).object;
  assert.equal(object.type, "commit"); assert.equal(object.sha, baseline.commit);
  const assets = path.join(directory, "assets"); mkdirSync(assets);
  gh(["release", "download", baseline.tag, "--repo", "jihoon22-lee/devbox", "--pattern", "release-manifest.json", "--dir", assets]);
  const manifestFile = path.join(assets, "release-manifest.json"); assert.equal(digest(manifestFile), baseline.manifestSha256);
  const manifest = JSON.parse(readFileSync(manifestFile, "utf8")); const entry = manifest.apps.find(app => app.id === "api-playground").portable;
  assert.equal(entry.name, "api-playground.exe");
  gh(["release", "download", baseline.tag, "--repo", "jihoon22-lee/devbox", "--pattern", entry.name, "--dir", assets]);
  const binary = path.join(assets, entry.name); assert.equal(digest(binary), entry.sha256); assert.equal(lstatSync(binary).size, entry.size);
  evidence.baseline = { commit: baseline.commit, binarySha256: entry.sha256 };
  const oldExe = path.join(directory, `legacy-api-${randomUUID()}.exe`); copyFileSync(binary, oldExe);
  const old = await start(oldExe, "API Playground", path.join(directory, "com.devbox.apiplayground"));
  await wait(old.cdp, 'localStorage.getItem("apip-collections-v1-migrated") === "2" && localStorage.getItem("apip-history-v1-migrated") === "2"', "legacy bootstrap did not finish");
  assert.equal(await old.cdp.evaluate(`(async () => {
    const sealed = await window.__TAURI_INTERNALS__.invoke("seal_secret", {value:"migration-fixture-secret"});
    const request={method:"POST",url:${JSON.stringify(fixtureUrl)},headers:[{key:"X-Repeat",value:"one",enabled:true},{key:"X-Repeat",value:"two",enabled:false},{key:"Authorization",value:"\${TOKEN}",enabled:true}],cookies:[],multipart:[],params:[],body_kind:"json",body:'{"fixture":true}',auth:null,timeout_ms:30000,requiresSecretReview:true};
    localStorage.setItem("apip-environments",JSON.stringify({version:1,environments:[{id:"legacy-env",name:"fixture environment",variables:[{key:"TOKEN",value:sealed,secret:true},{key:"BROKEN",value:"unavailable-fixture-envelope",secret:true}]}]}));
    localStorage.setItem("apip-collections-v2",JSON.stringify({version:2,collections:[{id:"legacy-collection",name:"fixture collection",folder:"fixture",saved_at:1000,request,requiresSecretReview:true}]}));
    localStorage.setItem("apip-history-v2",JSON.stringify({version:2,history:[{id:"legacy-history",saved_at:1000,request,status:201}]}));
    localStorage.setItem("apip-history","synthetic excluded v1 history");
    localStorage.setItem("devbox.api-playground.grpc-history/v1",JSON.stringify({schema:"devbox.api-playground.grpc-history/v1",entries:[{sourceKind:"local-proto",service:"fixture.Service",method:"Call",rpcKind:"unary",requestMessageCount:1,responseMessageCount:1,startedAtMs:1000,elapsedMs:1,status:"OK",tlsMode:"plaintext",credentialUsed:false}]}));
    return JSON.parse(localStorage.getItem("apip-collections-v2")).collections.length;
  })()`), 1);
  await delay(500); await stop(old); await delay(500);
  mkdirSync(path.join(directory, "com.devbox.webhooklab/service-profiles"), { recursive: true }); mkdirSync(path.join(directory, "com.devbox.developertoolbox"));
  writeFileSync(path.join(directory, "com.devbox.webhooklab/fixtures.json"), JSON.stringify(nativeFixture.fixtures), {flag:"wx"});
  writeFileSync(path.join(directory, `com.devbox.webhooklab/service-profiles/${profileId}.json`), JSON.stringify(nativeFixture.profile), {flag:"wx"});
  writeFileSync(path.join(directory, "com.devbox.developertoolbox/smart-workflows.json"), JSON.stringify(nativeFixture.workflows), {flag:"wx"});
  const frozen = sourceHashes(); progress("legacy-seeded");
  const productExe = path.join(directory, `api-product-${randomUUID()}.exe`); copyFileSync(path.resolve("target/debug/devbox-api-studio.exe"), productExe);
  const beforeRoots = new Set(readdirSync(process.env.LOCALAPPDATA).filter(name => name.startsWith("com.devbox.v08.apistudio.i")));
  const profile = path.join(directory, "product-webview"); const env = { DEVBOX_API_MIGRATION_FIXTURE_ROOT: directory };
  let product = await start(productExe, "Devbox API Studio", profile, env);
  await wait(product.cdp, '(document.body?.innerText ?? "").includes("선택한 데이터 확인")', "migration startup gate did not open");
  const roots = readdirSync(process.env.LOCALAPPDATA).filter(name => name.startsWith("com.devbox.v08.apistudio.i") && !beforeRoots.has(name)); assert.equal(roots.length, 1);
  const dataRoot = path.join(process.env.LOCALAPPDATA, roots[0]); ownedProductDataRoot = dataRoot;
  assert.equal(await product.cdp.evaluate('(async()=>{try{await window.__TAURI_INTERNALS__.invoke("plugin:api-studio|legacy_export_message",{request:{nonce:"a".repeat(32),action:{kind:"open"}}});return false}catch{return true}})()'), true);
  progress("first-preview"); await click(product.cdp, "선택한 데이터 확인");
  await wait(product.cdp, '!!document.querySelector("[data-migration-review]")', "real legacy export did not produce a plan");
  await click(product.cdp, "이 계획으로 가져오기"); await wait(product.cdp, '(document.body?.innerText ?? "").includes("데이터를 가져왔습니다")', "first import did not finish");
  await click(product.cdp, "API Studio 열기"); await wait(product.cdp, '!!document.querySelector(".url-input")', "features did not open after activation");
  const state = await product.cdp.evaluate('({collections:JSON.parse(localStorage.getItem("apip-collections-v2")),history:JSON.parse(localStorage.getItem("apip-history-v2")),environments:JSON.parse(localStorage.getItem("apip-environments")),grpc:JSON.parse(localStorage.getItem("devbox.api-playground.grpc-history/v1"))})');
  assert.equal(state.collections.collections.length, 1); assert.equal(state.history.history.length, 1); assert.equal(state.environments.environments.length, 1); assert.equal(state.grpc.entries.length, 1);
  assert.deepEqual(state.collections.collections[0].request.headers.slice(0,2), [{key:"X-Repeat",value:"one",enabled:true},{key:"X-Repeat",value:"two",enabled:false}]);
  assert.equal(state.environments.environments[0].variables.find(variable => variable.key === "BROKEN").value, "");
  const sanitized = await command(product.cdp, "api-studio.api", "sanitize_persisted_json", {serialized:'{"visible":"migration-fixture-secret"}',environment:state.environments.environments[0].variables});
  assert.ok(sanitized.value.includes("[REDACTED]")); assert.ok(!sanitized.value.includes("migration-fixture-secret"));
  assert.equal((await command(product.cdp, "api-studio.webhooks", "server_status")).value.running, false);
  assert.equal((await command(product.cdp, "api-studio.webhooks", "list_fixtures")).value.length, 1);
  assert.ok((await command(product.cdp, "api-studio.transforms", "load_workflow_metadata")).value.metadata.favoriteTools.includes("hash"));
  assert.ok(existsSync(path.join(dataRoot, `webhooks/service-profiles/${profileId}.json`)));
  assert.equal(hits, 0, "import must not replay a request"); assert.deepEqual(sourceHashes(), frozen);
  await product.cdp.evaluate('(()=>{const store=JSON.parse(localStorage.getItem("apip-collections-v2"));store.collections[0].name="edited product collection";localStorage.setItem("apip-collections-v2",JSON.stringify(store));return true})()');
  await delay(200); await stop(product); progress("repeat-preview");
  product = await start(productExe, "Devbox API Studio", profile, env);
  await wait(product.cdp, '(document.body?.innerText ?? "").includes("선택한 데이터 확인")', "repeat startup gate did not open");
  await click(product.cdp, "선택한 데이터 확인"); await wait(product.cdp, '!!document.querySelector("[data-migration-review]")', "repeat review failed");
  assert.ok((await product.cdp.evaluate('document.querySelector("[data-migration-review]").textContent')).includes("추가 0개"));
  await click(product.cdp, "이 계획으로 가져오기"); await wait(product.cdp, '(document.body?.innerText ?? "").includes("데이터를 가져왔습니다")', "repeat import failed");
  await click(product.cdp, "API Studio 열기"); await wait(product.cdp, '!!document.querySelector(".url-input")', "repeat activation did not open features");
  assert.equal(await product.cdp.evaluate('JSON.parse(localStorage.getItem("apip-collections-v2")).collections[0].name'), "edited product collection");
  assert.equal((await command(product.cdp, "api-studio.webhooks", "list_fixtures")).value.length, 1); assert.equal(hits, 0); await stop(product);
  assert.deepEqual(sourceHashes(), frozen);
  const installation = roots[0].split(".i").at(-1); const db = new DatabaseSync(path.join(dataRoot, `v08/api-studio/${installation}/migration/destination.db`), {readOnly:true});
  const mapping = db.prepare("SELECT source_id,destination_id FROM studio_import_receipts_v1 WHERE source_store='fixtures'").all();
  assert.deepEqual(mapping.map(row=>({...row})), [{source_id:"fixture-7",destination_id:"fixture-1"}]); db.close();
  evidence.originalPreserved = true; evidence.repeatNoDuplicates = true; evidence.productEditPreserved = true; evidence.dpapiReused = true; evidence.missingSecretRequiresReconnect = true; evidence.noAutomaticRequests = true; evidence.result = "pass"; progress("complete");
} catch (error) {
  evidence.error = error.message;
  if (ownedProductDataRoot) evidence.ownedStaging = inspectOwnedStaging(ownedProductDataRoot);
  const current = Array.from(live).at(-1);
  if (current?.cdp) {
    try { const status = await command(current.cdp, "api-studio.migration", "migration_status"); evidence.migrationStage = status.value?.stage ?? "unavailable"; } catch { /* Preserve the original failure. */ }
  }
  throw error;
}
finally { for (const item of live) { try { await stop(item); } catch { item.child?.kill(); if(item.policy)restoreElevatedCdpPolicy(item.policy); } } await new Promise(resolve=>server.close(resolve)); writeFileSync(report, JSON.stringify(evidence,null,2)); }
