// Only synthetic data in an exclusively created hosted-runner legacy namespace.
import assert from "node:assert/strict";
import {mkdirSync,writeFileSync,readFileSync,readdirSync,realpathSync,unlinkSync,rmdirSync} from "node:fs";
import {randomUUID} from "node:crypto";
import path from "node:path";
import {setTimeout as delay} from "node:timers/promises";

export async function exerciseWorkspaceTemplateImport({cdp,directory,call,success,waitForRenderer}) {
  assert.equal(process.env.GITHUB_ACTIONS,"true");assert.equal(process.env.RUNNER_ENVIRONMENT,"github-hosted");
  const sourceRoot=path.join(process.env.LOCALAPPDATA,"com.devbox.workbench");
  mkdirSync(sourceRoot);
  const ownedRoot=realpathSync.native(sourceRoot),nonce=randomUUID();
  const marker=path.join(sourceRoot,".workspace-fixture-owner");writeFileSync(marker,nonce,{flag:"wx"});
  const template={id:randomUUID(),name:"한글 Web 기본값",windowsPath:null,wsl:{distro:"Missing fixture distro",path:"/home/fixture"},gitRoot:null,expectedPorts:[4321],runManagerServiceIds:["never-run-service"]};
  const bytes=JSON.stringify({version:1,templates:[template]});
  writeFileSync(path.join(sourceRoot,"profile-templates.json"),bytes,{flag:"wx"});
  let removed=false;
  const removeOwnedSource=()=>{
    assert.equal(realpathSync.native(sourceRoot),ownedRoot);assert.equal(readFileSync(marker,"utf8"),nonce);
    assert.equal(readFileSync(path.join(sourceRoot,"profile-templates.json"),"utf8"),bytes);
    assert.deepEqual(readdirSync(sourceRoot).sort(),[".workspace-fixture-owner","profile-templates.json"]);
    unlinkSync(path.join(sourceRoot,"profile-templates.json"));unlinkSync(marker);rmdirSync(sourceRoot);removed=true;
  };
  const migration=(method,args={})=>call("workspace.migration",method,args);
  const registry=(method,args={})=>call("workspace.registry",method,args);
  const click=async label=>{
    const expression=`Array.from(document.querySelectorAll('.workspace-registry button')).find(b=>b.textContent.trim()===${JSON.stringify(label)}&&!b.disabled)`;
    await waitForRenderer(cdp,`(()=>{const button=${expression};if(!button)return false;button.click();return true;})()`,"Template fixture action unavailable");
  };
  try {
    const before=success(await registry("snapshot"));
    const job=success(await migration("prepare_legacy_snapshot",{source:"workbench"}));
    const deadline=performance.now()+10_000;let finished;
    do {finished=success(await migration("legacy_snapshot_job"));if(finished.phase==="ready")break;assert.notEqual(finished.phase,"failed");await delay(100);} while(performance.now()<deadline);
    assert.equal(finished.phase,"ready");assert.equal(finished.id,job.id);
    removeOwnedSource();
    const preview=success(await migration("preview_template_import",{jobId:job.id}));
    assert.deepEqual(preview.plan.rows[0].template,template);assert.equal(preview.plan.rows[0].disposition,"new");
    assert.deepEqual(success(await registry("snapshot")),before);
    const applied=success(await migration("apply_template_import",{previewId:preview.previewId,choices:[{sourceId:template.id,decision:"import"}]}));
    assert.equal(applied.result.added,1);const imported=applied.registry.importedTemplates.at(-1);
    assert.notEqual(imported.id,template.id);assert.deepEqual(imported.template,template);
    assert.deepEqual(applied.registry.projects,before.projects);assert.deepEqual(applied.registry.worktrees,before.worktrees);
    assert.equal((await migration("apply_template_import",{previewId:preview.previewId,choices:[]})).operation.outcome.state,"failed");
    const repeated=success(await migration("preview_template_import",{jobId:job.id}));
    assert.equal(repeated.plan.rows[0].disposition,"identical");
    const reused=success(await migration("apply_template_import",{previewId:repeated.previewId,choices:[{sourceId:template.id,decision:"reuse"}]}));
    assert.equal(reused.result.reused,1);assert.deepEqual(reused.registry,applied.registry);
    const root=path.join(directory,"템플릿 concrete project");mkdirSync(root);
    const projectMarker=path.join(root,"preserved.txt");writeFileSync(projectMarker,nonce,{flag:"wx"});
    const selectedBefore=(await cdp.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe")')).context;
    await click("목록 새로 고침");
    await waitForRenderer(cdp,'!!document.querySelector(\'select[aria-label="프로젝트 템플릿"]\')',"Imported template did not reach project creation UI");
    await cdp.evaluate(`(()=>{const input=document.querySelector('select[aria-label="프로젝트 템플릿"]');Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype,"value").set.call(input,${JSON.stringify(imported.id)});input.dispatchEvent(new Event("change",{bubbles:true}));})()`);
    await cdp.evaluate(`(()=>{const input=document.getElementById("workspace-project-path");Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set.call(input,${JSON.stringify(root)});input.dispatchEvent(new Event("input",{bubbles:true}));})()`);
    await click("폴더 확인");
    await waitForRenderer(cdp,'!!document.querySelector(\'section[aria-label="프로젝트 등록 확인"]\')?.textContent.includes("선택한 템플릿으로")',"Template registration review did not display candidate metadata");
    assert.deepEqual(success(await registry("snapshot")),applied.registry);
    await click("등록");
    await waitForRenderer(cdp,'!document.getElementById("workspace-project-name")',"Template registration did not complete");
    const registered=success(await registry("snapshot"));
    const profile=registered.importedProfiles.find(profile=>profile.sourceTemplateId===imported.id);
    assert.ok(profile);assert.notEqual(profile.profile.id,template.id);assert.equal(profile.sourceSnapshotId,imported.sourceSnapshotId);
    assert.deepEqual(profile.profile.expectedPorts,[4321]);assert.equal(profile.profile.environment,null);
    assert.equal(realpathSync.native(profile.profile.windowsPath),realpathSync.native(root));
    const binding=registered.importedProfileBindings.find(binding=>binding.importedId===profile.id);
    const tree=registered.worktrees.find(tree=>tree.id===binding.worktreeId);assert.equal(tree.trustedDigest,null);
    assert.deepEqual((await cdp.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe")')).context,selectedBefore);
    const editorChecks=await exerciseTemplateEditor({cdp,directory,registry,migration,success,waitForRenderer,click,jobId:job.id,imported,registered,profile});
    const current=success(await registry("snapshot"));
    const context={projectId:tree.projectId,worktreeId:tree.id,revision:tree.revision,target:tree.binding.target};
    const unbound=success(await registry("unbind_imported_profile",{revision:current.revision,importedId:profile.id,target:"windows"}));
    const cleaned=success(await registry("remove",{revision:unbound.revision,context}));
    assert.deepEqual(cleaned.projects,before.projects);assert.deepEqual(cleaned.worktrees,before.worktrees);
    assert.deepEqual(cleaned.importedTemplates,current.importedTemplates);assert.equal(readFileSync(projectMarker,"utf8"),nonce);
    await click("목록 새로 고침");
    return {...editorChecks,sourcePreserved:true,verifiedSnapshotIndependentOfSource:true,originalTemplateIdAndDefaults:true,repeatReuses:true,tokenReplayRejected:true,actualTemplateCreationUi:true,atomicProfileAndBinding:true,explicitRegistrationWithoutSelectionOrTrust:true,unbindingPreservesMetadataAndFiles:true};
  } finally {if(!removed)removeOwnedSource();}
}


async function exerciseTemplateEditor({cdp,directory,registry,migration,success,waitForRenderer,click,jobId,imported,registered,profile}) {
  const fill=async(label,value)=>cdp.evaluate(`(()=>{const input=Array.from(document.querySelectorAll('form[aria-label="템플릿 편집"] label')).find(label=>label.firstChild?.textContent===${JSON.stringify(label)}).querySelector("input");Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set.call(input,${JSON.stringify(value)});input.dispatchEvent(new Event("input",{bubbles:true}));})()`);
  const expand=async(name)=>cdp.evaluate(`(()=>{const details=Array.from(document.querySelectorAll('section[aria-label="템플릿 관리"] details')).find(details=>details.querySelector("summary").textContent===${JSON.stringify(name)});if(!details.open)details.querySelector("summary").click();})()`);
  const save=async()=>{await click("템플릿 저장");await waitForRenderer(cdp,`!document.querySelector('form[aria-label="템플릿 편집"]')`,"Template editor did not finish");};
  await click("템플릿 관리 열기");
  await waitForRenderer(cdp,`!!document.querySelector('section[aria-label="템플릿 관리"]')`,"Template manager did not load");
  await expand(imported.template.name);await click("템플릿 수정");
  await fill("기본 포트 (쉼표로 구분)","9090");await save();
  const edited=success(await registry("snapshot"));
  assert.deepEqual(edited.importedTemplates.find(entry=>entry.id===imported.id).template.expectedPorts,[9090]);
  assert.deepEqual(edited.importedProfiles,registered.importedProfiles);assert.deepEqual(edited.importedProfileBindings,registered.importedProfileBindings);
  assert.deepEqual(edited.projects,registered.projects);assert.deepEqual(edited.worktrees,registered.worktrees);
  const stale=await registry("save_template",{revision:registered.revision,id:imported.id,template:imported.template});
  assert.equal(stale.operation.outcome.state,"failed");assert.equal(stale.value.issue,"stale_registry");
  assert.deepEqual(success(await registry("snapshot")),edited);
  await click("템플릿 보관");await click("템플릿 보관 취소");assert.deepEqual(success(await registry("snapshot")),edited);
  await click("템플릿 보관");await click("템플릿 보관 확인");
  await waitForRenderer(cdp,`!document.querySelector('section[aria-label="템플릿 보관 확인"]')`,"Template archive did not finish");
  await waitForRenderer(cdp,`!Array.from(document.querySelectorAll('select[aria-label="프로젝트 템플릿"] option')).some(option=>option.value===${JSON.stringify(imported.id)})`,"Archived template remains selectable");
  const archived=success(await registry("snapshot"));assert.equal(archived.importedTemplates.find(entry=>entry.id===imported.id).archived,true);
  assert.deepEqual(archived.importedProfiles,registered.importedProfiles);assert.deepEqual(archived.importedProfileBindings,registered.importedProfileBindings);
  const denied=await registry("preview_template_profile_windows",{templateId:imported.id,root:profile.profile.windowsPath,name:"archived"});
  assert.equal(denied.operation.outcome.state,"failed");assert.equal(denied.value.issue,"unknown_imported_template");
  const repeat=success(await migration("preview_template_import",{jobId}));assert.equal(repeat.plan.rows[0].alreadyImported,true);
  const reused=success(await migration("apply_template_import",{previewId:repeat.previewId,choices:[{sourceId:imported.template.id,decision:"reuse"}]}));
  assert.equal(reused.result.reused,1);assert.deepEqual(reused.registry,archived);
  await click("템플릿 복원 검토");await save();
  await waitForRenderer(cdp,`Array.from(document.querySelectorAll('select[aria-label="프로젝트 템플릿"] option')).some(option=>option.value===${JSON.stringify(imported.id)})`,"Restored template remains unavailable");
  await click("새 템플릿");await fill("템플릿 이름","Workspace 로컬 기본값");await fill("기본 포트 (쉼표로 구분)","5173");await save();
  const localRegistry=success(await registry("snapshot"));const local=localRegistry.importedTemplates.find(entry=>entry.local===true);
  assert.ok(local);assert.equal(local.sourceSnapshotId,undefined);assert.notEqual(local.id,local.template.id);
  const localRoot=path.join(directory,"로컬 템플릿 project");mkdirSync(localRoot);
  await waitForRenderer(cdp,`Array.from(document.querySelectorAll('select[aria-label="프로젝트 템플릿"] option')).some(option=>option.value===${JSON.stringify(local.id)})`,"Local template did not reach creation UI");
  await cdp.evaluate(`(()=>{const select=document.querySelector('select[aria-label="프로젝트 템플릿"]');Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype,"value").set.call(select,${JSON.stringify(local.id)});select.dispatchEvent(new Event("change",{bubbles:true}));const input=document.getElementById("workspace-project-path");Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set.call(input,${JSON.stringify(localRoot)});input.dispatchEvent(new Event("input",{bubbles:true}));})()`);
  await click("폴더 확인");await click("등록");await waitForRenderer(cdp,'!document.getElementById("workspace-project-name")',"Local template registration did not finish");
  const created=success(await registry("snapshot"));const concrete=created.importedProfiles.find(entry=>entry.sourceTemplateId===local.id);
  assert.ok(concrete);assert.equal(concrete.local,true);assert.equal(concrete.sourceSnapshotId,undefined);assert.deepEqual(concrete.profile.expectedPorts,[5173]);
  const binding=created.importedProfileBindings.find(binding=>binding.importedId===concrete.id);const tree=created.worktrees.find(tree=>tree.id===binding.worktreeId);
  assert.equal(tree.trustedDigest,null);assert.deepEqual(created.importedProfiles.find(entry=>entry.id===profile.id),profile);
  const unbound=success(await registry("unbind_imported_profile",{revision:created.revision,importedId:concrete.id,target:"windows"}));
  success(await registry("remove",{revision:unbound.revision,context:{projectId:tree.projectId,worktreeId:tree.id,revision:tree.revision,target:tree.binding.target}}));
  return {actualTemplateEditUi:true,staleTemplateWriteRejected:true,existingProfilesPreserved:true,explicitArchiveAndRestoreUi:true,repeatPreservesEditsAndArchive:true,localTemplateCreationAndInstantiationUi:true};
}
