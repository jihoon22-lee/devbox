// Runtime's Windows owner starts/stops only this run-owned WSL fixture group.
import assert from "node:assert/strict";
import {spawnSync} from "node:child_process";
import {readFileSync} from "node:fs";
import {randomUUID} from "node:crypto";
import path from "node:path";
import {setTimeout as delay} from "node:timers/promises";
export async function exerciseWorkspaceRuntimeWsl({call,success}) {
  assert.equal(process.env.GITHUB_ACTIONS,"true");assert.equal(process.env.RUNNER_ENVIRONMENT,"github-hosted");
  const owner=JSON.parse(readFileSync(path.join(process.env.RUNNER_TEMP,"devbox-knowledge-wsl-owner.json"),"utf8").replace(/^\uFEFF/,""));
  const distro=process.env.DEVBOX_KNOWLEDGE_WSL_DISTRO;
  assert.equal(owner.runId,process.env.GITHUB_RUN_ID);assert.equal(owner.name,distro);assert.match(distro,/^DevboxKnowledgeFixture-[0-9]+-[a-f0-9]{12}$/);
  const wsl=(args,input)=>{const result=spawnSync("wsl.exe",["--distribution",distro,"--exec",...args],{encoding:"utf8",input,timeout:15000,windowsHide:true});assert.equal(result.status,0,result.stderr);return result.stdout.trim();};
  // Hosted Windows uses an explicitly provisioned WSL1 fixture. Its kernel
  // does not expose listener rows; lifecycle remains required here and actual
  // listener correlation is exercised with the same body on owned WSL2.
  return exerciseRuntimeWslFixture({call,success,distro,wsl,wslVersion:1});
}

export async function exerciseRuntimeWslFixture({call,success,distro,wsl,wslVersion}) {
  assert.equal(process.platform,"win32");
  assert.ok(wslVersion===1||wslVersion===2);
  const root=`/tmp/devbox-runtime-${randomUUID()}`;
  const script=`import os,socket,sys,time
if os.fork()==0:
 s=socket.socket();s.bind(('127.0.0.1',0));s.listen(1)
 open('${root}/ready','w').write(str(os.getpid())+' '+str(s.getsockname()[1]))
 print('synthetic-wsl-listener',flush=True)
 print(os.getenv('FIXTURE_SECRET','missing'),flush=True)
 while True: time.sleep(1)
else:
 while True: time.sleep(1)
`;
  wsl(["/usr/bin/python3","-c",`import os,sys; os.mkdir('${root}'); open('${root}/listener.py','w').write(sys.stdin.read())`],script);
  const runtime=(method,args={})=>call("workspace.runtime",method,args,29000);
  const control=(method,args,operationId=randomUUID())=>runtime("runtime_control",{operationId,method,args});
  const until=async(fn,message)=>{const deadline=performance.now()+25000;do{const result=await fn();if(result)return result;await delay(200);}while(performance.now()<deadline);assert.fail(message);};
  const input={name:"Owned WSL Runtime fixture",command:`/usr/bin/python3 -u ${root}/listener.py`,cwd:root,targetKind:"wsl",targetDistro:distro,environment:{action:"replace",values:{FIXTURE_SECRET:"synthetic-wsl-private"}},restartPolicy:"never",autoStart:false,healthTcpAddress:null,healthTcpPort:null};
  const service=success(await runtime("create_service",{input}));let stopped=false;
  try {
    const operationId=randomUUID();const start=success(await control("start_service",{id:service.id},operationId));
    assert.deepEqual(success(await control("start_service",{id:service.id},operationId)),start);
    const [pid,port]=await until(async()=>{const value=wsl(["/usr/bin/python3","-c",`import os; p='${root}/ready'; print(open(p).read() if os.path.exists(p) else '')`]);return value&&value.split(' ').map(Number);},"WSL child did not listen");
    const run=success(await runtime("get_active_run",{id:service.id}));assert.ok(run);
    let listenerCorrelation;
    if(wslVersion===1) {
      const snapshot=success(await call("workspace.processes","list_port_observations",{},29000));
      assert.ok(snapshot.unavailable_wsl.includes(distro),"Unsupported WSL1 query must be visible");
      listenerCorrelation={state:"unsupported",reason:"wsl1-listener-observation-unavailable"};
    } else {
    const selection=await until(async()=>{
      const rows=success(await call("workspace.processes","list_port_observations",{},29000)).rows;
      const row=rows.find(row=>row.source==="wsl"&&row.wsl_distro===distro&&row.pid===pid&&row.port===port);
      const correlation=row?.correlations.find(item=>item.target_id===service.id);
      return correlation&&{row,correlation};
    },"WSL listener did not correlate with its Runtime group");
    assert.equal(selection.correlation.confidence,"declared");
    const endpoint={proto:selection.row.proto,local_addr:selection.row.local_addr,port:selection.row.port,state:selection.row.state};
    assert.deepEqual(success(await call("workspace.process-actions","kill_listener",{request:{endpoint,identity:selection.row.identity}},29000)),{kind:"ownedTask",taskId:service.id});
    const stale={...selection.row.identity,start_tick:selection.row.identity.start_tick+1};
    assert.equal((await call("workspace.process-actions","kill_listener",{request:{endpoint,identity:stale}},29000)).operation.outcome.state,"failed");
      listenerCorrelation={state:"passed",groupDescendantOwnership:true,startTickMismatch:true,declaredConfidencePreserved:true};
    }
    const resolved=success(await call("workspace.logs","reconnect_runtime_sources",{sources:[{kind:"runtimeRun",runId:run.id,stream:"stdout",revision:"1".repeat(64)}],filter:{text:"",regex:false}},29000));
    const logs=success(await call("workspace.logs","read_sources",{sources:resolved.sources,cursors:[null],sequenceStarts:[0],generation:1,operationId:randomUUID()},29000));
    const serialized=JSON.stringify(logs);assert.ok(serialized.includes("synthetic-wsl-listener"));assert.ok(!serialized.includes("synthetic-wsl-private"),"Runtime leaked the execution secret into logs");
    success(await control("stop_service",{id:service.id}));stopped=true;
    await until(async()=>wsl(["/usr/bin/python3","-c",`import os; print('gone' if not os.path.exists('/proc/${pid}') else open('/proc/${pid}/stat').read().split(') ',1)[1].split()[0])`])==="gone","WSL group stop left a descendant");
    const stoppedInstance=success(await runtime("get_service_instance",{id:service.id}));assert.equal(stoppedInstance.state,"stopped");assert.equal(stoppedInstance.nextRetryAt,null);
    // A failing service reaches backoff; stop cancels the generation's retry.
    const failing=success(await runtime("create_service",{input:{...input,name:"WSL backoff fixture",command:"exit 7",environment:{action:"clear"},restartPolicy:"on-failure"}}));
    try {
      success(await control("start_service",{id:failing.id}));
      await until(async()=>{const instance=success(await runtime("get_service_instance",{id:failing.id}));return instance.state==="retry_waiting"&&instance;},"WSL service did not reach retry backoff");
      success(await control("stop_service",{id:failing.id}));
      const generation=success(await runtime("get_service_instance",{id:failing.id})).generation;
      await delay(1200);const stopped=success(await runtime("get_service_instance",{id:failing.id}));assert.equal(stopped.state,"stopped");assert.equal(stopped.generation,generation);assert.equal(stopped.nextRetryAt,null);
    } finally {await control("stop_service",{id:failing.id}).catch(()=>{});await runtime("delete_service",{id:failing.id}).catch(()=>{});}
    return {wslVersion,ownedDistroOnly:true,duplicateGenerationReceipt:true,listenerCorrelation,platformSecretRedaction:true,groupStop:true,backoffStop:true};
  } finally {
    if(!stopped)await control("stop_service",{id:service.id}).catch(()=>{});
    await runtime("delete_service",{id:service.id}).catch(()=>{});
    // No distro shutdown or other process cleanup is performed here.
    wsl(["/usr/bin/python3","-c",`import shutil; shutil.rmtree('${root}')`]);
  }
}
