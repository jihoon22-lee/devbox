// Native Windows catalog/archive/cache checks using disposable fixture data.
import assert from "node:assert/strict";
import {createServer} from "node:net";
import {createHash} from "node:crypto";
import {mkdirSync,readFileSync,writeFileSync,realpathSync,existsSync} from "node:fs";
import path from "node:path";
import {pathToFileURL} from "node:url";
import {setTimeout as delay} from "node:timers/promises";
import {nativeFileDialog,nativeFileSave} from "./windows-workspace-files.mjs";

export async function createWorkspaceLspProxy() {
  let holding=false, attempts=0;
  const sockets=new Set(), pending=new Set();
  const deny=socket=>{pending.delete(socket);socket.end("HTTP/1.1 502 Fixture Offline\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");};
  const server=createServer(socket=>{
    sockets.add(socket);socket.on("error",()=>{});
    socket.on("close",()=>{sockets.delete(socket);pending.delete(socket);});
    socket.once("data",()=>{attempts++;holding?pending.add(socket):deny(socket);});
  });
  await new Promise((resolve,reject)=>{server.once("error",reject);server.listen(0,"127.0.0.1",resolve);});
  return {
    url:`http://127.0.0.1:${server.address().port}`,
    attempts:()=>attempts,
    hold:()=>{holding=true;},
    release:()=>{holding=false;for(const socket of pending)deny(socket);},
    async waitForAttempt(previous) {
      const deadline=performance.now()+15_000;
      while(attempts===previous&&performance.now()<deadline)await delay(25);
      assert.ok(attempts>previous,"Native installer did not reach the owned offline proxy");
    },
    close:()=>{for(const socket of sockets)socket.destroy();server.close();},
  };
}

async function downloadArchive(artifact,destination) {
  const expected=artifact.size_bytes;
  assert.ok(Number.isSafeInteger(expected)&&expected>0&&expected<=64*1024*1024);
  const hosts=new Set(["github.com","release-assets.githubusercontent.com","registry.npmjs.org"]);
  let url=new URL(artifact.url);
  for(let redirects=0;redirects<=5;redirects++) {
    assert.ok(url.protocol==="https:"&&hosts.has(url.hostname)&&!url.username&&!url.password,"Unexpected reviewed artifact transport");
    const response=await fetch(url,{redirect:"manual",signal:AbortSignal.timeout(120_000)});
    if(response.status>=300&&response.status<400) {
      const location=response.headers.get("location");await response.body?.cancel();
      assert.ok(location,"Reviewed artifact redirect lacks a destination");url=new URL(location,url);continue;
    }
    assert.equal(response.status,200,"Reviewed artifact download failed");
    const chunks=[];let size=0;
    for await(const chunk of response.body){size+=chunk.length;assert.ok(size<=expected,"Reviewed artifact exceeds expected size");chunks.push(chunk);}
    const bytes=Buffer.concat(chunks);
    assert.equal(size,expected);assert.equal(createHash("sha256").update(bytes).digest("hex"),artifact.sha256);
    writeFileSync(destination,bytes,{flag:"wx"});return destination;
  }
  throw new Error("Reviewed artifact redirect limit exceeded");
}

export async function exerciseWorkspaceLspInstaller({cdp,root,directory,call,success,waitForRenderer,processId,executable,network}) {
  const lsp=(method,args={})=>call("workspace.lsp",method,args);
  const rejected=result=>assert.equal(result.operation.outcome.state,"failed");
  const catalog=success(await lsp("lsp_catalog"));
  const rust=catalog.find(item=>item.id==="rust-analyzer"), node=catalog.find(item=>item.id==="typescript-language-server");
  assert.ok(rust&&node);
  const key=item=>({manifestId:item.id,version:item.version,platform:item.platform});
  const state=async item=>{
    const deadline=performance.now()+15_000;
    let result;
    do {
      result=await lsp("lsp_installed");
      if(result.value?.issue!=="lsp_install_busy")break;
      await delay(100);
    }while(performance.now()<deadline);
    if(result.operation.outcome.state!=="succeeded") {
      const description=await cdp.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe")');
      const canonicalExe=realpathSync.native(executable);
      assert.equal(path.dirname(canonicalExe),realpathSync.native(directory));
      const namespace=createHash("sha256").update(path.toNamespacedPath(canonicalExe)).digest("hex");
      assert.equal(description.handshake.installationId,namespace);
      const privateRoot=path.join(process.env.LOCALAPPDATA,`com.devbox.v08.workspace.i${namespace}`);
      const pointer=JSON.parse(readFileSync(path.join(privateRoot,"active-stores.json"),"utf8"));
      assert.equal(pointer.schemaVersion,1);assert.match(pointer.id,/^[a-f0-9-]{36}$/);
      // Only this nonce-owned fixture's index is retained for schema diagnosis.
      // It contains synthetic catalog installation metadata, never user data.
      const index=path.join(privateRoot,"stores",pointer.id,"files","lsp","installed.json");
      if(existsSync(index))writeFileSync(path.join("product-foundation-evidence","workspace-lsp-owned-index.json"),readFileSync(index));
    }
    return success(result).find(entry=>entry.manifest_id===item.id&&entry.version===item.version);
  };
  assert.equal((await state(rust)).state,"not_installed");
  assert.equal((await state(node)).state,"not_installed");

  // Only this app process uses the local failing proxy. Node's fixture download
  // remains independent and verifies the native catalog's fixed digest.
  const beforeBlocked=network.attempts();network.hold();
  const installing=lsp("lsp_install",key(rust)).then(value=>({value}),error=>({error}));
  const editing=path.join(root,"lsp-install-edit.txt");writeFileSync(editing,"before",{flag:"wx"});
  try {
    await network.waitForAttempt(beforeBlocked);
    const document=success(await call("workspace.files","open_file",{request:{path:editing,encoding:null}}));
    success(await call("workspace.files","save_file",{request:nativeFileSave(document,"edited during blocked download")}));
    success(await call("workspace.files","unwatch_file",{path:document.path}));
    assert.equal(readFileSync(editing,"utf8"),"edited during blocked download");
  } finally {network.release();}
  const blocked=await installing;if(blocked.error)throw blocked.error;rejected(blocked.value);
  assert.equal((await state(rust)).state,"not_installed");

  const archives=path.join(directory,"reviewed-lsp-archives");mkdirSync(archives);
  const rustArchive=await downloadArchive(rust.artifact,path.join(archives,"rust-analyzer.zip"));
  rejected(await lsp("lsp_import_archive",{...key(rust),archivePaths:[rustArchive]}));
  const pick=async(action,selectedFiles=[])=>{
    const result=lsp("pick_lsp_archives");
    const [selected]=await Promise.all([result,nativeFileDialog({processId,executable,directory,action,selectedFiles})]);
    return success(selected);
  };
  assert.deepEqual(await pick("Cancel"),[]);
  const discarded=await pick("Open",[rustArchive]);assert.equal(discarded.length,1);
  assert.match(discarded[0],/^[0-9a-f-]{36}$/);assert.notEqual(discarded[0],rustArchive);
  success(await lsp("discard_lsp_archives",{archivePaths:discarded}));
  rejected(await lsp("lsp_import_archive",{...key(rust),archivePaths:discarded}));
  const selected=await pick("Open",[rustArchive]);
  success(await lsp("lsp_import_archive",{...key(rust),archivePaths:selected}));
  rejected(await lsp("lsp_import_archive",{...key(rust),archivePaths:selected}));
  const imported=await state(rust);assert.equal(imported.state,"installed");assert.equal(imported.installed.install_source,"local_archive");
  assert.deepEqual(success(await lsp("language_server_statuses")),[]);
  const unapprovedStart=await lsp("start_language_server",{languageId:"rust",operationId:"unapproved-install-proof"});
  rejected(unapprovedStart);assert.equal(unapprovedStart.value.issue,"lsp_settings_save_required");

  await cdp.evaluate(`(async()=>{const d=await window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe");const label=d.features.find(f=>f.route==="files").label;Array.from(document.querySelectorAll('nav[aria-label="제품 화면"] button')).find(b=>b.textContent.trim()===label).click();})()`);
  await waitForRenderer(cdp,'Array.from(document.querySelectorAll(".workspace-feature-files:not([hidden]) button")).some(b=>b.textContent.trim()==="언어 서버"&&!b.disabled)',"LSP settings button unavailable");
  await cdp.evaluate('Array.from(document.querySelectorAll(".workspace-feature-files button")).find(b=>b.textContent.trim()==="언어 서버").click()');
  const card=`Array.from(document.querySelectorAll(".lsp-installer-card")).find(card=>card.querySelector("strong")?.textContent==="rust-analyzer")`;
  const click=async(label,scope="document")=>{
    const predicate=`Array.from((${scope})?.querySelectorAll("button")??[]).find(b=>b.textContent.trim()===${JSON.stringify(label)}&&!b.disabled)`;
    await waitForRenderer(cdp,`(()=>{const button=${predicate};if(!button)return false;button.click();return true;})()`,"LSP installer action unavailable");
  };
  await click("제거",card);await click("제거 확인");
  await waitForRenderer(cdp,`!!(${card})?.querySelector(".lsp-state.not_installed")`,"Native LSP uninstall did not settle");
  assert.equal((await state(rust)).archive_cached,true);
  await click("설치",card);await click("취소",'document.querySelector(".lsp-confirmation")');
  assert.equal((await state(rust)).state,"not_installed");
  const beforeCache=network.attempts();
  await click("설치",card);await click("설치 확인");
  await waitForRenderer(cdp,`!!(${card})?.querySelector(".lsp-state.installed")`,"Offline native LSP install did not settle");
  const cached=await state(rust);assert.equal(cached.installed.install_source,"archive_cache");assert.equal(network.attempts(),beforeCache);
  await click("닫기",'document.querySelector(".lsp-panel")');

  const lockBytes=readFileSync("apps/code-pad/src-tauri/src/lsp/node-lock.json");
  assert.equal(createHash("sha256").update(lockBytes).digest("hex"),node.files.package_lock_sha256);
  const lock=JSON.parse(lockBytes), packages=lock.roots[node.id].map(root=>lock.packages.find(item=>item.name===root.name&&item.version===root.version&&item.path===root.path));
  assert.equal(packages.length,2);
  const nodeArchives=[];
  for(const [index,item] of packages.entries()) {
    assert.ok(item);assert.deepEqual(item.dependencies,{});assert.deepEqual(item.optional_dependencies,{});
    nodeArchives.push(await downloadArchive({...item,url:item.tarball},path.join(archives,`node-${index}.tgz`)));
  }
  const nodeChoices=await pick("Open",nodeArchives);assert.equal(nodeChoices.length,2);
  success(await lsp("lsp_import_archive",{...key(node),archivePaths:nodeChoices}));
  assert.equal((await state(node)).state,"installed");
  assert.deepEqual(success(await lsp("language_server_statuses")),[]);
  const config=success(await lsp("load_lsp_config"));assert.equal(config.config.enabled,false);
  const execution=await exerciseWorkspaceLspExecution({cdp,root,call,success,waitForRenderer});
  const recovery=await exerciseWorkspaceLspRecovery({cdp,root,directory,executable,call,success,waitForRenderer});
  return {...execution,...recovery,blockedDownloadLeavesFilesUsable:true,nativeCancelAndOpaqueChoices:true,arbitraryPathAndReplayRejected:true,verifiedNativeArchiveImported:true,confirmedUiUninstallAndOfflineCacheInstall:true,reviewedNodeClosureImportedWithNativeMultiPicker:true,installationNeverStartsLanguageServers:true,offlineProxyAttempts:network.attempts()};
}


// Tauri defines invoke/ipc as non-writable properties. Observe its actual fetch
// transport in the disposable renderer instead; retain no body, headers or URL.
export function installWorkspaceEditorTrace() {
  const original = window.fetch, rows = [];
  const wrapped = function(input, init) {
    let request;
    try {
      const url = new URL(typeof input === "string" ? input : input.url);
      if (url.hostname === "ipc.localhost" && decodeURIComponent(url.pathname) === "/plugin:workspace|execute" && typeof init?.body === "string") request = JSON.parse(init.body).request;
    } catch { /* Non-IPC fetches remain untouched. */ }
    const method = request?.method;
    const tracked = request?.component === "workspace.lsp" && (/^(open|change|reload|save|close)_lsp_document$/.test(method) || /^lsp_recovery_(list|preview|apply|cancel)$/.test(method))
      || request?.component === "workspace.files" && ["save_file", "sync_editor_document"].includes(method);
    if (!tracked) return original.call(this, input, init);
    const row = {method, phase:"pending", elapsedMs:0}, started = performance.now();
    rows.push(row); if (rows.length > 128) rows.shift();
    return original.call(this, input, init).then(response => {
      // Clone before Tauri consumes the body; return the original immediately.
      void response.clone().json().then(result => {
        row.phase = result?.operation?.outcome?.state ?? "unknown";
        const issue = result?.value?.issue;
        if (typeof issue === "string" && /^[a-z_]{1,80}$/.test(issue)) row.issue = issue;
      }).catch(() => {row.phase="invalid_response";}).finally(() => {row.elapsedMs=Math.round(performance.now()-started);});
      return response;
    }, error => {row.phase="rejected";row.elapsedMs=Math.round(performance.now()-started);throw error;});
  };
  window.fetch = wrapped;
  if (window.fetch !== wrapped) throw new Error("Native editor trace transport is unavailable");
  window.__workspaceLspTrace = {rows, restore:()=>{window.fetch=original;}};
}

async function exerciseWorkspaceLspExecution({cdp,root,call,success,waitForRenderer}) {
  const lsp=(method,args={})=>call("workspace.lsp",method,args);
  // Record only bounded method/outcome metadata from this disposable renderer.
  // Capture UI notifications too; fixture-only calls cannot diagnose a lost save.
  await cdp.evaluate(`(${installWorkspaceEditorTrace.toString()})()`);
  try {
  const owned=path.join(root,"lsp-owned-fixture");mkdirSync(owned);
  const script=path.join(owned,"server.mjs"), marker=path.join(owned,"child.pid"), file=path.join(owned,"lsp-owner-main.rs");
  writeFileSync(script,readFileSync(".github/fixtures/workspace-lsp-server.mjs"),{flag:"wx"});
  writeFileSync(file,"let value = 1;\r\n",{flag:"wx"});
  const original=success(await lsp("load_lsp_config"));
  const configured={...original.config,enabled:true,server_by_language:{},custom_servers:[{language_ids:["rust"],executable:script,args:[marker,"documents"],runtime:{kind:"node",executable:process.execPath,min_version:null},source:"synthetic Windows fixture",version:"1",license:"UNLICENSED"}]};
  success(await lsp("save_lsp_config",{config:configured,nativeRevision:original.nativeRevision,recoverInvalid:false}));
  const denied=await lsp("start_language_server",{languageId:"rust",operationId:"unapproved-server-proof"});
  assert.equal(denied.operation.outcome.state,"failed");assert.equal(denied.value.issue,"lsp_execution_approval_required");
  const click=async(label,scope="document")=>{
    const predicate=`Array.from((${scope})?.querySelectorAll("button")??[]).find(b=>b.textContent.trim()===${JSON.stringify(label)}&&!b.disabled)`;
    await waitForRenderer(cdp,`!!(${predicate})`,"Native LSP action unavailable");await cdp.evaluate(`(${predicate}).click()`);
  };
  await cdp.evaluate(`(()=>{const input=document.getElementById("path-input");Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set.call(input,${JSON.stringify(file)});input.dispatchEvent(new Event("input",{bubbles:true}));})()`);
  await click("파일 열기",'document.querySelector(".workspace-feature-files")');
  await waitForRenderer(cdp,'Array.from(document.querySelectorAll(".workspace-feature-files [role=tab]")).some(tab=>tab.textContent.includes("lsp-owner-main.rs"))',"LSP editor fixture did not open");
  await click("언어 서버",'document.querySelector(".workspace-feature-files")');
  await click("설정 저장",'document.querySelector(".lsp-panel")');
  await click("실행 설정 검토",'document.querySelector(".lsp-panel")');
  await waitForRenderer(cdp,'document.querySelector(".lsp-execution-review")?.textContent.includes("server.mjs")',"Native execution review omitted the script");
  assert.ok((await cdp.evaluate('document.querySelector(".lsp-execution-review").textContent')).includes("SystemRoot"));
  await click("검토 취소",'document.querySelector(".lsp-panel")');
  assert.deepEqual(success(await lsp("language_server_statuses")),[]);
  await click("실행 설정 검토",'document.querySelector(".lsp-panel")');
  await click("이 설정의 실행 승인",'document.querySelector(".lsp-panel")');
  await waitForRenderer(cdp,'document.querySelector(".lsp-panel").textContent.includes("검토한 설정의 실행을 승인했습니다")',"Native LSP approval did not settle");
  assert.deepEqual(success(await lsp("language_server_statuses")),[]);
  await click("시작",'document.querySelector(".lsp-panel")');
  await waitForRenderer(cdp,'document.querySelector(".lsp-panel").textContent.includes("준비됨")',"Native language server did not initialize");
  await click("닫기",'document.querySelector(".lsp-panel")');
  const opened=success(await call("workspace.files","open_file",{request:{path:file,encoding:null}}));
  const uri=pathToFileURL(opened.path).href;
  const hover=async()=>{
    const deadline=performance.now()+15_000;
    while(performance.now()<deadline){
      const result=await lsp("request_lsp_hover",{languageId:"rust",uri,position:{line:0,character:1}});
      if(result.operation.outcome.state==="succeeded")return success(result);
      assert.ok(["lsp_document_denied","file_snapshot_changed","lsp_busy","files_unavailable"].includes(result.value.issue),"Unexpected document synchronization failure");
      await delay(100);
    }
    throw new Error("Native editor document did not synchronize");
  };
  assert.equal((await hover()).stale,false);
  await waitForRenderer(cdp,'window.__workspaceLspTrace.rows.length>0',"Native editor trace did not observe IPC fetches");
  const previousChanges=await cdp.evaluate('window.__workspaceLspTrace.rows.filter(row=>row.method==="change_lsp_document"&&row.phase==="succeeded").length');
  await cdp.evaluate('document.querySelector(".workspace-feature-files .cm-content").focus()');
  await cdp.command("Input.insertText",{text:"// editor\n"});
  const dirty='Array.from(document.querySelectorAll(".workspace-feature-files [role=tab]")).some(tab=>tab.textContent.includes("lsp-owner-main.rs")&&tab.textContent.includes("●"))';
  await waitForRenderer(cdp,dirty,"LSP editor change did not remain dirty");
  await waitForRenderer(cdp,`window.__workspaceLspTrace.rows.filter(row=>row.method==="change_lsp_document"&&row.phase==="succeeded").length>${previousChanges}`,"Native editor change did not synchronize");
  // Keep an actual native LSP read active across the user's Save click. The
  // file request must wait for its permit, without resubmitting a failed save.
  const reading=lsp("request_lsp_hover",{languageId:"rust",uri,position:{line:0,character:2}}).then(result=>({result}),error=>({error}));
  let readingStarted=false;
  for(let attempt=0;attempt<100;attempt+=1){
    if(existsSync(marker+".hover")){readingStarted=true;break;}
    await delay(10);
  }
  assert.ok(readingStarted,"Native LSP read did not hold the fixture request");
  await click("저장",'document.querySelector(".workspace-feature-files")');
  const readResult=await reading;
  assert.ok(!readResult.error,"The concurrent native LSP read failed");
  assert.equal(success(readResult.result).stale,false);
  await waitForRenderer(cdp,`!(${dirty})`,"LSP editor save did not settle");
  await waitForRenderer(cdp,'window.__workspaceLspTrace.rows.filter(row=>row.method==="save_file").length===1&&window.__workspaceLspTrace.rows.some(row=>row.method==="save_file"&&row.phase==="succeeded")',"Native save did not complete exactly once");
  assert.ok(readFileSync(file,"utf8").includes("// editor"));
  await waitForRenderer(cdp,'window.__workspaceLspTrace.rows.some(row=>row.method==="save_lsp_document"&&row.phase==="succeeded")',"Native editor didSave did not complete");
  assert.equal((await hover()).stale,false);
  await click("언어 서버",'document.querySelector(".workspace-feature-files")');
  await click("실행 승인 해제",'document.querySelector(".lsp-panel")');
  await waitForRenderer(cdp,'document.querySelector(".lsp-panel").textContent.includes("실행 승인을 해제했습니다")',"Native LSP revocation did not settle");
  assert.deepEqual(success(await lsp("language_server_statuses")),[]);
  await click("닫기",'document.querySelector(".lsp-panel")');
  const current=success(await lsp("load_lsp_config"));
  success(await lsp("save_lsp_config",{config:original.config,nativeRevision:current.nativeRevision,recoverInvalid:false}));
  await cdp.evaluate('Array.from(document.querySelectorAll(".workspace-feature-files .document-tab")).find(tab=>tab.textContent.includes("lsp-owner-main.rs"))?.querySelector(".tab-action").click()');
  await waitForRenderer(cdp,'!Array.from(document.querySelectorAll(".workspace-feature-files [role=tab]")).some(tab=>tab.textContent.includes("lsp-owner-main.rs"))',"LSP editor fixture did not close");
  return {explicitNativeExecutionReview:true,approvalDoesNotAutoStart:true,actualNodeLspInitialize:true,codeMirrorDidOpenChangeSaveHover:true,saveWaitsForNativeLspReadWithoutReplay:true,revocationConfirmsNativeShutdown:true};
  } finally {
    const trace=await cdp.evaluate('(()=>{const trace=window.__workspaceLspTrace;trace.restore();delete window.__workspaceLspTrace;return trace.rows;})()');
    writeFileSync("product-foundation-evidence/workspace-lsp-editor-sync.json",JSON.stringify(trace,null,2));
  }
}

async function exerciseWorkspaceLspRecovery({cdp,root,directory,executable,call,success,waitForRenderer}) {
  await cdp.evaluate(`(${installWorkspaceEditorTrace.toString()})()`);
  try {
  const lsp=(method,args={})=>call("workspace.lsp",method,args);
  const rejected=result=>assert.equal(result.operation.outcome.state,"failed");
  const description=await cdp.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe")');
  // Only the nonce-named executable copied into this owned fixture can name
  // this disposable private namespace. Never enumerate existing installations.
  const canonicalExe=realpathSync.native(executable);
  assert.equal(path.dirname(canonicalExe),realpathSync.native(directory));
  const namespace=createHash("sha256").update(path.toNamespacedPath(canonicalExe)).digest("hex");
  assert.equal(description.handshake.installationId,namespace);
  assert.match(description.context.worktreeId,/^[a-f0-9-]{36}$/);
  const privateRoot=path.join(process.env.LOCALAPPDATA,`com.devbox.v08.workspace.i${namespace}`);
  const pointer=JSON.parse(readFileSync(path.join(privateRoot,"active-stores.json"),"utf8"));
  assert.equal(pointer.schemaVersion,1);assert.match(pointer.id,/^[a-f0-9-]{36}$/);
  assert.equal(success(await lsp("load_lsp_config")).config.enabled,false);
  assert.deepEqual(success(await lsp("language_server_statuses")),[]);
  success(await lsp("lsp_recovery_list"));
  const backups=path.join(privateRoot,"stores",pointer.id,"files","views",`worktree-${description.context.worktreeId}`,"lsp","runtime","rename-backups");
  assert.ok(existsSync(backups));
  const journalId="native-recovery-fixture", journalDir=path.join(backups,journalId);mkdirSync(journalDir);
  const target=path.join(root,"lsp-recovery-fixture.rs"), backup=path.join(journalDir,"backup-fixture.bak");
  const before="let before = 1;\r\n",after="let after = 1;\r\n",hash=text=>createHash("sha256").update(text).digest("hex");
  writeFileSync(target,after,{flag:"wx"});writeFileSync(backup,before,{flag:"wx"});
  const journal=path.join(journalDir,"journal.json");
  writeFileSync(journal,JSON.stringify({schema:1,planId:journalId,workspaceRoot:path.toNamespacedPath(realpathSync.native(root)),state:"rollbackfailed",entries:[{
    target:path.toNamespacedPath(realpathSync.native(target)),backup,beforeSize:Buffer.byteLength(before),beforeHash:hash(before),afterSize:Buffer.byteLength(after),afterHash:hash(after),
  }]}),{flag:"wx"});
  const opened=success(await call("workspace.files","open_file",{request:{path:target,encoding:null}}));
  rejected(await lsp("lsp_recovery_preview",{journalId}));
  success(await call("workspace.files","unwatch_file",{path:opened.path}));
  let preview=success(await lsp("lsp_recovery_preview",{journalId}));
  success(await lsp("lsp_recovery_cancel",{previewId:preview.previewId}));
  rejected(await lsp("lsp_recovery_apply",{previewId:preview.previewId}));
  assert.equal(readFileSync(target,"utf8"),after);
  preview=success(await lsp("lsp_recovery_preview",{journalId}));
  writeFileSync(target,"external fixture edit");
  rejected(await lsp("lsp_recovery_apply",{previewId:preview.previewId}));
  assert.equal(readFileSync(target,"utf8"),"external fixture edit");assert.equal(readFileSync(backup,"utf8"),before);
  writeFileSync(target,after);
  const click=async(label,scope="document")=>{
    const predicate=`Array.from((${scope})?.querySelectorAll("button")??[]).find(b=>b.textContent.trim()===${JSON.stringify(label)}&&!b.disabled)`;
    await waitForRenderer(cdp,`!!(${predicate})`,`Native recovery action unavailable: ${label}`);await cdp.evaluate(`(${predicate}).click()`);
  };
  await click("언어 서버",'document.querySelector(".workspace-feature-files")');
  await click("복구 기록 확인",'document.querySelector(".lsp-panel")');
  const records=success(await lsp("lsp_recovery_list")).records,index=records.findIndex(record=>record.journalId===journalId);
  assert.ok(index>=0);assert.equal(records[index].available,true);
  await click(`기록 ${index+1} 복구 검토`,'document.querySelector(".lsp-panel")');
  await waitForRenderer(cdp,`document.querySelector('section[aria-label="이름 변경 복구"]')?.textContent.includes("let before")`,"Native recovery preview omitted original bytes");
  await click("복구 검토 취소",'document.querySelector(".lsp-panel")');
  assert.equal(readFileSync(target,"utf8"),after);
  await click(`기록 ${index+1} 복구 검토`,'document.querySelector(".lsp-panel")');
  await click("검토한 원본 복원",'document.querySelector(".lsp-panel")');
  await waitForRenderer(cdp,`document.querySelector('section[aria-label="이름 변경 복구"]')?.textContent.includes("원본 복원이 완료되었습니다")`,"Native recovery did not complete");
  assert.equal(readFileSync(target,"utf8"),before);assert.equal(existsSync(journalDir),false);
  assert.deepEqual(success(await lsp("language_server_statuses")),[]);
  await click("닫기",'document.querySelector(".lsp-panel")');
  return {nativeJournalPreviewDoesNotWrite:true,recoveryRejectsOpenEditorAndChangedBytes:true,recoveryCancelAndReplayRejected:true,explicitUiRecoveryRestoresCrlfWithoutServerApproval:true};
  } finally {
    const trace=await cdp.evaluate('(()=>{const trace=window.__workspaceLspTrace;trace.restore();delete window.__workspaceLspTrace;return trace.rows;})()');
    writeFileSync("product-foundation-evidence/workspace-lsp-recovery-trace.json",JSON.stringify(trace,null,2));
  }
}
