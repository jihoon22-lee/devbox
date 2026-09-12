// Windows-only execution, real descendant listener and retained request checks.
// Every process/file belongs to this disposable runner fixture.
import assert from "node:assert/strict";
import {mkdirSync,writeFileSync,readFileSync,existsSync} from "node:fs";
import path from "node:path";
import {randomUUID} from "node:crypto";
import {setTimeout as delay} from "node:timers/promises";

export async function exerciseWorkspaceRuntime({cdp,directory,call,success,waitForRenderer}) {
  assert.equal(process.platform,"win32");
  assert.equal(process.env.GITHUB_ACTIONS,"true");assert.equal(process.env.RUNNER_ENVIRONMENT,"github-hosted");
  const root=path.join(directory,"Runtime owned fixture");mkdirSync(root);
  const script=path.join(root,"listener.cjs"),ready=path.join(root,"ready.json"),starts=path.join(root,"starts.txt");
  writeFileSync(script,`const fs=require('node:fs'),http=require('node:http'),cp=require('node:child_process');
const root=__dirname;
if(process.argv.includes('--child')){
 const server=http.createServer((_,response)=>response.end('synthetic'));
 server.listen(0,'127.0.0.1',()=>{fs.writeFileSync(root+'/ready.json',JSON.stringify({port:server.address().port,pid:process.pid}));console.log('synthetic-child-ready');});
}else{
 fs.appendFileSync(root+'/starts.txt','start\\n');
 cp.fork(__filename,['--child'],{stdio:'inherit'});console.log('synthetic-parent-ready');setInterval(()=>{},1000);
}`,{flag:"wx"});
  const runtime=(method,args={})=>call("workspace.runtime",method,args,29000);
  const observe=()=>call("workspace.processes","list_port_observations",{},29000);
  const logs=(method,args={})=>call("workspace.logs",method,args,29000);
  const control=(method,args,id=randomUUID())=>runtime("runtime_control",{operationId:id,method,args});
  const failed=result=>assert.equal(result.operation.outcome.state,"failed",JSON.stringify(result));
  const until=async(action,message,timeout=20000)=>{const end=performance.now()+timeout;let value;do{value=await action();if(value)return value;await delay(150);}while(performance.now()<end);assert.fail(message);};
  const navigate=route=>cdp.evaluate(`(async()=>{const d=await window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe");const label=d.features.find(feature=>feature.route===${JSON.stringify(route)}).label;Array.from(document.querySelectorAll('nav[aria-label="제품 화면"] button')).find(button=>button.textContent.trim()===label).click();})()`);
  const job=success(await runtime("create_job",{input:{name:"Owned Runtime fixture",command:`"${process.execPath}" "${script}"`,cwd:root,targetKind:"windows",targetDistro:null,environment:{action:"keep"},cronExpr:"0 0 1 1 *",enabled:false,overlapPolicy:"skip",catchUp:false}}));
  let stopped=false;
  try {
    // A direct legacy side-effect command is never admitted by the product.
    let rejected=false;try{await runtime("run_job_now",{id:job.id});}catch{rejected=true;}assert.ok(rejected);
    const operationId=randomUUID();const first=success(await control("run_job_now",{id:job.id},operationId));
    const second=success(await control("run_job_now",{id:job.id},operationId));assert.deepEqual(second,first);
    const child=await until(async()=>existsSync(ready)&&JSON.parse(readFileSync(ready,"utf8")),"Owned child did not listen");
    assert.equal(readFileSync(starts,"utf8"),"start\n");
    const run=await until(async()=>success(await runtime("get_active_run",{id:job.id})),"Active run is missing");
    const findOwned=async()=>{
      const snapshot=success(await observe());
      const row=snapshot.rows.find(row=>row.pid===child.pid&&row.port===child.port&&row.local_addr===`127.0.0.1:${child.port}`);
      const correlation=row?.correlations.find(item=>item.target_id===job.id&&item.confidence==="verified");
      return correlation&&{row,correlation};
    };
    const observed=await until(findOwned,"Task descendant lacks exact Runtime correlation");
    await navigate("runtime");
    await waitForRenderer(cdp,'!!document.querySelector(".workspace-feature-runtime:not([hidden])")',"Runtime route is missing");
    success(await call("workspace.processes","open_port_owner",{actionKey:observed.correlation.action_key},29000));
    await waitForRenderer(cdp,'!!document.querySelector(".workspace-feature-tasks:not([hidden])")',"Owning task navigation failed");
    const endpoint={proto:observed.row.proto,local_addr:observed.row.local_addr,port:observed.row.port,state:observed.row.state};
    const broker=success(await call("workspace.process-actions","kill_listener",{request:{endpoint,identity:observed.row.identity}},29000));
    assert.deepEqual(broker,{kind:"ownedTask",taskId:job.id});
    assert.equal((await fetch(`http://127.0.0.1:${child.port}`)).status,200,"Broker killed an owned descendant");
    const mismatch={...observed.row.identity,start_time:String(BigInt(observed.row.identity.start_time)+1n)};
    failed(await call("workspace.process-actions","kill_listener",{request:{endpoint,identity:mismatch}},29000));
    assert.equal((await fetch(`http://127.0.0.1:${child.port}`)).status,200);
    // Explicit reconnect renews an opaque reference; no path goes to Logs.
    const resolved=success(await logs("reconnect_runtime_sources",{sources:[{kind:"runtimeRun",runId:run.id,stream:"stdout",revision:"1".repeat(64)}],filter:{text:"",regex:false}}));
    assert.equal(resolved.unavailableSources,0);assert.match(resolved.sources[0].revision,/^[a-f0-9]{64}$/);
    const read=()=>logs("read_sources",{sources:resolved.sources,cursors:[null],sequenceStarts:[0],generation:1,operationId:randomUUID()});
    const snapshot=success(await read());assert.ok(JSON.stringify(snapshot).includes("synthetic-child-ready"));
    await navigate("runtime");
    success(await call("workspace.processes","open_port_log",{actionKey:observed.correlation.action_key,stream:"stdout"},29000));
    await waitForRenderer(cdp,'!!document.querySelector(".workspace-feature-logs:not([hidden])")',"Runtime log navigation failed");
    // Reload keeps the native process owner and replays the original receipt.
    await cdp.command("Page.reload");
    await waitForRenderer(cdp,'!!document.querySelector(".workspace-registry")',"Workspace did not reload");
    assert.deepEqual(success(await control("run_job_now",{id:job.id},operationId)),first);
    assert.equal(readFileSync(starts,"utf8"),"start\n");
    assert.equal((await fetch(`http://127.0.0.1:${child.port}`)).status,200);
    await navigate("tasks");
    success(await control("stop_active_run",{id:job.id}));stopped=true;
    await until(async()=>{try{await fetch(`http://127.0.0.1:${child.port}`,{signal:AbortSignal.timeout(500)});return false;}catch{return true;}},"Stopped Runtime left a child listener");
    failed(await call("workspace.processes","open_port_log",{actionKey:observed.correlation.action_key,stream:"stdout"},29000));
    assert.equal(success(await runtime("get_active_run",{id:job.id})),null);
    const terminal=success(await read());assert.ok(JSON.stringify(terminal).includes("synthetic-child-ready"),"Terminalization changed a retained log reference");
    return {duplicateReceipt:true,rendererReload:true,ownedDescendant:true,exactCreationMismatch:true,ownerBrokerRoutesWithoutKill:true,listenerLogTaskNavigation:true,terminalLogLease:true,staleActionRejected:true,stopWaitsForDescendants:true};
  } finally {
    if(!stopped)await control("stop_active_run",{id:job.id}).catch(()=>{});
    await runtime("delete_job",{id:job.id}).catch(()=>{});
  }
}
