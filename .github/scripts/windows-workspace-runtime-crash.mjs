// Abrupt native-owner exit is driven by the disposable product harness. These
// helpers create and inspect only this fixture's job, child, and operation ID.
import assert from "node:assert/strict";
import {writeFileSync,readFileSync,existsSync} from "node:fs";
import path from "node:path";
import {randomUUID} from "node:crypto";
import {setTimeout as delay} from "node:timers/promises";
import {workspaceRequestExpression} from "./windows-workspace-registration.mjs";
const call=(cdp,method,args={})=>cdp.evaluate(workspaceRequestExpression("workspace.runtime",method,args,29000),{timeoutMs:35000});
const success=result=>{assert.equal(result.operation.outcome.state,"succeeded",JSON.stringify(result));return result.value;};
export async function prepareRuntimeCrash(cdp,directory){
  const script=path.join(directory,"runtime-crash.cjs"),ready=path.join(directory,"runtime-crash-ready.json"),counter=path.join(directory,"runtime-crash-count.txt");
  writeFileSync(script,`const fs=require('node:fs'),http=require('node:http');fs.appendFileSync(${JSON.stringify(counter)},'start\\n');const server=http.createServer((_,r)=>r.end('fixture'));server.listen(0,'127.0.0.1',()=>fs.writeFileSync(${JSON.stringify(ready)},JSON.stringify({port:server.address().port,pid:process.pid})));`,{flag:"wx"});
  const job=success(await call(cdp,"create_job",{input:{name:"Crash-owned Runtime fixture",command:`"${process.execPath}" "${script}"`,cwd:directory,targetKind:"windows",targetDistro:null,cronExpr:"0 0 1 1 *",enabled:false,environment:{action:"keep"}}}));
  const operationId=randomUUID();const response=success(await call(cdp,"runtime_control",{operationId,method:"run_job_now",args:{id:job.id}}));
  const deadline=performance.now()+20000;while(!existsSync(ready)&&performance.now()<deadline)await delay(100);
  assert.ok(existsSync(ready));const child=JSON.parse(readFileSync(ready,"utf8"));assert.equal(readFileSync(counter,"utf8"),"start\n");
  return {jobId:job.id,operationId,response,child,counter};
}
export async function verifyRuntimeCrash(cdp,fixture){
  const deadline=performance.now()+20000;let running=true;
  while(running&&performance.now()<deadline){running=!!success(await call(cdp,"get_active_run",{id:fixture.jobId}));if(running)await delay(150);}
  assert.equal(running,false,"Restart adopted or failed to settle the prior process owner");
  await assert.rejects(fetch(`http://127.0.0.1:${fixture.child.port}`,{signal:AbortSignal.timeout(1000)}));
  const replay=success(await call(cdp,"runtime_control",{operationId:fixture.operationId,method:"run_job_now",args:{id:fixture.jobId}}));
  assert.deepEqual(replay,fixture.response);assert.equal(readFileSync(fixture.counter,"utf8"),"start\n");
  success(await call(cdp,"delete_job",{id:fixture.jobId}));
  return {nativeCrash:true,jobObjectDescendantsGone:true,restartSettlesOldOwnership:true,durableReceiptPreventsRelaunch:true};
}
