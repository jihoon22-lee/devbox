import {exerciseNativeWslTasks} from "./windows-workspace-tasks-wsl.mjs";
// Actual Workspace companions and native Session ownership, using owned fixture data only.
import assert from "node:assert/strict";
import {mkdirSync,writeFileSync,readFileSync,realpathSync,rmSync} from "node:fs";
import {spawnSync} from "node:child_process";
import {randomUUID} from "node:crypto";
import path from "node:path";
import {setTimeout as delay} from "node:timers/promises";

export async function exerciseWorkspaceTerminalSessions({cdp,directory,call,success,connectTerminal}) {
  assert.equal(process.platform,"win32");
  assert.equal(process.env.GITHUB_ACTIONS,"true");assert.equal(process.env.RUNNER_ENVIRONMENT,"github-hosted");
  const owner=JSON.parse(readFileSync(path.join(process.env.RUNNER_TEMP,"devbox-knowledge-wsl-owner.json"),"utf8").replace(/^\uFEFF/,""));
  const distro=process.env.DEVBOX_KNOWLEDGE_WSL_DISTRO;
  assert.equal(owner.runId,process.env.GITHUB_RUN_ID);assert.equal(owner.name,distro);
  assert.match(distro,/^DevboxKnowledgeFixture-[0-9]+-[a-f0-9]{12}$/);
  const wsl=(args,input)=>{
    const result=spawnSync("wsl.exe",["--distribution",distro,"--exec",...args],{input,encoding:"utf8",timeout:15000,windowsHide:true});
    assert.equal(result.status,0,result.stderr);return result.stdout.trim();
  };
  return exerciseTerminalSessionFixture({cdp,directory,call,success,connectTerminal,distro,wsl});
}
export async function exerciseTerminalSessionFixture({cdp,directory,call,success,connectTerminal,distro,wsl,multiplexer="native"}) {
  const until=async(check,message)=>{const deadline=performance.now()+45000;do{const value=await check();if(value)return value;await delay(150);}while(performance.now()<deadline);assert.fail(message);};
  const original=(await cdp.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe")')).context;
  const nonce=randomUUID(),root=path.join(directory,"b06-"+nonce),linux="/tmp/devbox-b06-"+nonce;
  mkdirSync(root);writeFileSync(path.join(root,".fixture-owner"),nonce,{flag:"wx"});const rootIdentity=realpathSync.native(root);
  const runtime=(method,args={})=>call("workspace.runtime",method,args,29000);
  const terminal=(method,args={})=>call("workspace.terminal",method,args,29000);
  const registry=(method,args={})=>call("workspace.registry",method,args,29000);
  const sessions=async()=>success(await terminal("development_sessions"));
  const sessionIds=[];let service,windowId,profileId,companion,context,primary;
  wsl(["/usr/bin/python3","-c","import os,sys; os.mkdir(sys.argv[1]); os.mkdir(sys.argv[1]+'/work'); open(sys.argv[1]+'/.fixture-owner','w').write(sys.argv[2])",linux,nonce]);
  try {
    const reviewed=success(await registry("preview_windows",{root}));
    context=success(await registry("apply_registration",{previewId:reviewed.previewId,name:"B06 owned Session fixture",action:"register"})).context;
    success(await registry("select_project",{context}));
    const script=path.join(root,"service.cjs");writeFileSync(script,"console.log('synthetic-session-ready');setInterval(()=>{},1000);\n",{flag:"wx"});
    service=success(await runtime("create_service",{input:{name:"B06 shared service",command:'"'+process.execPath+'" "'+script+'"',cwd:root,targetKind:"windows",targetDistro:null,environment:{action:"clear"},restartPolicy:"never",autoStart:false,healthTcpAddress:null,healthTcpPort:null}}));
    const start=async()=>{
      const prepared=success(await terminal("prepare_development_session",{operationId:randomUUID(),jobs:[service.id],terminalProfile:null}));
      assert.equal(prepared.preflight.executionBlocked,false,JSON.stringify(prepared.preflight));
      sessionIds.push(prepared.session.id);
      const args={id:prepared.session.id,revision:prepared.session.revision,planRevision:prepared.session.planRevision,mode:"startReviewed"};
      success(await terminal("start_development_session",args));
      await until(async()=>(await sessions()).sessions.find(value=>value.id===prepared.session.id&&value.phase==="active"),"Session did not become active");
      return prepared.session.id;
    };
    const first=await start();const generation=success(await runtime("get_service_instance",{id:service.id})).generation;
    const second=await start();
    assert.equal(success(await runtime("get_service_instance",{id:service.id})).generation,generation);
    success(await terminal("stop_development_session",{id:first}));
    await until(async()=>(await sessions()).sessions.find(value=>value.id===first&&value.phase==="stopped"),"First Session did not stop");
    assert.equal(success(await runtime("get_service_instance",{id:service.id})).state,"running");
    success(await terminal("stop_development_session",{id:second}));
    await until(async()=>success(await runtime("get_service_instance",{id:service.id})).state==="stopped","Last holder did not retire the created service");
    const stopped=await until(async()=>(await sessions()).sessions.find(value=>value.id===second&&value.phase==="stopped"),"Second Session did not stop");
    const summaryArgs={operationId:randomUUID(),sessionId:second,revision:stopped.revision,includeProblems:false};
    const summary=success(await terminal("prepare_session_summary",summaryArgs));
    assert.equal(summary.draft.metadata.failedRuns,null);assert.equal(summary.draft.metadata.gitCommits,null);
    assert.deepEqual(success(await terminal("prepare_session_summary",summaryArgs)),summary);
    assert.ok(!JSON.stringify(summary).includes(script));

    const store=success(await terminal("list_workspace_profiles"));
    profileId=randomUUID();
    const profile={id:profileId,name:"B06 native companion",tabs:[{id:"main",title:"Fixture",layout:"grid",paneKeys:["one","two"],sizing:{columns:[0.5,0.5],rows:[1]}}],panes:[{key:"one",distro,cwd:linux+"/work",multiplexer},{key:"two",distro,cwd:linux+"/work",multiplexer}],activeTabId:"main",activePaneKey:"two"};
    success(await terminal("save_workspace_profile",{expectedRevision:store.revision,profile}));
    const command=success(await terminal("terminal_commands")).profiles.find(value=>value.id===profileId);
    assert.ok(command);assert.ok(!JSON.stringify(command).includes(linux));
    windowId=randomUUID();
    success(await terminal("open_terminal_profile",{operationId:windowId,profileId,revision:command.revision}));
    companion=await connectTerminal(windowId);
    const invoke=async(method,args={})=>companion.evaluate("(async()=>{const invoke=window.__TAURI_INTERNALS__.invoke;const d=await invoke('plugin:workspace|terminal_describe');return invoke('plugin:workspace|terminal_execute',{request:{header:{protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+29000,route:'terminal',context:d.context},method:"+JSON.stringify(method)+",args:"+JSON.stringify(args)+"}});})()",{timeoutMs:35000});
    const panes=await until(async()=>{const value=await invoke("list_sessions");return value.length===2&&value;},"Companion did not restore both panes");
    assert.deepEqual(panes.map(value=>value.paneKey).sort(),["one","two"]);
    const nativeId=panes.find(value=>value.paneKey==="two").id;
    assert.equal((await invoke("terminal_window_policy")).closeBehavior,"hideToTray");
    await invoke("write_session",{sessionId:nativeId,data:"printf 'synthetic-b06-output\\n'\r"});
    await until(async()=>(await invoke("terminal_output",{sessionId:nativeId,after:0})).frames.map(frame=>frame.data).join("").includes("\r\nsynthetic-b06-output\r\n"),"PTY output did not arrive");
    success(await terminal("focus_terminal",{id:windowId}));
    await until(async()=>(await invoke("terminal_window_policy")).focused,"Companion did not receive native focus");
    const hide={operationId:randomUUID(),terminalId:windowId,deadlineMs:Date.now()+29000};
    const hidden=success(await terminal("summon_terminal",hide));assert.equal(hidden.visible,false);
    assert.deepEqual(success(await terminal("summon_terminal",hide)),hidden);
    assert.equal((await invoke("terminal_window_policy")).visible,false);
    await invoke("write_session",{sessionId:nativeId,data:"printf 'synthetic-hidden-output\\n'\r"});
    await until(async()=>(await invoke("terminal_output",{sessionId:nativeId,after:0})).frames.map(frame=>frame.data).join("").includes("\r\nsynthetic-hidden-output\r\n"),"Hidden companion lost its PTY");
    success(await terminal("focus_terminal",{id:windowId}));
    await companion.command("Page.addScriptToEvaluateOnNewDocument",{source:"const previous=HTMLCanvasElement.prototype.getContext;window.__fixtureWebGLDenied=0;HTMLCanvasElement.prototype.getContext=function(kind,...args){if(String(kind).includes('webgl')){window.__fixtureWebGLDenied++;return null;}return previous.call(this,kind,...args);};"});
    await companion.command("Page.reload");
    await until(async()=>{try{return (await invoke("list_sessions")).some(value=>value.id===nativeId);}catch{return false;}},"Companion reload lost native ownership");
    await until(async()=>companion.evaluate("document.querySelectorAll('.xterm').length===2&&window.__fixtureWebGLDenied>0"),"WebGL fallback was not exercised");
    await invoke("write_session",{sessionId:nativeId,data:"sleep 30\r"});
    await delay(200);await invoke("write_session",{sessionId:nativeId,data:"\u0003"});
    await invoke("write_session",{sessionId:nativeId,data:"printf 'synthetic-after-sigint\\n'\r"});
    await until(async()=>(await invoke("terminal_output",{sessionId:nativeId,after:0})).frames.map(frame=>frame.data).join("").includes("\r\nsynthetic-after-sigint\r\n"),"SIGINT killed the companion PTY");
    assert.deepEqual((await invoke("list_sessions")).map(value=>value.id).sort(),panes.map(value=>value.id).sort());
    // Explicit reopen keeps the multiplexer name but allocates a fresh native PTY.
    success(await terminal("stop_terminal",{id:windowId}));companion.close();companion=null;
    const restore={id:windowId,operationId:randomUUID(),expectedGeneration:0};
    const restored=success(await terminal("restore_terminal",restore));
    assert.equal(restored.id,windowId);assert.equal(restored.restoreOnly,true);
    assert.deepEqual(success(await terminal("restore_terminal",restore)),restored);
    companion=await connectTerminal(windowId);
    const reconnected=await until(async()=>{const value=await invoke("list_sessions");return value.length===2&&value;},"Reopened companion did not restore panes");
    assert.ok(reconnected.every(value=>!panes.some(old=>old.id===value.id)));
    assert.ok(reconnected.every(value=>value.multiplexer===multiplexer));
    if(multiplexer!=="native")assert.ok(reconnected.every(value=>value.resumed));
    await assert.rejects(()=>invoke("write_initial_command",{sessionId:reconnected[0].id,data:"printf 'must-not-run'\r"}));
    const nativeTasks=await exerciseNativeWslTasks({cdp,call,success,distro,wsl,root:linux+"/project"});
    return {nativeTasks,multiplexer,stateOnlyReconnect:true,sharedServiceLastHolderStop:true,summaryReceiptAndUnknownCounts:true,profileRevision:true,twoPanes:true,hiddenOutput:true,reloadKeepsPty:true,forcedWebglFallback:true,sigintPreservesPty:true};
  } catch(error) { primary=error;throw error; }
  finally {
    const errors=[];
    const attempt=async(action)=>{try{await action();return true;}catch(error){errors.push(error);return false;}};
    companion?.close();
    let retired=!primary?.preserveFixtures;
    if(windowId)retired=(await attempt(async()=>success(await terminal("stop_terminal",{id:windowId}))))&&retired;
    for(const id of sessionIds){
      const stopped=await attempt(async()=>{
        success(await terminal("stop_development_session",{id}));
        await until(async()=>(await sessions()).sessions.find(value=>value.id===id&&value.phase==="stopped"),"Session cleanup did not finish");
        success(await terminal("archive_development_session",{id}));
      });
      retired=stopped&&retired;
    }
    if(service)retired=(await attempt(async()=>success(await runtime("delete_service",{id:service.id}))))&&retired;
    if(profileId)await attempt(async()=>{const store=success(await terminal("list_workspace_profiles"));success(await terminal("delete_workspace_profile",{id:profileId,expectedRevision:store.revision}));});
    await attempt(async()=>{if(original)success(await registry("select_project",{context:original}));else success(await registry("clear_project"));});
    if(retired){
      if(context)await attempt(async()=>{const state=success(await registry("snapshot"));success(await registry("remove",{revision:state.revision,context}));});
      await attempt(async()=>{
        assert.equal(realpathSync.native(root),rootIdentity);assert.equal(readFileSync(path.join(root,".fixture-owner"),"utf8"),nonce);
        rmSync(root,{recursive:true});
      });
      await attempt(async()=>wsl(["/usr/bin/python3","-c","import pathlib,shutil,sys; p=pathlib.Path(sys.argv[1]); assert (p/'.fixture-owner').read_text()==sys.argv[2]; shutil.rmtree(p)",linux,nonce]));
    }else errors.push(new Error("Unconfirmed retirement: owned project fixtures preserved"));
    if(errors.length)throw new AggregateError(primary?[primary,...errors]:errors,"B06 fixture cleanup incomplete");
  }
}
