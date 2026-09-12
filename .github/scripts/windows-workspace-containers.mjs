// Real container actions against the daemon in this runner's disposable WSL2 distro.
import assert from "node:assert/strict";
import {randomUUID} from "node:crypto";
import {setTimeout as delay} from "node:timers/promises";
export async function exerciseOwnedContainers({cdp,call,success,wsl,distro}){
  const nonce=randomUUID(),root="/tmp/devbox-container-"+nonce,name="devbox-fixture-"+nonce;
  const terminal=(method,args={})=>call("workspace.terminal",method,args);
  const docker=args=>wsl(["/usr/bin/docker",...args]);
  const until=async(check,message)=>{const end=performance.now()+45000;do{const value=await check();if(value)return value;await delay(200);}while(performance.now()<end);assert.fail(message);};
  let cid,replacement,primary;
  wsl(["/usr/bin/python3","-c","import pathlib,tarfile,io,sys;p=pathlib.Path(sys.argv[1]);p.mkdir();(p/'owner').write_text(sys.argv[2]);t=tarfile.open(p/'rootfs.tar','w');t.add('/bin/busybox',arcname='bin/busybox',recursive=False);b=b'synthetic-container-response';i=tarfile.TarInfo('www/index.html');i.size=len(b);t.addfile(i,io.BytesIO(b));t.close();(p/'fixture.log').write_text('synthetic-container-log\\n')",root,nonce]);
  try{
    const image=docker(["import",root+"/rootfs.tar",name]);assert.match(image,/^sha256:[a-f0-9]{64}$/);
    cid=docker(["create","--name",name,"-p","127.0.0.1::8080",name,"/bin/busybox","httpd","-f","-p","8080","-h","/www"]);assert.match(cid,/^[a-f0-9]{64}$/);
    const action=async(kind,id=cid,operationId=randomUUID())=>{
      const args={operationId,distro,containerId:id,action:kind};
      success(await terminal("docker_action",args));return args;
    };
    const started=await action("start");
    success(await terminal("docker_action",started));
    assert.equal(success(await terminal("wsl_control_status",{operationId:started.operationId})).state,"completed");
    const observed=await until(async()=>{
      const snapshot=success(await terminal("dashboard_snapshot"));
      const row=snapshot.distros.find(value=>value.name===distro);
      return row?.dockerAvailability==="available"&&row.containers.find(value=>value.id===cid);
    },"Owned container was absent from the Workspace collector");
    assert.ok(observed.ports.includes("8080"));assert.ok(observed.name.includes(name));
    const before=docker(["inspect","--format","{{.State.StartedAt}}",cid]);
    const restarted=await action("restart");
    const after=docker(["inspect","--format","{{.State.StartedAt}}",cid]);assert.notEqual(after,before);
    success(await terminal("docker_action",restarted));
    assert.equal(docker(["inspect","--format","{{.State.StartedAt}}",cid]),after);
    await action("stop");assert.equal(docker(["inspect","--format","{{.State.Running}}",cid]),"false");
    docker(["rm",cid]);
    replacement=docker(["create","--name",name,name,"/bin/busybox","sleep","300"]);
    assert.notEqual(replacement,cid);
    const stale=await terminal("docker_action",{operationId:randomUUID(),distro,containerId:cid,action:"start"});
    assert.equal(stale.operation.outcome.state,"failed");
    assert.equal(docker(["inspect","--format","{{.State.Running}}",replacement]),"false");
    // Main's native log queue delivers this path once to the existing Logs adapter.
    await cdp.command("Page.reload");
    await until(async()=>{try{return await cdp.evaluate("!!document.querySelector('.workspace-registry')");}catch{return false;}},"Workspace did not reload for Logs");
    success(await terminal("open_wsl_file_in_log_lens",{distro,wslPath:root+"/fixture.log"}));
    await until(async()=>cdp.evaluate("(document.body.innerText||'').includes('synthetic-container-log')"),"WSL file did not reach the Logs surface");
    await until(async()=>success(await terminal("read_terminal_log"))===null,"Consumed WSL file request remained queued");
    return {realDaemon:true,exactContainerId:true,publishedPorts:true,start:true,restart:true,stop:true,duplicateReceipt:true,replacedNameRejected:true,wslFileToLogsUi:true};
  }catch(error){primary=error;throw error;}
  finally{
    const errors=[];
    for(const id of [replacement,cid].filter(Boolean))try{
      const current=docker(["ps","-aq","--no-trunc","--filter","id="+id]);
      if(current){assert.equal(current,id);docker(["rm","-f",id]);}
    }catch(error){errors.push(error);}
    try{docker(["image","rm",name]);}catch(error){errors.push(error);}
    if(!errors.length)try{wsl(["/usr/bin/python3","-c","import pathlib,shutil,sys;p=pathlib.Path(sys.argv[1]);assert (p/'owner').read_text()==sys.argv[2];shutil.rmtree(p)",root,nonce]);}catch(error){errors.push(error);}
    if(errors.length)throw new AggregateError(primary?[primary,...errors]:errors,"Owned container cleanup incomplete");
  }
}
