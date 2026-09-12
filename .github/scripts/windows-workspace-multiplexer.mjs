// Invoked only by the owned WSL2 runner, after tools are explicitly provisioned.
import assert from "node:assert/strict";
import {randomUUID} from "node:crypto";
import {setTimeout as delay} from "node:timers/promises";
export async function exerciseMultiplexerReconnect({call,success,connectTerminal,wsl,distro,multiplexer}){
  assert.ok(["tmux","zellij"].includes(multiplexer));
  const id=randomUUID(),root="/tmp/devbox-mux-"+id;
  const terminal=(method,args={})=>call("workspace.terminal",method,args);
  const until=async(check,message)=>{const end=performance.now()+45000;do{const value=await check();if(value)return value;await delay(150);}while(performance.now()<end);assert.fail(message);};
  wsl(["/usr/bin/python3","-c","import pathlib,sys;p=pathlib.Path(sys.argv[1]);p.mkdir();(p/'owner').write_text(sys.argv[2])",root,id]);
  let companion,opened=false,primary;
  const peer=async(method,args={})=>companion.evaluate("(async()=>{const invoke=window.__TAURI_INTERNALS__.invoke;const d=await invoke('plugin:workspace|terminal_describe');return invoke('plugin:workspace|terminal_execute',{request:{header:{protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+29000,route:'terminal',context:d.context},method:"+JSON.stringify(method)+",args:"+JSON.stringify(args)+"}});})()",{timeoutMs:35000});
  try{
    const store=success(await terminal("list_workspace_profiles"));
    const profile={id,name:"Owned "+multiplexer+" fixture",tabs:[{id:"main",title:"Fixture",layout:"grid",paneKeys:["pane"],sizing:{columns:[1],rows:[1]}}],panes:[{key:"pane",distro,cwd:root,multiplexer}],activeTabId:"main",activePaneKey:"pane"};
    success(await terminal("save_workspace_profile",{expectedRevision:store.revision,profile}));
    const selected=success(await terminal("terminal_commands")).profiles.find(value=>value.id===id);
    success(await terminal("open_terminal_profile",{operationId:id,profileId:id,revision:selected.revision}));opened=true;
    companion=await connectTerminal(id);
    const first=await until(async()=>{const rows=await peer("list_sessions");return rows.length===1&&rows[0];},"Multiplexer did not start");
    assert.equal(first.multiplexer,multiplexer);assert.equal(first.resumed,false);
    // A file proves the shell ran the command, independently of terminal echo.
    await peer("write_session",{sessionId:first.id,data:"printf 'first\\n' > "+root+"/proof\r"});
    await until(async()=>wsl(["/usr/bin/python3","-c","import pathlib,sys;p=pathlib.Path(sys.argv[1])/'proof';print(p.read_text() if p.exists() else '')",root])==="first","Multiplexer shell did not execute input");
    const layout=await peer("terminal_layout");
    layout.layout.panes[0].startCommand="touch "+root+"/must-not-run";
    await peer("save_terminal_layout",{expectedRevision:layout.revision,layout:layout.layout});
    success(await terminal("stop_terminal",{id}));opened=false;companion.close();companion=null;
    const request={id,operationId:randomUUID(),expectedGeneration:0};
    const restored=success(await terminal("restore_terminal",request));opened=true;
    assert.equal(restored.restoreOnly,true);assert.deepEqual(success(await terminal("restore_terminal",request)),restored);
    companion=await connectTerminal(id);
    const second=await until(async()=>{const rows=await peer("list_sessions");return rows.length===1&&rows[0];},"Multiplexer did not reconnect");
    assert.notEqual(first.id,second.id);assert.equal(second.multiplexer,multiplexer);assert.equal(second.resumed,true);
    await peer("write_session",{sessionId:second.id,data:"printf 'second\\n' >> "+root+"/proof\r"});
    await until(async()=>wsl(["/usr/bin/python3","-c","import pathlib,sys;print((pathlib.Path(sys.argv[1])/'proof').read_text())",root])==="first\nsecond","Reconnected multiplexer did not execute input");
    assert.equal(wsl(["/usr/bin/python3","-c","import pathlib,sys;print((pathlib.Path(sys.argv[1])/'must-not-run').exists())",root]),"False");
    assert.equal((await peer("terminal_layout")).layout.panes[0].startCommand,layout.layout.panes[0].startCommand);
    return {multiplexer,start:true,detachedSessionRetained:true,freshPtyAttach:true,noStartCommandReplay:true,definitionPreserved:true};
  }catch(error){primary=error;throw error;}
  finally{
    companion?.close();const errors=[];
    if(opened)try{success(await terminal("stop_terminal",{id}));opened=false;}catch(error){errors.push(error);}
    try{const store=success(await terminal("list_workspace_profiles"));if(store.profiles.some(value=>value.id===id))success(await terminal("delete_workspace_profile",{id,expectedRevision:store.revision}));}catch(error){errors.push(error);}
    // Multiplexer servers intentionally survive detach; the outer runner destroys
    // this entire disposable distro after checking all cases.
    if(!opened)try{wsl(["/usr/bin/python3","-c","import pathlib,shutil,sys;p=pathlib.Path(sys.argv[1]);assert (p/'owner').read_text()==sys.argv[2];shutil.rmtree(p)",root,id]);}catch(error){errors.push(error);}
    if(errors.length)throw new AggregateError(primary?[primary,...errors]:errors,"Multiplexer fixture cleanup incomplete");
  }
}
