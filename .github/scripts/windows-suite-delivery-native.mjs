// Actual installed products, native owner observations and activation gating.
// The PowerShell fixture owns the random installation and namespace cleanup.
import {requireHostedNetworkFixture} from "./fixture-network-safety.mjs";
import {freePort,connect,waitForRenderer} from "./workspace-cdp-fixture.mjs";
import {allWindowsProcesses,stopOwnedProcess,windowsProcessIsElevated,inspectElevatedCdpPolicy,installElevatedCdpPolicy,restoreElevatedCdpPolicy} from "./windows-packaged-smoke.mjs";
import assert from "node:assert/strict";
import {readFileSync,writeFileSync,mkdirSync,realpathSync,existsSync,appendFileSync} from "node:fs";
import path from "node:path";
import {spawn} from "node:child_process";
import {once} from "node:events";
import {setTimeout as delay} from "node:timers/promises";
requireHostedNetworkFixture();assert.equal(process.platform,"win32");
const [directory,mode,legacyDirectory,portableExecutable,legacyMarker]=process.argv.slice(2);assert.ok(["import","legacyImport","legacyReview","health","committed","cleanup"].includes(mode));
const root=realpathSync.native(directory);assert.ok(path.basename(path.dirname(root)).startsWith("devbox-suite-delivery-"));
const manifest=JSON.parse(readFileSync(path.join(root,"devbox-installation.json"),"utf8"));
const evidence={source:JSON.parse(readFileSync(path.join(root,"suite-payload.json"),"utf8")).sourceSha,fixtureSource:process.env.GITHUB_SHA,mode,installationId:manifest.installationId,generation:manifest.generation,checks:{},result:"failed",cleanup:[]};
const live=[];
const value=(result)=>{assert.equal(result.operation.outcome.state,"succeeded",JSON.stringify(result));return result.value;};
async function call(item,command,body,route){
 return item.cdp.evaluate(`(async()=>{const invoke=window.__TAURI_INTERNALS__.invoke;const d=await invoke('plugin:product-shell|describe');const header={protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+29000,route:${JSON.stringify(route)},context:d.context};try{return await invoke(${JSON.stringify(command)},{request:{header,...${JSON.stringify(body)}}});}catch(problem){throw new Error(JSON.stringify(problem).slice(0,2000));}})()`,{timeoutMs:35000});
}
async function start(member){
 const executable=realpathSync.native(path.join(root,member.executable)),port=await freePort();
 const policy=windowsProcessIsElevated()?inspectElevatedCdpPolicy(path.basename(executable),port):null;if(policy)installElevatedCdpPolicy(policy);
 const item={product:member.product,executable,policy,child:null,cdp:null};live.push(item);
 const env={...process.env,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port}`};for(const name of Object.keys(env))if(/TOKEN|SECRET|PASSWORD|PRIVATE_KEY|API_KEY/i.test(name))delete env[name];
 item.child=spawn(executable,[],{cwd:path.dirname(executable),env,stdio:["ignore","ignore","pipe"]});
 item.child.stderr.setEncoding("utf8");item.child.stderr.on("data",text=>{item.error=((item.error??"")+text).slice(-2000);});await once(item.child,"spawn");
 item.identity=allWindowsProcesses().find(row=>row.Pid===item.child.pid&&path.resolve(row.Path).toLowerCase()===path.resolve(executable).toLowerCase());assert.ok(item.identity);
 item.cdp=await connect(port,item.child);await waitForRenderer(item.cdp,"!!window.__TAURI_INTERNALS__","installed product bridge missing");
 await waitForRenderer(item.cdp,"!!document.querySelector('nav[aria-label=\"제품 화면\"]')","installed shell missing");
 assert.equal((await item.cdp.evaluate("window.__TAURI_INTERNALS__.invoke('plugin:product-shell|describe')")).deliveryState,mode.startsWith("legacy")?"import":mode==="cleanup"?"committed":mode);
 return item;
}
async function exerciseCleanup(center) {
 assert.match(legacyMarker,/^[0-9a-f-]{36}$/);
 const original=realpathSync.native(legacyDirectory),portable=realpathSync.native(portableExecutable);
 assert.ok(original.toLowerCase().startsWith(path.dirname(root).toLowerCase()+path.sep));
 const manager=path.resolve(portable,"../../../../..");
 assert.equal(path.basename(manager),"com.devbox.devboxmanager");
 assert.equal(readFileSync(path.join(manager,"suite-fixture-owner"),"utf8"),legacyMarker);
 const tools=async(method,args={})=>value(await call(center,"plugin:control-center|execute",{method,args},"migration"));
 const inventory=await tools("legacy_inventory");
 const installed=inventory.installers.entries.filter(row=>row.app==="port-manager"&&row.binary==="verified");assert.equal(installed.length,1);
 const installer=await tools("legacy_cleanup_preview",{kind:"installer",id:installed[0].registrationId});assert.equal(installer.state,"reviewed");
 const executable=path.join(original,"port-manager.exe");assert.ok(existsSync(executable));
 const script=path.join(path.dirname(root),"hold-legacy-image.ps1");
 writeFileSync(script,`param([string]$File)
$ErrorActionPreference='Stop'
$stream=[IO.File]::Open($File,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
try { [Console]::Out.WriteLine('held'); [Console]::Out.Flush(); [Console]::ReadLine() | Out-Null } finally { $stream.Dispose() }
`);
 const holder=spawn("powershell.exe",["-NoProfile","-NonInteractive","-File",script,"-File",executable],{stdio:["pipe","pipe","pipe"],windowsHide:true});
 try {
  let output="",errors="";holder.stdout.setEncoding("utf8");holder.stdout.on("data",value=>{output+=value;});holder.stderr.setEncoding("utf8");holder.stderr.on("data",value=>{errors+=value;});
  const deadline=Date.now()+10000;while(!output.includes("held")&&Date.now()<deadline&&holder.exitCode===null)await delay(100);assert.ok(output.includes("held"),errors);
  const partial=await tools("legacy_cleanup_apply",{id:installer.id});assert.equal(partial.state,"cleanupPending");assert.ok(existsSync(executable));
  evidence.checks.lockedLegacyCleanupRemainsPending=true;
 } finally {if(holder.exitCode===null){holder.stdin.end("release\n");await Promise.race([once(holder,"exit"),delay(10000)]);}if(holder.exitCode===null){holder.kill();throw new Error("Owned file holder did not stop");}}
 assert.equal((await tools("legacy_cleanup_apply",{id:installer.id})).state,"complete");
 assert.equal(existsSync(executable),false);assert.equal(readFileSync(path.join(original,"fixture-user-file.txt"),"utf8"),"preserve unlisted data");
 evidence.checks.verifiedInstallerResumePreservesUnknownFiles=true;
 const reviewed=await tools("legacy_cleanup_preview",{kind:"portable",id:"port-manager"});assert.equal(reviewed.state,"reviewed");
 const before=readFileSync(portable),registry=readFileSync(path.join(manager,"registry.json"));
 appendFileSync(portable,"changed fixture bytes");
 try {assert.equal((await tools("legacy_cleanup_apply",{id:reviewed.id})).state,"cleanupPending");assert.deepEqual(readFileSync(path.join(manager,"registry.json")),registry);}
 finally {writeFileSync(portable,before);}
 assert.equal((await tools("legacy_cleanup_apply",{id:reviewed.id})).state,"complete");assert.equal(existsSync(portable),false);
 assert.equal(readFileSync(path.join(manager,"suite-fixture-owner"),"utf8"),legacyMarker);
 evidence.checks.changedManagedPortableRefusedThenResumed=true;
 const recovery=await tools("suite_recovery");assert.equal(recovery.committed,true);assert.equal(recovery.cleanupPending,0);
}
try {
 const apps={};for(const member of manifest.members.filter(member=>mode!=="cleanup"||member.product==="control-center"))apps[member.product]=await start(member);
 if(mode==="import"||mode==="legacyImport") {
  for(const product of ["workspace","knowledge"]){const component=`${product}.migration`,route=product==="workspace"?"overview":"notes";value(await call(apps[product],`plugin:${product}|execute`,{component,method:"start_empty",args:{}},route));}
  value(await call(apps["api-studio"],"plugin:api-studio|execute",{component:"api-studio.migration",method:"finish_startup",args:{}},"requests"));
 }
 for(const item of Object.values(apps)) {
  const route={workspace:"overview","api-studio":"requests",knowledge:"notes","control-center":"migration"}[item.product];
  const review=value(await call(item,"plugin:suite|connection",{method:{kind:"preview"}},route));assert.equal(review.products.length,4);
  assert.equal(review.installationId,manifest.installationId);
  assert.ok(review.products.every(product=>product.available));
  // Health/reinstall/update reopen a remembered native connection. Preview
  // checks this exact package; approving an already connected bus is rejected.
  const status=value(await call(item,"plugin:suite|connection",{method:{kind:"status"}},route));
  if(!status.connected)value(await call(item,"plugin:suite|connection",{method:{kind:"approve",token:review.token,remember:true}},route));
  const connected=value(await call(item,"plugin:suite|connection",{method:{kind:"status"}},route));
  assert.equal(connected.connected,true);assert.equal(connected.generation,review.generation);
  evidence.checks[`connection_${item.product}`]=true;
 }
 const center=apps["control-center"];
 if(mode==="legacyImport") {
  const plan=value(await call(center,"plugin:commands|command_import_launcher",{method:"preview",id:null},"migration"));
  assert.equal(value(await call(center,"plugin:commands|command_import_launcher",{method:"apply",id:plan.id},"migration")).committed,true);
  evidence.checks.actualLauncherImporter=true;
 }

 if(mode==="cleanup") {
  await exerciseCleanup(center);
 } else if(mode!=="committed") {
  for(const member of manifest.members) {
   const method=mode==="health"?"record_suite_health":"record_migration_owner";
   const result=value(await call(center,"plugin:control-center|execute",{method,args:{product:member.product}},mode==="health"?"recovery":"migration"));
   assert.equal(result.recorded,true);evidence.checks[`recorded_${member.product}`]=true;
  }
  if(mode.startsWith("legacy")) {
   const review=value(await call(center,"plugin:control-center|execute",{method:"cutover_review",args:{}},"migration"));
   const source=review.sources.find(row=>row.identifier==="com.devbox.devboxlauncher");assert.ok(source);
   assert.equal(source.currentImport,mode==="legacyImport");
   const prepared=value(await call(center,"plugin:control-center|execute",{method:"prepare_cutover",args:{revision:review.revision,choices:review.sources.map(row=>({identifier:row.identifier,disposition:row.currentImport?"acceptedImport":"keepLegacy"}))}},"migration"));
   assert.equal(prepared.prepared,true);evidence.checks.explicitSourceDispositions=true;
  }
  const blocked=await call(apps.workspace,"plugin:workspace|execute",{component:"workspace.runtime",method:"list_jobs",args:{}},"tasks").catch(()=>null);
  assert.ok(blocked===null||blocked.operation.outcome.state!=="succeeded","ordinary Runtime remains blocked before commit");evidence.checks.businessGate=true;
 } else {
  for(const member of manifest.members){const result=value(await call(center,"plugin:suite|connection",{method:{kind:"readHealthStatus",product:member.product}},"recovery"));assert.equal(result.nativeStoreReady,true);}
  evidence.checks.fourCommittedNativeOwners=true;
 }
 evidence.result="passed";
}catch(error){evidence.failure=String(error).slice(0,3000);process.exitCode=1;}
finally {
 for(const item of live.reverse()) {
  // Request ordinary window shutdown first, including owner cleanup. Forceful
  // fixture cleanup is restricted to the captured synthetic process identity.
  try {await item.cdp?.evaluate("window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})").catch(()=>{});await delay(400);}
  catch{}
  item.cdp?.close();
  try {if(item.identity)await stopOwnedProcess(item.identity,item.executable,item.child);else if(item.child?.exitCode===null){item.child.kill();await Promise.race([once(item.child,"exit"),delay(10000)]);}}
  catch(error){evidence.cleanup.push({product:item.product,issue:String(error)});evidence.result="failed";process.exitCode=1;}
  finally{if(item.policy)restoreElevatedCdpPolicy(item.policy);}
  evidence.cleanup.push({product:item.product,nativeError:item.error,exited:!item.identity||!allWindowsProcesses().some(row=>row.Pid===item.identity.Pid&&row.Created===item.identity.Created)});
 }
 mkdirSync("product-foundation-evidence",{recursive:true});writeFileSync(`product-foundation-evidence/suite-delivery-${mode}-${Date.now()}.json`,JSON.stringify(evidence,null,2));
}
