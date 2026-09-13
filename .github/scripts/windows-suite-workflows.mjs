// Four actual product executables on one disposable hosted Windows VM. Reuses
// the completed product build; no local fixture, Docker or legacy executable.
import {requireHostedNetworkFixture} from "./fixture-network-safety.mjs";
import {freePort,connect,waitForRenderer} from "./workspace-cdp-fixture.mjs";
import {allWindowsProcesses,stopOwnedProcess,windowsProcessIsElevated,inspectElevatedCdpPolicy,installElevatedCdpPolicy,restoreElevatedCdpPolicy} from "./windows-packaged-smoke.mjs";
import assert from "node:assert/strict";
import {mkdtempSync,mkdirSync,copyFileSync,cpSync,writeFileSync,readFileSync,existsSync} from "node:fs";
import {tmpdir} from "node:os";
import path from "node:path";
import {spawn,spawnSync} from "node:child_process";
import {once} from "node:events";
import {createHash,randomUUID} from "node:crypto";
import {setTimeout as delay} from "node:timers/promises";
requireHostedNetworkFixture();assert.equal(process.platform,"win32");
const catalog=JSON.parse(readFileSync("apps/products.json","utf8"));
const root=mkdtempSync(path.join(tmpdir(),"devbox-suite-workflow-"));
const evidence={source:process.env.GITHUB_SHA,environment:"github-hosted-windows",stage:"assembly",checks:{},result:"failed"};
mkdirSync("product-foundation-evidence",{recursive:true});
const report=()=>writeFileSync("product-foundation-evidence/suite-workflows.json",JSON.stringify(evidence,null,2));
const stage=value=>{evidence.stage=value;report();};
const live=[];
const digest=file=>createHash("sha256").update(readFileSync(file)).digest("hex");
function chord(keys){
  assert.ok(["^%{SPACE}","^%n","^%p","^%t"].includes(keys));
  const result=spawnSync("powershell.exe",["-NoProfile","-NonInteractive","-Command",`Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait('${keys}')`],{encoding:"utf8",timeout:10000,windowsHide:true});
  assert.equal(result.status,0,"native shortcut input failed");
}
function assemble(directory,products=catalog.products){
  mkdirSync(directory,{recursive:true});
  const members=products.map(product=>{
    const member=`products/${product.id}/devbox-${product.id}.exe`,file=path.join(directory,member);
    mkdirSync(path.dirname(file),{recursive:true});copyFileSync(path.resolve(`target/debug/devbox-${product.id}.exe`),file);
    if(product.id==="workspace")cpSync("apps/devbox-workspace/src-tauri/resources/wsl",path.join(path.dirname(file),"resources/wsl"),{recursive:true});
    copyFileSync("THIRD_PARTY_NOTICES.md",path.join(path.dirname(file),"THIRD_PARTY_NOTICES.md"));
    return {product:product.id,executable:member,sha256:digest(file)};
  });
  const manifest={schemaVersion:1,installationId:randomUUID(),generation:randomUUID(),suiteVersion:"0.8.0",protocolVersion:1,members};
  writeFileSync(path.join(directory,"devbox-installation.json"),JSON.stringify(manifest));return manifest;
}
async function start(product,directory){
  const executable=path.join(directory,`products/${product}/devbox-${product}.exe`),port=await freePort();
  const policy=windowsProcessIsElevated()?inspectElevatedCdpPolicy(path.basename(executable),port):null;
  if(policy)installElevatedCdpPolicy(policy);
  const item={product,executable,port,policy,child:null,cdp:null};live.push(item);
  item.child=spawn(executable,[],{cwd:path.dirname(executable),env:{...process.env,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port}`},stdio:["ignore","ignore","pipe"]});
  item.child.stderr.setEncoding("utf8");item.child.stderr.on("data",value=>{item.nativeError=((item.nativeError??"")+value).slice(-2000);});
  await once(item.child,"spawn");item.cdp=await connect(port,item.child);await waitForRenderer(item.cdp,"!!window.__TAURI_INTERNALS__","native product bridge missing");
  return item;
}
function routeFor(component){return component==="workspace.terminal"?"terminal":component==="workspace.runtime"?"tasks":component==="workspace.logs"?"logs":component==="workspace.files"?"files":component.startsWith("workspace.")?"overview":component==="api-studio.transforms"?"transforms":component==="api-studio.webhooks"?"webhooks":component.startsWith("api-studio.")?"requests":(component.startsWith("knowledge.search")||component==="knowledge.opener")?"search":"notes";}
async function request(item,command,body,route){
  return item.cdp.evaluate(`(async()=>{const invoke=window.__TAURI_INTERNALS__.invoke;const d=await invoke('plugin:product-shell|describe');const header={protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+29000,route:${JSON.stringify(route??catalog.products.find(product=>product.id===item.product).defaultRoute)},context:d.context};return invoke(${JSON.stringify(command)},{request:{header,...${JSON.stringify(body)}}});})()`,{timeoutMs:35000});
}
const value=result=>{assert.equal(result.operation.outcome.state,"succeeded",JSON.stringify(result));return result.value;};
const domain=async(item,component,method,args={})=>value(await request(item,`plugin:${item.product}|execute`,{component,method,args},routeFor(component)));
const suite=async(item,method)=>value(await request(item,"plugin:suite|connection",{method}));
const commands=async(item,method,args)=>value(await request(item,`plugin:commands|${method}`,args));
async function approve(item){const review=await suite(item,{kind:"preview"});assert.equal(review.products.length,4);await suite(item,{kind:"approve",token:review.token,remember:true});assert.equal((await suite(item,{kind:"status"})).connected,true);}
async function click(item,selector,text){await waitForRenderer(item.cdp,`[...document.querySelectorAll(${JSON.stringify(selector)})].some(node=>node.textContent.trim()===${JSON.stringify(text)}&&!node.disabled)`,"fixture button missing");await item.cdp.evaluate(`[...document.querySelectorAll(${JSON.stringify(selector)})].find(node=>node.textContent.trim()===${JSON.stringify(text)}&&!node.disabled).click()`);}
async function review(item,receipt,accept=true){
  const pending=await suite(item,{kind:"pending"});const selected=pending.find(row=>row.operationId===receipt.operationId);assert.ok(selected,"native destination review missing");
  if(!accept){await suite(item,{kind:"decide",id:selected.operationId,revision:selected.revision,accept:false});return selected;}
  // The actual common Review sets the renderer hint and acknowledges navigation.
  await click(item,'section[aria-label="다른 제품의 열기 요청"] button',"화면 열기");
  return selected;
}
async function until(check,label){const deadline=Date.now()+15000;do{const result=await check();if(result)return result;await delay(150);}while(Date.now()<deadline);throw new Error(label);}
try {
  const installation=path.join(root,"suite");assemble(installation);
  stage("start-four-products");
  const apps={};for(const product of catalog.products)apps[product.id]=await start(product.id,installation);
  const workspace=apps.workspace,api=apps["api-studio"],knowledge=apps.knowledge,center=apps["control-center"];
  await domain(workspace,"workspace.migration","start_empty");
  await domain(knowledge,"knowledge.migration","start_empty");
  for(const item of Object.values(apps))await approve(item);
  evidence.checks.approvedExactFourProductInstallation=true;
  for(const product of ["workspace","api-studio","knowledge"]){const description=await suite(center,{kind:"probe",product});assert.ok(description);}
  stage("project-identity-and-command-review");
  const projects=[];
  for(const suffix of ["one","two"]){const directory=path.join(root,suffix);mkdirSync(directory);writeFileSync(path.join(directory,"selected.txt"),"가😀나\nsynthetic suite selection\n");const preview=await domain(workspace,"workspace.registry","preview_windows",{root:directory});const registered=await domain(workspace,"workspace.registry","apply_registration",{previewId:preview.previewId,name:"같은 이름",action:"register"});projects.push({directory,context:registered.context});}
  const source=await commands(center,"command_source",{product:"workspace",source:"projects",query:"같은 이름",generation:1,mode:"name"});
  const rows=source.result.items;assert.equal(rows.length,2);assert.notEqual(rows[0].id,rows[1].id);evidence.checks.sameNameDistinctProjects=true;
  const search=await commands(center,"command_search",{query:""});
  const capture=search.items.find(row=>row.id==="knowledge.quick-capture"||row.id==="knowledge.open-capture");
  const target=capture??search.items.find(row=>row.owner==="knowledge"&&row.target.kind==="route");assert.ok(target);
  const openRequest=()=>({operationId:randomUUID(),commandId:target.id,revision:target.revision,context:target.context,selectionId:null});
  const rejectedCommand=openRequest();const rejected=await commands(center,"command_open",{command:rejectedCommand});await review(knowledge,rejected,false);
  assert.equal((await commands(center,"command_status",{product:"knowledge",operationId:rejected.operationId})).phase,"rejected");
  const acceptedCommand=openRequest(),accepted=await commands(center,"command_open",{command:acceptedCommand});await review(knowledge,accepted);
  await until(async()=>(await commands(center,"command_status",{product:"knowledge",operationId:accepted.operationId})).phase==="opened","destination did not acknowledge open");
  assert.equal((await commands(center,"command_open",{command:acceptedCommand})).operationId,accepted.operationId);
  await assert.rejects(()=>commands(center,"command_open",{command:{...openRequest(),revision:"f".repeat(64)}}));
  evidence.checks.routeReviewRejectAcceptReplayAndStale=true;
  stage("cold-activation-and-namespace");
  knowledge.cdp.close();const oldKnowledgeExit=once(knowledge.child,"exit");knowledge.child.kill();await oldKnowledgeExit;
  const coldRequest=openRequest();const coldReceipt=await commands(center,"command_open",{command:coldRequest});
  const identity=allWindowsProcesses().find(item=>path.resolve(item.Path).toLowerCase()===path.resolve(knowledge.executable).toLowerCase());assert.ok(identity,"cold activation did not launch the verified member");
  knowledge.identity=identity;knowledge.child={pid:identity.Pid,get exitCode(){return allWindowsProcesses().some(item=>item.Pid===identity.Pid&&item.Created===identity.Created&&item.Path===identity.Path)?null:0;}};
  knowledge.cdp=await connect(knowledge.port,knowledge.child);await review(knowledge,coldReceipt);
  await until(async()=>(await commands(center,"command_status",{product:"knowledge",operationId:coldReceipt.operationId})).phase==="opened","cold route was not acknowledged");
  evidence.checks.coldActivationUsesExactRememberedMember=true;
  stage("editor-to-transform");
  await domain(workspace,"workspace.registry","select_project",{context:projects[0].context});
  const file=await domain(workspace,"workspace.files","open_file",{request:{path:path.join(projects[0].directory,"selected.txt"),encoding:null}});
  await domain(workspace,"workspace.files","sync_editor_document",{path:file.path,nativeRevision:file.nativeRevision,text:file.text});
  const sent=await domain(workspace,"workspace.files","send_editor_selection",{path:file.path,nativeRevision:file.nativeRevision,text:file.text,from:1,to:3});
  const selectionReview=await review(api,sent.receipt);
  await waitForRenderer(api.cdp,"[...document.querySelectorAll('[role=dialog],dialog')].some(node=>node.textContent.includes('😀'))","Transform selection preview missing");
  evidence.checks.editorUnicodeSelectionReachedNativePreview=true;
  // Cancel the actual preview; original document bytes and buffer stay intact.
  await click(api,'[role=dialog] button,dialog button',"취소");
  assert.equal(readFileSync(path.join(projects[0].directory,"selected.txt"),"utf8"),file.text);
  await domain(workspace,"workspace.files","sync_editor_document",{path:file.path,nativeRevision:file.nativeRevision,text:"changed buffer"});
  await assert.rejects(()=>domain(api,"api-studio.transforms","open_workspace_selection",{id:sent.handoffId,operationId:sent.receipt.operationId,revision:selectionReview.commandRevision}));
  evidence.checks.cancelPreservesSourceAndStaleSelectionDenied=true;
  stage("indexed-file-to-editor");
  await domain(knowledge,"knowledge.search-settings","add_root",{path:projects[0].directory,indexContent:true});
  await until(async()=>!(await domain(knowledge,"knowledge.search","index_status")).indexing,"synthetic index did not settle");
  const query=await domain(knowledge,"knowledge.search","source_query",{source:"files",query:"selected.txt",mode:"name",limit:10});
  const found=await until(async()=>{const current=await domain(knowledge,"knowledge.search","source_poll",{generation:query.generation});return current.rows.find(row=>row.reference&&row.availability==="available");},"native indexed file reference missing");
  const fileReceipt=await domain(knowledge,"knowledge.opener","open_in",{appId:"devbox-workspace",reference:found.reference});
  await review(workspace,fileReceipt);
  await waitForRenderer(workspace.cdp,"document.querySelector('.doc-host')?.textContent?.includes('synthetic suite selection')","Indexed file did not reach Editor");
  const afterFile=(await workspace.cdp.evaluate("window.__TAURI_INTERNALS__.invoke('plugin:product-shell|describe')")).context;
  assert.deepEqual(afterFile,projects[0].context);evidence.checks.indexedFileRevalidatedWithoutChangingProject=true;
  await workspace.cdp.evaluate("[...document.querySelectorAll('.cm-content')].find(node=>node.getBoundingClientRect().width>0).focus()");
  await workspace.cdp.command("Input.insertText",{text:"dirty retained "});
  const dirtyText=await workspace.cdp.evaluate("[...document.querySelectorAll('.cm-content')].find(node=>node.getBoundingClientRect().width>0).textContent");
  const sameFile=await domain(knowledge,"knowledge.opener","open_in",{appId:"devbox-workspace",reference:found.reference});await review(workspace,sameFile);
  await delay(300);assert.equal(await workspace.cdp.evaluate("[...document.querySelectorAll('.cm-content')].find(node=>node.getBoundingClientRect().width>0).textContent"),dirtyText);
  evidence.checks.receivedFilePreservesDirtyEditor=true;

  stage("native-log-selection");
  const logFile=path.join(projects[0].directory,"selected.log");writeFileSync(logFile,"synthetic log selection\n");
  const logs=await domain(workspace,"workspace.logs","read_sources",{sources:[{kind:"localFile",path:logFile}],cursors:[null],sequenceStarts:[0],generation:700,operationId:randomUUID()});assert.ok(logs.records.length);
  const logSent=await domain(workspace,"workspace.logs","send_selection_to_toolbox",{generation:700,keys:logs.records.map(row=>({sourceId:row.sourceId,sequence:row.sequence}))});
  const logPending=(await suite(api,{kind:"pending"})).find(row=>row.target.kind==="entity"&&row.target.id===logSent.handoffId);assert.ok(logPending);await review(api,{operationId:logPending.operationId});
  await waitForRenderer(api.cdp,"[...document.querySelectorAll('[role=dialog]')].some(node=>node.textContent.includes('synthetic log selection'))","Log Transform preview missing");await click(api,'[role=dialog] button',"취소");
  evidence.checks.nativeLogSelectionPreview=true;
  stage("saved-result-to-knowledge");
  const saved=await domain(api,"api-studio.api","save_knowledge_draft",{output:"synthetic suite knowledge result"});
  const delivery=await domain(api,"api-studio.api","send_knowledge_draft",{id:saved.draft.artifact.id});
  await review(knowledge,delivery);
  await waitForRenderer(knowledge.cdp,"[...document.querySelectorAll('[role=dialog],dialog')].some(node=>node.textContent.includes('synthetic suite knowledge result'))","Knowledge result preview missing");
  await click(knowledge,'[role=dialog] button,dialog button',"취소");
  assert.equal((await domain(api,"api-studio.api","get_knowledge_draft",{id:saved.draft.artifact.id})).body,"synthetic suite knowledge result");
  evidence.checks.savedResultPreviewCancelPreservesProducer=true;
  stage("session-summary-to-daily");
  const prepared=await domain(workspace,"workspace.terminal","prepare_development_session",{operationId:randomUUID(),jobs:[],terminalProfile:null});assert.ok(prepared.session);
  await domain(workspace,"workspace.terminal","start_development_session",{id:prepared.session.id,revision:prepared.session.revision,planRevision:prepared.session.planRevision,mode:"restoreOnly"});
  await until(async()=>(await domain(workspace,"workspace.terminal","development_sessions")).sessions.some(row=>row.id===prepared.session.id&&row.phase==="active"),"state-only session did not become active");
  await domain(workspace,"workspace.terminal","stop_development_session",{id:prepared.session.id});
  const stopped=await until(async()=>(await domain(workspace,"workspace.terminal","development_sessions")).sessions.find(row=>row.id===prepared.session.id&&row.phase==="stopped"),"state-only session did not stop");
  const summaryId=randomUUID();await domain(workspace,"workspace.terminal","prepare_session_summary",{operationId:summaryId,sessionId:stopped.id,revision:stopped.revision,includeProblems:false});
  const summaryReceipt=await suite(workspace,{kind:"sendSessionSummary",sourceId:summaryId,operationId:randomUUID()});await review(knowledge,summaryReceipt);
  await waitForRenderer(knowledge.cdp,"!!document.querySelector('section[aria-label=\"전달받은 세션 요약\"] pre')","Daily summary preview missing");
  await click(knowledge,'section[aria-label="전달받은 세션 요약"] button',"초안 저장 화면 열기");await waitForRenderer(knowledge.cdp,"!!document.querySelector('#knowledge-draft-title')","Session Notes preview missing");await click(knowledge,'[role=dialog] button',"취소");
  evidence.checks.sessionSummaryDailyAndNotesPreview=true;
  stage("owner-operations-and-shortcuts");
  for(const product of ["workspace","api-studio","knowledge"]){const operations=await suite(center,{kind:"readOperations",product});assert.ok(Array.isArray(operations));assert.ok(operations.length<=128);assert.ok(!JSON.stringify(operations).includes(root));}
  const config={enabled:true,accelerator:"Ctrl+AltSpace",terminal:true,capture:true,project:true};
  // Exact config grammar stays in the native owner; no Ctrl+C binding is allowed.
  await assert.rejects(()=>suite(center,{kind:"configureShortcuts",config}));
  config.accelerator="Ctrl+Alt+Space";const shortcuts=await suite(center,{kind:"configureShortcuts",config});assert.equal(shortcuts.registration,"registered");
  await assert.rejects(()=>suite(center,{kind:"configureShortcuts",config:{...config,accelerator:"Ctrl+C"}}));
  evidence.checks.nativeShortcutRegistrationAndForbiddenBinding=true;
  chord("^%{SPACE}");await waitForRenderer(center.cdp,"!!document.querySelector('#launcher-search')","native Launcher shortcut did not reach its owner");
  await center.cdp.evaluate("(()=>{const input=document.querySelector('#launcher-search');input.focus();input.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true,data:'한'}));})()");
  const beforeCapture=(await suite(knowledge,{kind:"pending"})).length;chord("^%n");await delay(300);assert.equal((await suite(knowledge,{kind:"pending"})).length,beforeCapture);
  await center.cdp.evaluate("document.querySelector('#launcher-search').dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:'한'}))");
  await center.cdp.command("Input.insertText",{text:"한글"});assert.equal(await center.cdp.evaluate("document.querySelector('#launcher-search').value"),"한글");
  await center.cdp.command("Input.dispatchKeyEvent",{type:"keyDown",key:"Escape",code:"Escape",windowsVirtualKeyCode:27});await center.cdp.command("Input.dispatchKeyEvent",{type:"keyUp",key:"Escape",code:"Escape",windowsVirtualKeyCode:27});
  evidence.checks.nativeHotkeyAndWebViewCompositionModalGuard=true;
  evidence.inputCoverage="OS global hotkey; WebView composition events and Unicode text via CDP; OS Korean IME layout and multi-monitor DPI are not asserted";

  const foreignDirectory=path.join(root,"other-installation");assemble(foreignDirectory);const foreign=await start("control-center",foreignDirectory);await approve(foreign);
  await assert.rejects(()=>suite(foreign,{kind:"configureShortcuts",config}));
  await assert.rejects(()=>commands(foreign,"command_source",{product:"workspace",source:"projects",query:"같은 이름",generation:1,mode:"name"}));
  evidence.checks.foreignInstallationCannotUseExistingOwnerOrProjectProvider=true;

  stage("common-surface-accessibility");
  for(const item of Object.values(apps)){
    await item.cdp.command("Emulation.setEmulatedMedia",{features:[{name:"forced-colors",value:"active"}]});
    await item.cdp.command("Emulation.setDeviceMetricsOverride",{width:1280,height:800,deviceScaleFactor:1.5,mobile:false});
    await click(item,".shell-toolbar button","작업 상태");
    await waitForRenderer(item.cdp,"!!document.querySelector('section[aria-label=\"작업 상태\"]')","Operations surface missing");
    assert.equal(await item.cdp.evaluate("document.querySelectorAll('button').length>0&&!!document.querySelector('a.shell-skip[href=\"#product-content\"]')"),true);
    await item.cdp.command("Emulation.setEmulatedMedia",{features:[]});
    await item.cdp.command("Emulation.clearDeviceMetricsOverride");
  }
  evidence.checks.fourNativeSurfacesWithRendererContrastAndDpiEmulation=true;
  await suite(center,{kind:"configureShortcuts",config:{...config,enabled:false}});
  evidence.result="passed";
} catch(error){evidence.failure=String(error).slice(0,3000);process.exitCode=1;}
finally {
  evidence.cleanup=[];
  for(const item of live.reverse()){
    item.cdp?.close();if(item.child?.exitCode===null){if(item.identity)await stopOwnedProcess(item.identity,item.executable,item.child);else{const exited=once(item.child,"exit");item.child.kill();await Promise.race([exited,delay(10000)]);}}
    if(item.policy)restoreElevatedCdpPolicy(item.policy);
    const exited=!item.child||item.child.exitCode!==null;evidence.cleanup.push({product:item.product,exited,nativeError:item.nativeError});
    if(!exited){process.exitCode=1;evidence.result="failed";}
  }
  report();
}
