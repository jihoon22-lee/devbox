// Synthetic source stores only. The Python owner keeps committed data in WAL
// while the native importer acquires its SQLite online backup.
import assert from "node:assert/strict";
import {mkdirSync,writeFileSync,readFileSync,existsSync,rmSync,realpathSync} from "node:fs";
import {spawn} from "node:child_process";
import {once} from "node:events";
import {randomUUID,createHash} from "node:crypto";
import path from "node:path";
import {setTimeout as delay} from "node:timers/promises";
export async function exerciseWorkspaceRuntimeImport({cdp,directory,call,success}) {
  assert.equal(process.env.GITHUB_ACTIONS,"true");assert.equal(process.env.RUNNER_ENVIRONMENT,"github-hosted");
  const roots=[];const nonce=randomUUID();
  const own=identifier=>{const root=path.join(process.env.LOCALAPPDATA,identifier);mkdirSync(root);writeFileSync(path.join(root,".fixture-owner"),nonce,{flag:"wx"});roots.push({root,identity:realpathSync.native(root)});return root;};
  const source=own("com.devbox.runmanager"),port=own("com.devbox.portmanager"),logs=own("com.devbox.loglens");
  const jobId=randomUUID(),serviceId=randomUUID(),runId=randomUUID();
  const sql=readFileSync("apps/run-manager/src-tauri/src/storage.rs","utf8").match(/const MIGRATION_SQL: &str = r#"([\s\S]*?)"#;/)?.[1];assert.ok(sql);
  const schema=path.join(directory,"runtime-import-schema.sql");writeFileSync(schema,sql,{flag:"wx"});
  const bytes=Buffer.from("synthetic preserved run log\n");const logDirectory=path.join(source,"logs","runs",runId);mkdirSync(logDirectory,{recursive:true});writeFileSync(path.join(logDirectory,`stdout.g0.o0-${bytes.length}.log`),bytes,{flag:"wx"});
  const preferences={schema_version:1,refresh_interval_ms:2000,pinned_only:false,favorite_ports:[],favorite_processes:[]};
  writeFileSync(path.join(port,"port-manager-preferences-v1.json"),JSON.stringify(preferences),{flag:"wx"});
  const savedViews={schemaVersion:1,revision:4,views:[{name:"Imported Runtime history",sources:[{kind:"run",source_id:`run-manager:${runId}:stdout`}],filter:{text:"synthetic",regex:false}}]};
  writeFileSync(path.join(logs,"saved-views.json"),JSON.stringify(savedViews),{flag:"wx"});
  const script=`import sqlite3,sys
root,schema,job,service,run=sys.argv[1:]
c=sqlite3.connect(root+'/data.db')
c.execute('PRAGMA journal_mode=WAL')
c.execute('PRAGMA wal_autocheckpoint=0')
c.executescript(open(schema,encoding='utf-8').read())
c.execute("INSERT INTO meta(key,value) VALUES('schema_version','4')")
c.execute("INSERT INTO jobs(id,name,command,cron_expr,enabled,env_ciphertext,created_at,updated_at) VALUES(?, 'Imported guarded fixture', 'exit 0', '* * * * *',1,?,100,100)",(job,b'synthetic-unusable-ciphertext'))
c.execute("INSERT INTO jobs(id,kind,name,command,restart_policy,auto_start,created_at,updated_at) VALUES(?,'service','Imported inactive service','exit 0','always',1,100,100)",(service,))
c.execute("INSERT INTO runs(id,job_id,queue_sequence,status,owner_instance_id,attempt_token,target_pid,log_dir,created_at) VALUES(?,?,1,'running','legacy-owner','synthetic-attempt',4294967288,?,200)",(run,job,'logs/runs/'+run))
c.execute("INSERT INTO service_instances(job_id,generation,state,updated_at) VALUES(?,3,'retry_waiting',200)",(service,))
c.commit()
print('ready',flush=True)
sys.stdin.read()
c.close()
`;
  const writer=spawn("python",["-u","-c",script,source,schema,jobId,serviceId,runId],{stdio:["pipe","pipe","pipe"],windowsHide:true});
  let output="",errors="";writer.stdout.on("data",bytes=>{output+=bytes;});writer.stderr.on("data",bytes=>{errors+=bytes;});
  const completed=once(writer,"exit");
  const until=async(fn,message)=>{const deadline=performance.now()+30000;do{const value=await fn();if(value)return value;await delay(100);}while(performance.now()<deadline);assert.fail(message);};
  const runtime=(method,args={})=>call("workspace.runtime",method,args,29000);
  const migration=(method,args={})=>call("workspace.migration",method,args);
  const failed=result=>assert.equal(result.operation.outcome.state,"failed");
  const waitImport=async()=>until(async()=>{const job=success(await runtime("runtime_import_status"));assert.notEqual(job?.phase,"failed",JSON.stringify(job));return ["ready","complete"].includes(job?.phase)&&job;},"Runtime import did not complete");
  try {
    await until(async()=>{assert.equal(writer.exitCode,null,errors);return output.includes("ready");},"WAL fixture did not initialize");
    assert.ok(existsSync(path.join(source,"data.db-wal")));
    const sourceHash=()=>["data.db","data.db-wal"].map(name=>createHash("sha256").update(readFileSync(path.join(source,name))).digest("hex"));
    const before=sourceHash();
    const preparing=success(await runtime("runtime_import_prepare"));const preserved=await waitImport();assert.equal(preserved.id,preparing.id);assert.equal(preserved.summary.runs,1);assert.equal(preserved.summary.secretsRequiringReview,1);
    success(await runtime("runtime_import_apply",{id:preserved.id}));const imported=await waitImport();assert.equal(imported.phase,"complete");assert.equal(imported.alreadyImported,false);
    assert.deepEqual(sourceHash(),before,"Import changed legacy DB/WAL bytes");
    const job=success(await runtime("get_job",{id:jobId}));assert.equal(job.enabled,false);assert.equal(job.envConfigured,false);assert.equal(job.envReconnectRequired,true);
    const service=success(await runtime("get_service",{id:serviceId}));assert.equal(service.autoStart,false);
    assert.equal(success(await runtime("get_service_instance",{id:serviceId})).state,"stopped");
    assert.equal(success(await runtime("get_run",{id:runId})).status,"failed");
    failed(await runtime("set_job_enabled",{id:jobId,enabled:true}));
    failed(await runtime("runtime_control",{operationId:randomUUID(),method:"run_job_now",args:{id:jobId}}));
    const input={name:"Edited imported fixture",command:"exit 0",cwd:null,targetKind:"windows",targetDistro:null,environment:{action:"clear"},cronExpr:"0 0 1 1 *",enabled:false,overlapPolicy:"skip",catchUp:false};
    const reviewed=success(await runtime("update_job",{id:jobId,input}));assert.equal(reviewed.envReconnectRequired,false);
    success(await runtime("runtime_import_resume",{id:preserved.id}));await waitImport();success(await runtime("runtime_import_apply",{id:preserved.id}));assert.equal((await waitImport()).alreadyImported,true);
    assert.equal(success(await runtime("get_job",{id:jobId})).name,"Edited imported fixture");assert.deepEqual(sourceHash(),before);
    const importSettings=async(source,component)=>{
      const job=success(await migration("prepare_legacy_snapshot",{source}));
      await until(async()=>{const state=success(await migration("legacy_snapshot_job"));assert.notEqual(state.phase,"failed");return state.id===job.id&&state.phase==="ready";},"Settings snapshot did not finish");
      const preview=success(await call(component,"preview_legacy_runtime_settings",{jobId:job.id},29000));
      success(await call(component,"apply_legacy_runtime_settings",{token:preview.token,replace:true},29000));return job;
    };
    await importSettings("port-manager","workspace.processes");
    assert.equal(success(await call("workspace.processes","load_port_manager_preferences",{},29000)).refresh_interval_ms,2000);
    const saved=await importSettings("log-lens","workspace.logs");
    const document=success(await call("workspace.logs","list_saved_views",{},29000));
    const view=document.views.find(view=>view.name==="Imported Runtime history");assert.ok(view);assert.equal(view.sources[0].kind,"runtimeRun");assert.equal(view.sources[0].runId,runId);assert.notEqual(view.sources[0].revision,"0".repeat(64));
    const read=success(await call("workspace.logs","read_sources",{sources:view.sources,cursors:[null],sequenceStarts:[0],generation:1,operationId:randomUUID()},29000));
    assert.ok(JSON.stringify(read).includes("synthetic preserved run log"));
    success(await call("workspace.logs","delete_saved_view",{expectedRevision:document.revision,name:view.name},29000));
    const preview=success(await call("workspace.logs","preview_legacy_runtime_settings",{jobId:saved.id},29000));
    assert.equal(success(await call("workspace.logs","apply_legacy_runtime_settings",{token:preview.token,replace:true},29000)).alreadyImported,true);
    assert.equal(success(await call("workspace.logs","list_saved_views",{},29000)).views.length,0);
    assert.deepEqual(sourceHash(),before);
    return {walSnapshot:true,sourceBytesUnchanged:true,inactiveDefinitions:true,secretReconnectGate:true,explicitSecretClear:true,interruptedHistory:true,retainedRunLogMapping:true,replayPreservesEdits:true,preferencesImported:true,savedViewReceiptPreservesDeletion:true};
  } finally {
    writer.stdin.end();
    await Promise.race([completed,delay(3000).then(()=>{if(writer.exitCode===null)writer.kill();})]);
    await completed.catch(()=>{});
    for(const entry of roots){assert.equal(realpathSync.native(entry.root),entry.identity);assert.equal(readFileSync(path.join(entry.root,".fixture-owner"),"utf8"),nonce);rmSync(entry.root,{recursive:true});}
  }
}
