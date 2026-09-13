import {requireHostedNetworkFixture} from "./fixture-network-safety.mjs";
// Exact-artifact acceptance only on a disposable GitHub-hosted Windows runner.
import {windowsProcessIsElevated,inspectElevatedCdpPolicy,installElevatedCdpPolicy,restoreElevatedCdpPolicy} from "./windows-packaged-smoke.mjs";
import {exerciseTerminalSessionFixture} from "./windows-workspace-terminal-sessions.mjs";
import {exerciseMultiplexerReconnect} from "./windows-workspace-multiplexer.mjs";
import {exerciseOwnedContainers} from "./windows-workspace-containers.mjs";
import assert from 'node:assert/strict';
import {mkdirSync,copyFileSync,readFileSync,writeFileSync,existsSync,lstatSync,realpathSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';import path from 'node:path';
import {spawn,spawnSync} from 'node:child_process';import {once} from 'node:events';
import {createServer} from 'node:net';import {createHash,randomUUID} from 'node:crypto';
import {exerciseRuntimeWslFixture} from './windows-workspace-runtime-wsl.mjs';
import {workspaceRequestExpression} from './windows-workspace-registration.mjs';
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));let currentProbe={stage:'local-runtime'};
import {freePort,connect,waitForRenderer} from "./workspace-cdp-fixture.mjs";
const networkFixture=requireHostedNetworkFixture();
assert.equal(process.platform,'win32');
const [ownerFile,artifact,expectedSource,expectedRun]=process.argv.slice(2);
assert.equal(expectedRun,networkFixture.runId);
const json=p=>JSON.parse(readFileSync(p,'utf8').replace(/^\uFEFF/,''));
const owner=json(ownerFile),directory=path.dirname(ownerFile);
assert.equal(owner.schema,1);assert.equal(owner.version,2);
assert.match(owner.name,/^DevboxWorkspaceFixture-[0-9a-f]{32}$/);
assert.equal(path.basename(directory),owner.name);assert.equal(readFileSync(path.join(directory,'owner.txt'),'utf8'),owner.name);
assert.equal(realpathSync.native(path.dirname(directory)).toLowerCase(),realpathSync.native(tmpdir()).toLowerCase());
const metadata=json(path.join(artifact,'fixture.json'));assert.equal(metadata.sourceSha,expectedSource);assert.equal(metadata.runId,expectedRun);
assert.equal(metadata.purpose,'owned-workspace-webview-acceptance');
const appDirectory=path.join(directory,'app');mkdirSync(appDirectory);mkdirSync(path.join(appDirectory,'resources/wsl'),{recursive:true});
const digest=bytes=>createHash('sha256').update(bytes).digest('hex');
for(const entry of metadata.files){
 assert.ok(['devbox-workspace.exe','THIRD_PARTY_NOTICES.md','resources/wsl/manifest.json','resources/wsl/devbox-workspace-wsl'].includes(entry.path));
 const source=path.join(artifact,entry.path);assert.ok(lstatSync(source).isFile());assert.equal(digest(readFileSync(source)),entry.sha256);copyFileSync(source,path.join(appDirectory,entry.path));
}
process.chdir(directory);mkdirSync('product-foundation-evidence');
const imageName='devbox-workspace-wsl2-'+randomUUID()+'.exe';
const executable=path.join(appDirectory,imageName);copyFileSync(path.join(appDirectory,'devbox-workspace.exe'),executable);
const installationId=digest(Buffer.from('\\\\?\\'+realpathSync.native(executable)));
const dataRoot=path.join(process.env.LOCALAPPDATA,`com.devbox.v08.workspace.i${installationId}`);
assert.equal(existsSync(dataRoot),false);
writeFileSync(path.join(directory,'app-owner.json'),JSON.stringify({executable,installationId,dataRoot,preexisting:false}));
const wslExe=path.join(process.env.SystemRoot,'System32','wsl.exe');
const wsl=(args,input)=>{
 const result=spawnSync(wslExe,['--distribution-id',owner.distroId,'--user','root','--cd','/','--exec',...args],{input,encoding:'utf8',timeout:15000,maxBuffer:1024*1024,windowsHide:true});
 if(result.status!==0)throw new Error('Owned WSL command failed '+result.status+' '+String(result.stderr).slice(0,500));return result.stdout.trim();
};
let child,cdp,policy,confirmed=false,appExited=false,ownedDataRemoved=false;
const evidence={source:expectedSource,environment:'github-hosted-windows-wsl2',result:'diagnostic',observations:{}};
const success=result=>{if(result.operation.outcome.state!=='succeeded')throw new Error('Native operation failed: '+JSON.stringify(result.operation.outcome));return result.value;};
try{
 const port=await freePort();policy=windowsProcessIsElevated()?inspectElevatedCdpPolicy(imageName,port):null;if(policy)installElevatedCdpPolicy(policy);
 child=spawn(executable,['--route=overview'],{cwd:appDirectory,env:{...process.env,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port}`,WEBVIEW2_USER_DATA_FOLDER:path.join(appDirectory,'webview2')},stdio:['ignore','ignore','pipe']});
 child.stderr.setEncoding('utf8');child.stderr.on('data',value=>{evidence.nativeError=((evidence.nativeError??'')+value).slice(-16000);});
 await once(child,'spawn');cdp=await connect(port,child);
 const deadline=performance.now()+30000;let ready=false;
 while(performance.now()<deadline){try{ready=await cdp.evaluate('Array.from(document.querySelectorAll(".workspace-registry button")).some(b=>b.textContent.trim()==="빈 Workspace 시작"&&!b.disabled)');}catch{cdp.close();cdp=await connect(port,child,deadline);}if(ready)break;await delay(100);}
 assert.ok(ready);const description=await cdp.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe")');assert.equal(description.handshake.installationId,installationId);confirmed=true;
 const call=(component,method,args={})=>cdp.evaluate(workspaceRequestExpression(component,method,args,29000),{timeoutMs:35000});
 success(await call('workspace.migration','start_empty'));
 evidence.observations.runtime=await exerciseRuntimeWslFixture({call,success,distro:owner.name,wsl,wslVersion:2});
 assert.equal(evidence.observations.runtime.listenerCorrelation.state,'passed');
 const connectTerminal=id=>connect(port,child,performance.now()+45000,id);
 evidence.observations.sessions=await exerciseTerminalSessionFixture({cdp,directory,call,success,connectTerminal,distro:owner.name,wsl});
 evidence.observations.multiplexers=[];
 for(const multiplexer of ["tmux","zellij"])evidence.observations.multiplexers.push(await exerciseMultiplexerReconnect({call,success,connectTerminal,wsl,distro:owner.name,multiplexer}));
 evidence.observations.containers=await exerciseOwnedContainers({cdp,call,success,wsl,distro:owner.name});
 const terminated=spawnSync(wslExe,["--terminate",owner.name],{encoding:"utf8",timeout:30000,windowsHide:true});assert.equal(terminated.status,0);
 const running=()=>{const value=spawnSync(wslExe,["--list","--running","--quiet"],{encoding:"utf16le",timeout:15000,windowsHide:true});assert.equal(value.status,0);return value.stdout.split(/\\r?\\n/).map(name=>name.trim()).includes(owner.name);};
 assert.equal(running(),false);
 const snapshot=success(await call("workspace.terminal","dashboard_snapshot"));
 const stopped=snapshot.distros.find(value=>value.name===owner.name);assert.ok(stopped);
 assert.equal(stopped.dockerAvailability,"notQueried");assert.equal(stopped.resource,null);assert.equal(running(),false);
 evidence.observations.stoppedQueryDoesNotStart=true;
 evidence.result='pass';
}catch(error){evidence.error=String(error).slice(0,2000);throw error;}
finally{
 try{cdp?.close();if(child&&child.exitCode===null){child.kill();await Promise.race([once(child,'exit'),delay(10000)]);}appExited=!!child&&(child.exitCode!==null||child.signalCode!==null);
  if(confirmed&&appExited){rmSync(dataRoot,{recursive:true,force:true});ownedDataRemoved=!existsSync(dataRoot);}
 }finally{if(policy)restoreElevatedCdpPolicy(policy);writeFileSync(path.join(artifact,'hosted-wsl2-workspace-acceptance.json'),JSON.stringify({...evidence,appExited,ownedDataRemoved},null,2));}
}
console.log(JSON.stringify({result:'pass',appExited,ownedDataRemoved,acceptanceComplete:true}));
