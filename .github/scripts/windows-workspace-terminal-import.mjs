// Uses the owned primary's real WebView localStorage, copied only while closed.
import assert from "node:assert/strict";
import {mkdirSync,writeFileSync,readFileSync,readdirSync,realpathSync,rmSync} from "node:fs";
import {createHash,randomUUID} from "node:crypto";
import {spawnSync} from "node:child_process";
import path from "node:path";
import {setTimeout as delay} from "node:timers/promises";
import {workspaceRequestExpression} from "./windows-workspace-registration.mjs";
const success=result=>{assert.equal(result.operation.outcome.state,"succeeded",JSON.stringify(result));return result.value;};
const contents=root=>{
  const result=[];
  const visit=(directory,relative="")=>{for(const item of readdirSync(directory,{withFileTypes:true})){
    assert.ok(!item.isSymbolicLink());const name=relative+item.name,full=path.join(directory,item.name);
    if(item.isDirectory())visit(full,name+"/");
    else result.push([name,createHash("sha256").update(readFileSync(full)).digest("hex")]);
  }};
  visit(root);return result.sort((a,b)=>a[0].localeCompare(b[0]));
};
export function cleanupTerminalImport(fixture){
  if(!fixture||fixture.removed)return;
  assert.equal(realpathSync.native(fixture.legacy),fixture.identity);
  assert.equal(readFileSync(path.join(fixture.legacy,".fixture-owner"),"utf8"),fixture.nonce);
  rmSync(fixture.legacy,{recursive:true});fixture.removed=true;
}
export async function seedTerminalImport(cdp){
  assert.equal(process.env.GITHUB_ACTIONS,"true");assert.equal(process.env.RUNNER_ENVIRONMENT,"github-hosted");
  const nonce=randomUUID(),legacy=path.join(process.env.LOCALAPPDATA,"com.devbox.wsldesktop");
  mkdirSync(legacy);writeFileSync(path.join(legacy,".fixture-owner"),nonce,{flag:"wx"});
  const fixture={nonce,legacy,identity:realpathSync.native(legacy)};
  try {
  const id=randomUUID();
  const profile={id,name:"Synthetic legacy terminal",tabs:[{id:"old-tab",title:"Old",layout:"grid",paneKeys:["stable-old-pane"],sizing:{columns:[1],rows:[1]}}],panes:[{key:"stable-old-pane",distro:process.env.DEVBOX_KNOWLEDGE_WSL_DISTRO,multiplexer:"native",startCommand:"printf 'synthetic-migration-must-not-execute\\n'"}],activeTabId:"old-tab",activePaneKey:"stable-old-pane"};
  writeFileSync(path.join(legacy,"terminal-profiles.json"),JSON.stringify({version:2,profiles:[profile]}),{flag:"wx"});
  const values={"wsl-desktop:cwd-pinned":"1","wsl-desktop:cwd-value":"/tmp/devbox-import-"+nonce,"wsl-desktop:recent-paths":JSON.stringify(["/tmp/fixture-recent"]),"wsl-desktop:copy-on-select":"1","wsl-desktop:font-size":"17","wsl-desktop:settings":JSON.stringify({version:2,openTerminalOnStart:false,fontId:"consolas",theme:"light",keepInTray:true}),"wsl-desktop:last-layout":JSON.stringify({...profile,version:2})};
  const source=await cdp.evaluate("(async()=>{for(const [key,value] of Object.entries("+JSON.stringify(values)+"))localStorage.setItem(key,value);return window.__TAURI_INTERNALS__.invoke('plugin:path|resolve_directory',{directory:15});})()");
  assert.match(path.basename(source.replace(/[\\/]+$/,"")),/^com\.devbox\.v08\.workspace\.i[a-f0-9]{64}$/);
  return {...fixture,source,values};
  }catch(error){try{cleanupTerminalImport(fixture);}catch(cleanup){throw new AggregateError([error,cleanup],"Terminal fixture seed cleanup failed");}throw error;}
}
export function copyClosedTerminalImport(fixture){
  const result=spawnSync("powershell.exe",["-NoProfile","-NonInteractive","-File",path.resolve(".github/scripts/copy-owned-terminal-profile.ps1"),"-Source",fixture.source,"-Target",fixture.legacy,"-Nonce",fixture.nonce],{encoding:"utf8",timeout:35000,windowsHide:true});
  assert.equal(result.status,0,result.stderr);fixture.before=contents(fixture.legacy);
}
export async function verifyTerminalImport(cdp,fixture,connectTerminal){
  const call=async(method,args={})=>cdp.evaluate(workspaceRequestExpression("workspace.terminal",method,args,29000),{timeoutMs:35000});
  const until=async(check)=>{const deadline=performance.now()+90000;do{const value=await check();if(value)return value;await delay(200);}while(performance.now()<deadline);assert.fail("Terminal browser export did not finish");};
  const operation=randomUUID();let windowId,companion;
  try{
    const before=success(await call("list_workspace_profiles"));
    success(await call("start_terminal_import",{id:operation}));
    await until(async()=>{
      const job=success(await call("terminal_imports")).find(job=>job.id===operation);
      assert.notEqual(job?.state,"failed",JSON.stringify(job));return job?.state==="ready";
    });
    const review=success(await call("preview_terminal_import",{id:operation}));
    assert.equal(review.profiles.length,2);assert.equal(review.preferenceKeys.length,6);
    assert.ok(review.notices.every(notice=>!["missing","invalid-or-future-schema","unsupported-value"].includes(notice.state)),JSON.stringify(review.notices));
    const apply={id:operation,expectedRevision:review.revision,sourceRevision:review.sourceRevision,replacePreferences:true};
    assert.equal(success(await call("apply_terminal_import",apply)).repeated,false);
    assert.equal(success(await call("apply_terminal_import",apply)).repeated,true);
    assert.deepEqual(contents(fixture.legacy),fixture.before);
    assert.equal(success(await call("terminal_sessions")).filter(window=>window.state==="active").length,0);
    const imported=success(await call("list_workspace_profiles"));
    assert.equal(imported.profiles.length,before.profiles.length+2);
    const selected=imported.profiles.find(profile=>profile.name==="Synthetic legacy terminal");assert.ok(selected);
    assert.equal(selected.panes[0].key,"stable-old-pane");
    assert.ok(selected.panes[0].startCommand.includes("synthetic-migration-must-not-execute"));

    // Explicitly create an empty companion to read the native preference owner.
    windowId=randomUUID();success(await call("open_terminal",{operationId:windowId}));
    companion=await connectTerminal(windowId);
    const peer=async(method)=>companion.evaluate("(async()=>{const invoke=window.__TAURI_INTERNALS__.invoke;const d=await invoke('plugin:workspace|terminal_describe');return invoke('plugin:workspace|terminal_execute',{request:{header:{protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+29000,route:'terminal',context:d.context},method:"+JSON.stringify(method)+",args:{}}});})()");
    const preferences=await peer("terminal_preferences");
    for(const [key,value] of Object.entries(fixture.values).filter(([key])=>key!=="wsl-desktop:last-layout"))assert.equal(preferences[key],value);
    assert.equal((await peer("list_sessions")).length,0);
    companion.close();companion=null;success(await call("stop_terminal",{id:windowId}));windowId=null;
    success(await call("delete_workspace_profile",{id:selected.id,expectedRevision:imported.revision}));
    assert.equal(success(await call("apply_terminal_import",apply)).repeated,true);
    assert.ok(!success(await call("list_workspace_profiles")).profiles.some(profile=>profile.id===selected.id));
    const history=success(await call("terminal_import_history"));
    const restore=success(await call("preview_terminal_import_restore",{sourceRevision:history.entries[0].sourceRevision}));
    success(await call("restore_terminal_import",{sourceRevision:restore.sourceRevision,expectedRevision:restore.revision,restoreRevision:restore.restoreRevision}));
    assert.equal(success(await call("apply_terminal_import",apply)).repeated,true);
    assert.deepEqual(contents(fixture.legacy),fixture.before);
    return {realWebviewStorage:true,closedConsistentSource:true,allSevenKeys:true,originalStablePane:true,noAutomaticPty:true,repeatPreservesDeletion:true,reviewedPreimageRestore:true};
  }finally{
    companion?.close();
    if(windowId)success(await call("stop_terminal",{id:windowId}));
    // The outer runner retains this source until import workers finish or the
    // exact owned product process tree has retired after a failure.

  }
}
