// Runs only inside the caller's disposable registered WSL filesystem.
import assert from "node:assert/strict";
import {randomUUID} from "node:crypto";
import {setTimeout as delay} from "node:timers/promises";
export async function exerciseNativeWslTasks({cdp,call,success,distro,wsl,root}){
  const before=(await cdp.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe")')).context;
  const runtime=(method,args={})=>call("workspace.runtime",method,args,29000);
  const registry=(method,args={})=>call("workspace.registry",method,args,29000);
  const control=(method,args)=>runtime("runtime_control",{operationId:randomUUID(),method,args});
  const until=async(check,message)=>{const deadline=performance.now()+60000;do{const value=await check();if(value)return value;await delay(200);}while(performance.now()<deadline);assert.fail(message);};
  const source={version:"2.0.0",tasks:[
    {label:"Native diagnostic fixture",type:"process",command:"/usr/bin/python3",args:["-c","import os,pathlib;pathlib.Path('cwd').write_text(os.getcwd());print('fixture.rs:1:1: error: synthetic task problem',flush=True);raise SystemExit(3)"],problemMatcher:{fileLocation:"relative",pattern:{regexp:"^(.+):([0-9]+):([0-9]+): (error): (.+)$",file:1,line:2,column:3,severity:4,message:5}}},
    {label:"Native stopping fixture",type:"process",command:"/usr/bin/python3",args:["-c","import os,time,pathlib;pathlib.Path('linger').write_text(str(os.getpid()));time.sleep(60)"]}
  ]};
  wsl(["/usr/bin/python3","-c","import pathlib,sys; p=pathlib.Path(sys.argv[1]); p.mkdir(); (p/'.vscode').mkdir(); (p/'.vscode/tasks.json').write_text(sys.stdin.read()); (p/'fixture.rs').write_text('// synthetic source\\n')",root],JSON.stringify(source));
  let context,primary;const jobs=[],operations=[];
  try{
    const distribution=success(await registry("list_wsl_distros")).find(value=>value.name===distro);assert.ok(distribution);
    const preview=success(await registry("preview_wsl",{distroId:distribution.id,root,startStopped:false}));
    context=success(await registry("apply_registration",{previewId:preview.previewId,name:"Native WSL task fixture",action:"register"})).context;
    success(await registry("select_project",{context}));
    const descriptor=success(await runtime("workspace_task_source"));
    assert.deepEqual(descriptor.source,{path:root,targetKind:"wsl",targetDistro:distro});
    const plan=success(await runtime("preview_workspace_task_import",{path:root,targetKind:"wsl",targetDistro:distro,operationId:randomUUID()}));
    assert.equal(plan.items.length,2);assert.ok(plan.items.every(item=>item.status==="ready"));
    const applied=success(await runtime("apply_workspace_task_import",{path:root,sourceRoot:plan.sourceRoot,projectIdentity:plan.projectIdentity,revision:plan.revision,targetKind:"wsl",targetDistro:distro,selected:plan.items.map(item=>item.id),operationId:randomUUID()}));
    jobs.push(...success(await runtime("list_workspace_tasks")).filter(job=>job.sourceId===applied.sourceId));
    assert.equal(jobs.length,2);assert.ok(jobs.every(job=>!job.trusted));
    success(await runtime("trust_workspace_task_source",{sourceId:applied.sourceId,revision:plan.revision}));
    const first=success(await control("run_workspace_task_operation",{id:jobs.find(job=>job.label==="Native diagnostic fixture").jobId,failFast:true}));operations.push(first.id);
    const completed=await until(async()=>{const value=success(await runtime("get_workspace_task_operation",{operationId:first.id}));return ["succeeded","failed","cancelled"].includes(value.status)&&value;},"Native WSL task did not finish");
    assert.equal(completed.status,"failed");
    assert.equal(wsl(["/usr/bin/python3","-c","import pathlib,sys; print((pathlib.Path(sys.argv[1])/'cwd').read_text())",root]),root);
    const runId=completed.runs[0].runId;assert.ok(runId);
    const diagnostics=success(await runtime("list_workspace_task_diagnostics",{runId}));
    assert.equal(diagnostics.items.length,1);assert.match(diagnostics.items[0].offset,/^[0-9]+$/);
    const problems=success(await call("workspace.problems","snapshot",{},29000));
    const problem=problems.problems.find(item=>item.source==="matcher"&&item.message==="synthetic task problem");assert.ok(problem);
    const target=success(await call("workspace.problems","resolve",{id:problem.id,revision:problem.revision,log:false},29000));
    assert.equal(target.target.relativePath,"fixture.rs");assert.equal(target.target.column,null);
    const log=success(await call("workspace.problems","resolve",{id:problem.id,revision:problem.revision,log:true},29000));
    assert.equal(log.target.request.source.runId,runId);assert.equal(log.target.request.offset,diagnostics.items[0].offset);

    // Exercise the actual Problems -> Files surface after a renderer reload.
    await cdp.command("Page.reload");
    await until(async()=>{try{return await cdp.evaluate("!!document.querySelector('nav')");}catch{return false;}},"Workspace renderer did not reload");
    await cdp.evaluate("(async()=>{const d=await window.__TAURI_INTERNALS__.invoke('plugin:product-shell|describe');const label=d.features.find(f=>f.route==='problems').label;Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()===label).click();})()");
    await until(async()=>cdp.evaluate("Array.from(document.querySelectorAll('.problem-list li')).some(row=>row.textContent.includes('synthetic task problem'))"),"Matcher problem did not reach the UI");
    await cdp.evaluate("Array.from(document.querySelectorAll('.problem-list li')).find(row=>row.textContent.includes('synthetic task problem')).querySelector('button').click()");
    await until(async()=>cdp.evaluate("Array.from(document.querySelectorAll('.cm-content')).some(editor=>editor.textContent.includes('synthetic source'))"),"Problem navigation did not open native WSL source");
    await cdp.evaluate("Array.from(document.querySelectorAll('button[aria-label]')).find(button=>button.getAttribute('aria-label').endsWith('fixture.rs 닫기')).click()");
    await until(async()=>cdp.evaluate("!Array.from(document.querySelectorAll('button[aria-label]')).some(button=>button.getAttribute('aria-label').endsWith('fixture.rs 닫기'))"),"Fixture editor did not close");

    const second=success(await control("run_workspace_task_operation",{id:jobs.find(job=>job.label==="Native stopping fixture").jobId,failFast:true}));operations.push(second.id);
    const pid=await until(async()=>{const value=wsl(["/usr/bin/python3","-c","import pathlib,sys; p=pathlib.Path(sys.argv[1])/'linger'; print(p.read_text() if p.exists() else '')",root]);return value&&Number(value);},"Native WSL task did not start");
    wsl(["/usr/bin/python3","-c","import pathlib,sys; (pathlib.Path(sys.argv[1])/'.vscode/tasks.json').write_text('changed synthetic source')",root]);
    // Stop uses the retained target even after the source loses execution trust.
    success(await control("stop_workspace_task_operation",{operationId:second.id}));
    await until(async()=>success(await runtime("get_workspace_task_operation",{operationId:second.id})).status==="cancelled","Native WSL stop did not settle");
    assert.equal(wsl(["/usr/bin/python3","-c","import pathlib,sys; print('gone' if not pathlib.Path('/proc/'+sys.argv[1]).exists() else 'present')",String(pid)]),"gone");
    assert.equal((await control("run_workspace_task_operation",{id:jobs[0].jobId,failFast:true})).operation.outcome.state,"failed");
    assert.ok(success(await runtime("list_workspace_tasks")).filter(job=>job.sourceId===applied.sourceId).every(job=>!job.trusted));
    return {linuxSource:true,explicitTrust:true,actualCwd:true,matcherAndLogOffset:true,problemToNativeFileUi:true,sourceChangeRejectsStart:true,sourceChangeDoesNotBlockOwnedStop:true};
  }catch(error){primary=error;throw error;}
  finally{
    const errors=[];
    const attempt=async(action)=>{try{await action();return true;}catch(error){errors.push(error);return false;}};
    let retired=true;
    for(const operationId of operations)retired=(await attempt(async()=>{
      success(await control("stop_workspace_task_operation",{operationId}));
      await until(async()=>["succeeded","failed","cancelled"].includes(success(await runtime("get_workspace_task_operation",{operationId})).status),"Task cleanup did not settle");
    }))&&retired;
    if(retired)for(const job of jobs)await attempt(async()=>success(await runtime("delete_job",{id:job.jobId})));
    await attempt(async()=>{if(before)success(await registry("select_project",{context:before}));else success(await registry("clear_project"));});
    if(retired&&context)await attempt(async()=>{const state=success(await registry("snapshot"));success(await registry("remove",{revision:state.revision,context}));});
    if(errors.length){const error=new AggregateError(primary?[primary,...errors]:errors,"Native WSL task fixture cleanup incomplete");error.preserveFixtures=!retired;throw error;}
  }
}
