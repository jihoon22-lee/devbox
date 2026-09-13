// Exact-artifact acceptance in a disposable WSL2 distro owned by the paired runner.
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
async function freePort() {
  const server = createServer(); server.listen(0, "127.0.0.1"); await once(server, "listening");
  const port = server.address().port; await new Promise((resolve) => server.close(resolve)); return port;
}

async function connect(port, child, deadline = performance.now() + 30_000, terminalId = null) {
  while (performance.now() < deadline) {
    if (child.exitCode !== null) throw new Error("product exited before renderer opened");
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(500) });
      const pages = await response.json(); const page = pages.find((p) => {
        if (p.type !== "page" || !p.webSocketDebuggerUrl) return false;
        try { const url = new URL(p.url); return (url.hostname === "tauri.localhost" || (url.protocol === "tauri:" && url.hostname === "localhost")) && ["/", "/index.html"].includes(url.pathname) && (terminalId ? url.searchParams.get("surface")==="terminal" && url.searchParams.get("id")===terminalId : url.searchParams.get("surface")!=="terminal"); } catch { return false; }
      });
      if (page) {
        const socket = new WebSocket(page.webSocketDebuggerUrl); await once(socket, "open");
        let id = 0; const pending = new Map(); const diagnostics = [];
        socket.addEventListener("message", ({ data }) => {
          const response = JSON.parse(data); const entry = pending.get(response.id);
          if (["Runtime.exceptionThrown", "Log.entryAdded", "Page.javascriptDialogOpening", "Inspector.targetCrashed"].includes(response.method)) {
            diagnostics.push({ event: response.method, details: JSON.stringify(response.params).slice(0, 6000) });
            if (diagnostics.length > 30) diagnostics.shift();
            writeFileSync("product-foundation-evidence/renderer-events.json", JSON.stringify({ currentProbe, diagnostics }, null, 2));
          }
          if (entry) { pending.delete(response.id); clearTimeout(entry.timer); response.error ? entry.reject(new Error("CDP request failed")) : entry.resolve(response.result); }
        });
        const command = (method, params = {}) => {
          const next = ++id;
          return new Promise((resolve, reject) => {
            const timer = setTimeout(() => { pending.delete(next); reject(new Error("CDP setup timeout")); }, 10_000);
            pending.set(next, { resolve, reject, timer });
            socket.send(JSON.stringify({ id: next, method, params }));
          });
        };
        try { await command("Runtime.enable"); await command("Log.enable"); await command("Page.enable"); } catch (error) { socket.close(); throw error; }
        return {
          close: () => socket.close(),
          command,
          async evaluate(expression, {timeoutMs=10_000} = {}) {
            if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 660_000) throw new Error("Invalid fixture CDP deadline");
            new Function(expression); // Check fixture JavaScript before sending it.
            const next = ++id;
            const result = await new Promise((resolve, reject) => {
              const timer = setTimeout(() => { pending.delete(next); writeFileSync("product-foundation-evidence/renderer-timeout.json", JSON.stringify({ currentProbe, diagnostics, expression: expression.slice(0, 240) }, null, 2)); reject(new Error(`CDP request timeout at ${currentProbe?.stage}`)); }, timeoutMs);
              pending.set(next, { resolve, reject, timer });
              socket.send(JSON.stringify({ id: next, method: "Runtime.evaluate", params: { expression, awaitPromise: true, returnByValue: true } }));
            });
            if (result.exceptionDetails) throw new Error(`renderer probe failed: ${String(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text).slice(0, 2000)}`);
            return result.result.value;
          },
        };
      }
    } catch { /* bounded startup polling */ }
    await delay(250);
  }
  throw new Error("renderer startup deadline exceeded");
}

async function waitForRenderer(cdp, expression, label) {
  const deadline = performance.now() + 60_000;
  while (performance.now() < deadline) {
    if (await cdp.evaluate(expression)) return;
    await delay(100);
  }
  // This runner contains only synthetic fixtures. Retain bounded UI state on
  // failure so a delivery, route, IPC error and lazy-load failure are distinct.
  const snapshot = await cdp.evaluate("({ route: document.querySelector('nav[aria-label=\"제품 화면\"] [aria-current=page]')?.textContent, dialogs: document.querySelectorAll(\"[role=dialog]\").length, text: (document.body?.innerText ?? \"\").slice(0, 12000) })");
  writeFileSync(path.join("product-foundation-evidence", `renderer-failure-${Date.now()}.json`), JSON.stringify({ label, snapshot }, null, 2));
  throw new Error(`${label}: route=${snapshot.route}, dialogs=${snapshot.dialogs}`);
}



assert.equal(process.platform,'win32');
const [ownerFile,artifact,expectedSource,expectedRun]=process.argv.slice(2);
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
const executable=path.join(appDirectory,'devbox-workspace.exe');
const installationId=digest(Buffer.from('\\\\?\\'+realpathSync.native(executable)));
const dataRoot=path.join(process.env.LOCALAPPDATA,`com.devbox.v08.workspace.i${installationId}`);
assert.equal(existsSync(dataRoot),false);
writeFileSync(path.join(directory,'app-owner.json'),JSON.stringify({executable,installationId,dataRoot,preexisting:false}));
const wslExe=path.join(process.env.SystemRoot,'System32','wsl.exe');
const wsl=(args,input)=>{
 const result=spawnSync(wslExe,['--distribution-id',owner.distroId,'--user','root','--cd','/','--exec',...args],{input,encoding:'utf8',timeout:15000,maxBuffer:1024*1024,windowsHide:true});
 if(result.status!==0)throw new Error('Owned WSL command failed '+result.status+' '+String(result.stderr).slice(0,500));return result.stdout.trim();
};
let child,cdp,confirmed=false,appExited=false,ownedDataRemoved=false;
const evidence={source:expectedSource,environment:'owned-local-windows-wsl2',result:'diagnostic',observations:{}};
const success=result=>{if(result.operation.outcome.state!=='succeeded')throw new Error('Native operation failed: '+JSON.stringify(result.operation.outcome));return result.value;};
try{
 const port=await freePort();child=spawn(executable,['--route=overview'],{cwd:appDirectory,env:{...process.env,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port}`,WEBVIEW2_USER_DATA_FOLDER:path.join(appDirectory,'webview2')},stdio:['ignore','ignore','pipe']});
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
 }finally{writeFileSync(path.join(artifact,'local-wsl2-workspace-acceptance.json'),JSON.stringify({...evidence,appExited,ownedDataRemoved},null,2));}
}
console.log(JSON.stringify({result:'pass',appExited,ownedDataRemoved,acceptanceComplete:true}));
