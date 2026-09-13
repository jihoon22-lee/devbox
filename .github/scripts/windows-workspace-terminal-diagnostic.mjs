// Exact prior CI artifact, disposable hosted VM, companion restore only.
import {requireHostedNetworkFixture} from "./fixture-network-safety.mjs";
import {freePort,connect,waitForRenderer} from "./workspace-cdp-fixture.mjs";
import {workspaceRequestExpression} from "./windows-workspace-registration.mjs";
import assert from "node:assert/strict";
import {createHash,randomUUID} from "node:crypto";
import {readFileSync,writeFileSync,mkdirSync,lstatSync,existsSync,realpathSync,rmSync,copyFileSync} from "node:fs";
import {spawn} from "node:child_process";
import {once} from "node:events";
import {nativeWindowState,windowsProcessIsElevated,inspectElevatedCdpPolicy,installElevatedCdpPolicy,restoreElevatedCdpPolicy} from "./windows-packaged-smoke.mjs";
import {setTimeout as delay} from "node:timers/promises";
import path from "node:path";
const hosted=requireHostedNetworkFixture();assert.equal(process.platform,"win32");
const [artifact,priorRun]=process.argv.slice(2);assert.match(priorRun,/^\d+$/);
const json=p=>JSON.parse(readFileSync(p,"utf8").replace(/^\uFEFF/,""));
const metadata=json(path.join(artifact,"fixture.json"));assert.equal(metadata.runId,priorRun);assert.match(metadata.sourceSha,/^[a-f0-9]{40}$/);
assert.equal(metadata.purpose,"owned-workspace-webview-acceptance");assert.equal(metadata.buildProfile,"debug");
const expected=new Set(["devbox-workspace.exe","THIRD_PARTY_NOTICES.md","resources/wsl/manifest.json","resources/wsl/devbox-workspace-wsl"]);
for(const file of metadata.files){assert.ok(expected.delete(file.path));const selected=path.join(artifact,file.path);assert.ok(lstatSync(selected).isFile());assert.equal(createHash("sha256").update(readFileSync(selected)).digest("hex"),file.sha256);}
assert.equal(expected.size,0);
const owner=json(path.join(process.env.RUNNER_TEMP,"devbox-knowledge-wsl-owner.json"));assert.equal(owner.runId,hosted.runId);assert.equal(owner.name,process.env.DEVBOX_KNOWLEDGE_WSL_DISTRO);
const imageName="devbox-workspace-diagnostic-"+randomUUID()+".exe";
const executable=path.join(artifact,imageName);copyFileSync(path.join(artifact,"devbox-workspace.exe"),executable);
const installationId=createHash("sha256").update('\\\\?\\'+realpathSync.native(executable)).digest("hex");
const dataRoot=path.join(process.env.LOCALAPPDATA,`com.devbox.v08.workspace.i${installationId}`);assert.equal(existsSync(dataRoot),false);
const evidenceDirectory=path.join(process.env.GITHUB_WORKSPACE,"product-foundation-evidence");mkdirSync(evidenceDirectory,{recursive:true});
const evidence={result:"diagnostic",artifactSource:metadata.sourceSha,artifactRun:priorRun,diagnosticSource:process.env.GITHUB_SHA,diagnosticRun:hosted.runId,environment:"github-hosted-windows-wsl1",observations:{}};
let child,main,companion,policy;let appExited=false;
try {
 const port=await freePort();policy=windowsProcessIsElevated()?inspectElevatedCdpPolicy(imageName,port):null;if(policy)installElevatedCdpPolicy(policy);
 child=spawn(executable,["--route=overview"],{cwd:artifact,env:{...process.env,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port}`,WEBVIEW2_USER_DATA_FOLDER:path.join(artifact,"webview2")},stdio:["ignore","ignore","pipe"]});
 child.stderr.setEncoding("utf8");child.stderr.on("data",value=>{evidence.nativeError=((evidence.nativeError??"")+value).slice(-16000);});child.on("exit",()=>{appExited=true;});await once(child,"spawn");
 main=await connect(port,child);await waitForRenderer(main,"!!window.__TAURI_INTERNALS__","Workspace did not start");
 const success=result=>{assert.equal(result.operation.outcome.state,"succeeded",JSON.stringify(result));return result.value;};
 const call=(component,method,args={})=>main.evaluate(workspaceRequestExpression(component,method,args,29000),{timeoutMs:35000});
 success(await call("workspace.migration","start_empty"));
 const root=path.join(artifact,"synthetic-project");mkdirSync(root);const registration=success(await call("workspace.registry","preview_windows",{root}));
 const context=success(await call("workspace.registry","apply_registration",{previewId:registration.previewId,name:"Terminal diagnostic",action:"register"})).context;
 success(await call("workspace.registry","select_project",{context}));
 const profiles=success(await call("workspace.terminal","list_workspace_profiles"));
 const profileId=randomUUID(),windowId=randomUUID();
 success(await call("workspace.terminal","save_workspace_profile",{expectedRevision:profiles.revision,profile:{id:profileId,name:"Native two-pane diagnostic",tabs:[{id:"main",title:"Diagnostic",layout:"grid",paneKeys:["one","two"],sizing:{columns:[0.5,0.5],rows:[1]}}],panes:[{key:"one",distro:owner.name,cwd:"/home/devbox-fixture",multiplexer:"native"},{key:"two",distro:owner.name,cwd:"/home/devbox-fixture",multiplexer:"native"}],activeTabId:"main",activePaneKey:"two"}}));
 const descriptor=success(await call("workspace.terminal","terminal_commands")).profiles.find(profile=>profile.id===profileId);assert.ok(descriptor);
 success(await call("workspace.terminal","open_terminal_profile",{operationId:windowId,profileId,revision:descriptor.revision}));
 companion=await connect(port,child,performance.now()+45000,windowId);
 const invoke=(method,args={})=>companion.evaluate("(async()=>{const invoke=window.__TAURI_INTERNALS__.invoke;const d=await invoke('plugin:workspace|terminal_describe');return invoke('plugin:workspace|terminal_execute',{request:{header:{protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+29000,route:'terminal',context:d.context},method:"+JSON.stringify(method)+",args:"+JSON.stringify(args)+"}});})()",{timeoutMs:35000});
 const deadline=performance.now()+45000;
 do { evidence.observations.sessions=await invoke("list_sessions");if(evidence.observations.sessions.length===2)break;await delay(300); } while(performance.now()<deadline);
 evidence.observations.renderer=await companion.evaluate("({text:(document.body?.innerText??'').slice(0,16000),alerts:[...document.querySelectorAll('[role=alert]')].map(node=>node.textContent),storage:[...Array(localStorage.length)].map((_,i)=>localStorage.key(i))})");
 for(const method of ["terminal_layout","terminal_window_policy","list_distros"]){try{evidence.observations[method]=await invoke(method);}catch(error){evidence.observations[method]={error:String(error)};}}
 if(evidence.observations.sessions.length!==2){
   evidence.observations.explicitNativeStarts=[];
   for(const paneKey of ["two","one"]){
     try { await invoke("reset_failed_pane",{paneKey}); const result=await invoke("start_session",{paneKey,distro:owner.name,cwd:"/home/devbox-fixture",multiplexer:"native"}); evidence.observations.explicitNativeStarts.push({paneKey,result}); }
     catch(error){evidence.observations.explicitNativeStarts.push({paneKey,error:String(error)});}
   }
   evidence.observations.sessionsAfterExplicitStart=await invoke("list_sessions");
 }
 assert.equal(evidence.observations.sessions.length,2,"Companion did not restore both panes; diagnostic state retained");
 evidence.observations.twoPanesRestored=true;
 const title=await companion.evaluate("document.title");
 evidence.observations.focus=[];
 for(const mode of ["native-request","cdp-bring-to-front","native-request-again"]){
   if(mode==="cdp-bring-to-front")await companion.command("Page.bringToFront");
   else success(await call("workspace.terminal","focus_terminal",{id:windowId}));
   await delay(500);
   evidence.observations.focus.push({mode,policy:await invoke("terminal_window_policy"),documentFocus:await companion.evaluate("document.hasFocus()"),native:nativeWindowState(child.pid,title)});
 }

} catch(error) { evidence.failure=String(error);process.exitCode=1; }
finally {
 companion?.close();main?.close();
 if(child&&child.exitCode===null){const exited=once(child,"exit");child.kill();await Promise.race([exited,delay(10000)]);}
 if(policy)restoreElevatedCdpPolicy(policy);
 evidence.appExited=!child||appExited;
 if(evidence.appExited&&existsSync(dataRoot)){rmSync(dataRoot,{recursive:true});evidence.ownedDataRemoved=!existsSync(dataRoot);}
 writeFileSync(path.join(evidenceDirectory,"terminal-companion-diagnostic.json"),JSON.stringify(evidence,null,2));
}
