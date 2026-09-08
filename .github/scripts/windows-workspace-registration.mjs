// Actual hidden Workspace native admission/registration with owned fixtures.
import assert from "node:assert/strict";
import {mkdirSync, writeFileSync, readFileSync} from "node:fs";
import path from "node:path";
import {exerciseWorkspaceFiles} from "./windows-workspace-files.mjs";

export function workspaceRequestExpression(component, method, args = {}) {
  const route = component === "workspace.files" || component === "workspace.lsp" ? "files" : "overview";
  return `(async () => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const d = await invoke("plugin:product-shell|describe");
    const header = {protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+5000,route:${JSON.stringify(route)},...(d.context ? {context:d.context} : {})};
    return invoke("plugin:workspace|execute",{request:{header,...${JSON.stringify({component, method, args})}}});
  })()`;
}

export async function exerciseWorkspaceRegistration({cdp, directory, waitForRenderer, suffix}) {
  const root = path.join(directory, "한글 project");
  mkdirSync(root);
  const marker = path.join(root, "preserved.txt");
  writeFileSync(marker, "synthetic project bytes\r\n", {flag:"wx"});
  const call = async (component, method, args = {}) => cdp.evaluate(workspaceRequestExpression(component, method, args));
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
  const files = await exerciseWorkspaceFiles({cdp, root, directory, call, success, waitForRenderer});
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
  return {authority,files,explicitActivation:true,previewCancelDidNotRegister:true,explicitRegistrationUntrusted:true,selectedContextAndStaleHeaderChecked:true,cancelledPreviewRejected:true,renameRemovePreservedProjectFiles:true,boundary:"Actual Windows Registry and basic Files UI/native commands; native file dialog, Source, LSP and importer acceptance are separate"};
}
