import {exerciseWorkspaceRuntimeWsl} from "./windows-workspace-runtime-wsl.mjs";
import {exerciseWorkspaceRuntimeImport} from "./windows-workspace-runtime-import.mjs";
import {exerciseWorkspaceRuntime} from "./windows-workspace-runtime.mjs";
// Actual hidden Workspace native admission/registration with owned fixtures.
import assert from "node:assert/strict";
import {mkdirSync, writeFileSync, readFileSync, realpathSync} from "node:fs";
import path from "node:path";
import {exerciseWorkspaceLspInstaller} from "./windows-workspace-lsp.mjs";
import {exerciseWorkspaceFiles} from "./windows-workspace-files.mjs";
import {exerciseWorkspaceWindowImport} from "./windows-workspace-window-import.mjs";
import {exerciseWorkspaceTemplateImport} from "./windows-workspace-template-import.mjs";
import {exerciseWorkspaceSessionImport} from "./windows-workspace-session-import.mjs";
import {exerciseWorkspaceDependencies} from "./windows-workspace-dependencies.mjs";
import {exerciseWorkspaceDefinitions} from "./windows-workspace-definitions.mjs";
import {exerciseWorkspaceSource} from "./windows-workspace-source.mjs";

export function workspaceRequestExpression(component, method, args = {}, budgetMs = ["workspace.dependencies","workspace.source","workspace.lsp"].includes(component) ? 29000 : 5000) {
  assert.ok(Number.isInteger(budgetMs) && budgetMs >= 100 && budgetMs <= 29000, "fixture deadline outside native bounds");
  const route = component === "workspace.runtime" ? "tasks" : component === "workspace.processes" || component === "workspace.process-actions" ? "runtime" : component === "workspace.logs" ? "logs" : component === "workspace.files" || component === "workspace.lsp" ? "files" : component === "workspace.dependencies" ? "dependencies" : component === "workspace.source" ? "source" : "overview";
  return `(async () => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const d = await invoke("plugin:product-shell|describe");
    const header = {protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+${budgetMs},route:${JSON.stringify(route)},...(d.context ? {context:d.context} : {})};
    for(let attempt=0;;attempt++) {
      try {return await invoke("plugin:workspace|execute",{request:{header,...${JSON.stringify({component, method, args})}}});}
      catch(problem) {
        // This exact envelope is emitted before authorization/admission. No
        // operation ran; background readers may still hold the context lease.
        // Never retry accepted failures, transport errors or other provenance.
        if(attempt<19 && Date.now()+50<header.deadlineMs && problem?.code==="unavailable"
          && problem.provenance?.product==="workspace" && problem.provenance?.component==="workspace.dispatch"
          && problem.provenance?.requestId==="rejected" && problem.provenance?.revision===1) {
          await new Promise(resolve=>setTimeout(resolve,50));
          header.requestId=crypto.randomUUID();
          continue;
        }
        throw new Error("Native Workspace request rejected: " + JSON.stringify(problem).slice(0,2000));
      }
    }
  })()`;
}

export async function exerciseWorkspaceRegistration({cdp, directory, waitForRenderer, suffix, processId, executable, network}) {
  const root = path.join(directory, "한글 project");
  mkdirSync(root);
  const marker = path.join(root, "preserved.txt");
  writeFileSync(marker, "synthetic project bytes\r\n", {flag:"wx"});
  const requests = [];
  const call = async (component, method, args = {}, budgetMs) => {
    const started = performance.now();
    const row = {component,method,state:"running"};
    requests.push(row); if(requests.length>16)requests.shift();
    const record = () => writeFileSync(`product-foundation-evidence/workspace-native-requests-${suffix}.json`,JSON.stringify(requests,null,2));
    record();
    try {
      const result = await cdp.evaluate(workspaceRequestExpression(component, method, args, budgetMs),{timeoutMs:method==="lsp_install"?660_000:35_000});
      row.state=result?.operation?.outcome?.state??"invalid";
      if(typeof result?.value?.issue==="string"&&/^[a-z_]{1,80}$/.test(result.value.issue))row.issue=result.value.issue;
      return result;
    } catch(error) {row.state="transport-failed";throw error;}
    finally {row.elapsedMs=Math.round(performance.now()-started);record();}
  };
  const success = result => {assert.equal(result.operation.outcome.state,"succeeded",JSON.stringify(result));return result.value;};
  const click = async label => {
    const predicate = `Array.from(document.querySelectorAll(".workspace-registry button")).some(button => button.textContent.trim() === ${JSON.stringify(label)} && !button.disabled)`;
    await waitForRenderer(cdp, predicate, "Workspace action did not become available");
    await cdp.evaluate(`Array.from(document.querySelectorAll(".workspace-registry button")).find(button => button.textContent.trim() === ${JSON.stringify(label)} && !button.disabled).click()`);
  };
  const fill = async (id, value) => cdp.evaluate(`(() => {const input=document.getElementById(${JSON.stringify(id)});Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set.call(input,${JSON.stringify(value)});input.dispatchEvent(new Event("input",{bubbles:true}));})()`);
  assert.equal(success(await call("workspace.migration","status")).phase,"setup");
  await click("빈 Workspace 시작");
  await waitForRenderer(cdp,'!!document.getElementById("workspace-project-path")',"Workspace registry did not activate");
  assert.equal(success(await call("workspace.registry","snapshot")).projects.length,0);
  const authority = await cdp.evaluate(`(async () => {
    const invoke=window.__TAURI_INTERNALS__.invoke;
    const d=await invoke("plugin:product-shell|describe");
    const header=()=>({protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+5000,route:"overview"});
    const request={header:header(),component:"workspace.registry",method:"snapshot",args:{}};
    await invoke("plugin:workspace|execute",{request});
    let replay=false,role=false,installation=false;
    try {await invoke("plugin:workspace|execute",{request});} catch {replay=true;}
    try {await invoke("plugin:workspace|execute",{request:{...request,header:header(),component:"workspace.migration",method:"apply_registration"}});} catch {role=true;}
    try {await invoke("plugin:workspace|execute",{request:{...request,header:{...header(),installationId:"foreign-installation"}}});} catch {installation=true;}
    return {replay,role,installation};
  })()`);
  assert.deepEqual(authority,{replay:true,role:true,installation:true});
  await fill("workspace-project-path",root);
  await click("폴더 확인");
  await waitForRenderer(cdp,'!!document.getElementById("workspace-project-name")',"Workspace preview did not render");
  assert.equal(success(await call("workspace.registry","snapshot")).projects.length,0);
  await click("취소");
  await waitForRenderer(cdp,'!document.getElementById("workspace-project-name")',"Workspace preview cancellation did not finish");
  assert.equal(success(await call("workspace.registry","snapshot")).projects.length,0);
  await click("폴더 확인");
  await waitForRenderer(cdp,'!!document.getElementById("workspace-project-name")',"Workspace second preview did not render");
  await fill("workspace-project-name","Native fixture project");
  await click("등록");
  await waitForRenderer(cdp,'!document.getElementById("workspace-project-name") && document.querySelector(".workspace-registry")?.textContent.includes("Native fixture project")',"Workspace explicit registration did not finish");
  let registry=success(await call("workspace.registry","snapshot"));
  assert.equal(registry.projects.length,1);assert.equal(registry.worktrees.length,1);
  assert.equal(registry.worktrees[0].trustedDigest,null);
  assert.notEqual(registry.projects[0].id,root);
  await click("프로젝트 선택");
  await waitForRenderer(cdp,'!!Array.from(document.querySelectorAll(".workspace-registry button")).find(button => button.textContent.trim() === "프로젝트 선택 해제")',"Workspace selected context did not refresh");
  const selected = await cdp.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe")');
  assert.equal(selected.context.worktreeId,registry.worktrees[0].id);
  const staleContext = await cdp.evaluate(`(async () => {
    const invoke=window.__TAURI_INTERNALS__.invoke;
    const d=await invoke("plugin:product-shell|describe");
    const header={protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+5000,route:"overview"};
    try {await invoke("plugin:workspace|execute",{request:{header,component:"workspace.registry",method:"snapshot",args:{}}});return null;} catch (error) {return error.code;}
  })()`);
  assert.equal(staleContext,"stale-context");
  assert.equal((await call("workspace.registry","select_project",{context:{...selected.context,revision:selected.context.revision+1}})).operation.outcome.state,"failed");
  // Native registration canonicalizes Windows TEMP's possible 8.3 spelling.
  // Feature clients use that returned path, while fixture ownership is checked
  // against the directory we created before any subsequent fixture writes.
  const canonicalRoot = registry.worktrees[0].binding.root;
  assert.equal(realpathSync.native(canonicalRoot), realpathSync.native(root));
  const definitions = await exerciseWorkspaceDefinitions({cdp, root:canonicalRoot, call, success, waitForRenderer});
  const record = (feature, checks) => writeFileSync(`product-foundation-evidence/workspace-${feature}-${suffix}.json`, JSON.stringify({source:process.env.GITHUB_SHA,environment:"github-hosted-windows",result:"pass",checks},null,2));
  record("definitions", definitions);
  const templateImport=await exerciseWorkspaceTemplateImport({cdp,directory,call,success,waitForRenderer});
  record("template-import",templateImport);
  const windowImport=await exerciseWorkspaceWindowImport({cdp,call,success,waitForRenderer});
  record("window-import",windowImport);
  registry = success(await call("workspace.registry","snapshot"));
  const dependencies = await exerciseWorkspaceDependencies({cdp, root:canonicalRoot, call, success, waitForRenderer});
  record("dependencies", dependencies);
  const source = await exerciseWorkspaceSource({cdp, directory, call, success, waitForRenderer, processId, executable});
  record("source", source);
  registry = success(await call("workspace.registry","snapshot"));
  const files = await exerciseWorkspaceFiles({cdp, root:canonicalRoot, directory, call, success, waitForRenderer, processId, executable});
  record("files", files);
  const sessionImport=await exerciseWorkspaceSessionImport({cdp,root:canonicalRoot,call,success,waitForRenderer});
  record("session-import",sessionImport);
  const runtime=await exerciseWorkspaceRuntime({cdp,directory,call,success,waitForRenderer});
  record("runtime",runtime);
  const runtimeWsl=await exerciseWorkspaceRuntimeWsl({call,success});
  record("runtime-wsl",runtimeWsl);
  const runtimeImport=await exerciseWorkspaceRuntimeImport({cdp,directory,call,success});
  record("runtime-import",runtimeImport);
  const lspInstaller = await exerciseWorkspaceLspInstaller({cdp,root:canonicalRoot,directory,call,success,waitForRenderer,processId,executable,network});
  record("lsp-installer",lspInstaller);
  await cdp.evaluate(`(async()=>{const d=await window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe");const label=d.features.find(f=>f.route==="overview").label;Array.from(document.querySelectorAll('nav[aria-label="제품 화면"] button')).find(b=>b.textContent.trim()===label).click();})()`);
  await click("프로젝트 선택 해제");
  await waitForRenderer(cdp,'!Array.from(document.querySelectorAll(".workspace-registry button")).find(button => button.textContent.trim() === "프로젝트 선택 해제")',"Workspace context did not clear");
  assert.equal((await cdp.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe")')).context,null);
  const reviewed=success(await call("workspace.registry","preview_windows",{root}));
  success(await call("workspace.registry","cancel_registration",{previewId:reviewed.previewId}));
  assert.equal((await call("workspace.registry","apply_registration",{previewId:reviewed.previewId,name:"replayed",action:"register"})).operation.outcome.state,"failed");
  registry=success(await call("workspace.registry","rename",{revision:registry.revision,projectId:registry.projects[0].id,name:"Renamed fixture"}));
  assert.equal(registry.projects[0].name,"Renamed fixture");
  const worktree=registry.worktrees[0];
  registry=success(await call("workspace.registry","remove",{revision:registry.revision,context:{projectId:worktree.projectId,worktreeId:worktree.id,revision:worktree.revision,target:worktree.binding.target}}));
  assert.equal(registry.projects.length,0);assert.equal(registry.worktrees.length,0);
  assert.equal(readFileSync(marker,"utf8"),"synthetic project bytes\r\n");
  await click("목록 새로 고침");
  await waitForRenderer(cdp,'(document.querySelector(".workspace-registry")?.textContent ?? "").includes("등록한 프로젝트가 없습니다.")',"Workspace empty registry did not refresh");
  const shot=await cdp.command("Page.captureScreenshot",{format:"png"});
  writeFileSync(`product-foundation-evidence/workspace-registry-${suffix}.png`,Buffer.from(shot.data,"base64"));
  return {authority,definitions,templateImport,windowImport,dependencies,source,files,lspInstaller,runtime,runtimeWsl,runtimeImport,explicitActivation:true,previewCancelDidNotRegister:true,explicitRegistrationUntrusted:true,selectedContextAndStaleHeaderChecked:true,cancelledPreviewRejected:true,renameRemovePreservedProjectFiles:true,boundary:"Actual Windows Registry, definition trust/write, Dependencies, Source approval/selected stage/commit and basic Files UI/native commands; native file dialog, Source worktree creation, LSP and importer acceptance are separate"};
}
