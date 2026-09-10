// Native dependency parsing and reviewed transmission; never contact public APIs.
import assert from "node:assert/strict";
import {readFileSync, writeFileSync, existsSync} from "node:fs";
import path from "node:path";

export async function exerciseWorkspaceDependencies({cdp,root,call,success,waitForRenderer}) {
  const dependencies=(method,request={})=>call("workspace.dependencies",method,{request:{path:root,...request}});
  const rejected=result=>assert.equal(result.operation.outcome.state,"failed",JSON.stringify(result));
  const manifest=path.join(root,"package.json");
  const before=JSON.parse(readFileSync(manifest,"utf8"));
  writeFileSync(manifest,JSON.stringify({...before,name:"owned-fixture",version:"1.0.0",dependencies:{"fixture-dependency":"1.2.3"}}));
  const lock=path.join(root,"package-lock.json");
  const writeLock=version=>writeFileSync(lock,JSON.stringify({name:"owned-fixture",version:"1.0.0",lockfileVersion:3,packages:{"":{name:"owned-fixture",version:"1.0.0",dependencies:{"fixture-dependency":"1.2.3"}},"node_modules/fixture-dependency":{version,resolved:`https://registry.npmjs.org/fixture-dependency/-/fixture-dependency-${version}.tgz`}}}));
  writeLock("1.2.3");
  assert.equal(existsSync(path.join(root,".git")),false);
  const report=success(await dependencies("dependency_inventory"));
  assert.ok(report.packages.some(item=>item.name==="fixture-dependency"&&item.version==="1.2.3"));
  rejected(await dependencies("dependency_inventory",{path:path.dirname(root)}));
  const services={osv:true,depsDev:true};
  const preview=success(await dependencies("dependency_enrichment_preview",{services,forceRefresh:false}));
  assert.equal(preview.revision,report.revision);
  assert.ok(preview.services.some(service=>service.transmitted.length>0));
  success(await dependencies("dependency_enrichment_cancel",{previewToken:preview.token}));
  rejected(await dependencies("dependency_enrichment_execute",{previewToken:preview.token}));
  const stale=success(await dependencies("dependency_enrichment_preview",{services,forceRefresh:false}));
  writeLock("1.2.4");
  rejected(await dependencies("dependency_enrichment_execute",{previewToken:stale.token}));
  writeLock("1.2.3");
  await cdp.evaluate(`(async()=>{const d=await window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe");const label=d.features.find(f=>f.route==="dependencies").label;Array.from(document.querySelectorAll('nav[aria-label="제품 화면"] button')).find(b=>b.textContent.trim()===label).click();})()`);
  await waitForRenderer(cdp,'!!document.querySelector(".dependency-lens-panel")',"Dependency panel unavailable");
  assert.equal(await cdp.evaluate('!!document.querySelector(".dependency-enrichment-preview")'),false);
  const click=async label=>{
    const target=`Array.from(document.querySelectorAll(".dependency-lens-panel button")).find(b=>b.textContent.trim()===${JSON.stringify(label)}&&!b.disabled)`;
    await waitForRenderer(cdp,`!!(${target})`,"Dependency action unavailable");
    await cdp.evaluate(`${target}.click()`);
  };
  await click("의존성 분석");
  await waitForRenderer(cdp,'document.querySelector(".dependency-lens-panel")?.textContent.includes("fixture-dependency")',"Dependency inventory missing");
  await click("전송 내용 검토");
  await waitForRenderer(cdp,'!!document.querySelector(".dependency-enrichment-preview")',"Dependency preview missing");
  assert.equal(await cdp.evaluate('Array.from(document.querySelectorAll(".workspace-registry button")).find(b=>b.textContent.trim()==="프로젝트 선택 해제").disabled'),true);
  await click("전송 검토 취소");
  await waitForRenderer(cdp,'!document.querySelector(".dependency-enrichment-preview") && !Array.from(document.querySelectorAll(".workspace-registry button")).find(b=>b.textContent.trim()==="프로젝트 선택 해제").disabled',"Dependency cancellation did not release selection");
  assert.equal(existsSync(path.join(root,"unexpected-execution.txt")),false);
  assert.deepEqual(JSON.parse(readFileSync(manifest,"utf8")).scripts,before.scripts);
  return {plainFolderOfflineAnalysis:true,rendererPathRejected:true,reviewCancelledAndReplayRejected:true,changedLockRejectedBeforeNetwork:true,uiAnalyzeReviewCancel:true,selectionLockedDuringReview:true,sourceScriptsNeverExecuted:true,boundary:"Native parser and approval/cancellation only; public OSV/deps.dev requests are not issued by this fixture"};
}
