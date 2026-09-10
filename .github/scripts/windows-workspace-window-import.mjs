// Hosted synthetic namespace only; actual native main-window review/apply/restore.
import assert from "node:assert/strict";
import {mkdirSync,writeFileSync,readFileSync,readdirSync,realpathSync,unlinkSync,rmdirSync} from "node:fs";
import {randomUUID} from "node:crypto";
import path from "node:path";
import {setTimeout as delay} from "node:timers/promises";
export async function exerciseWorkspaceWindowImport({cdp,call,success,waitForRenderer}) {
  assert.equal(process.env.GITHUB_ACTIONS,"true");assert.equal(process.env.RUNNER_ENVIRONMENT,"github-hosted");
  const source=path.join(process.env.LOCALAPPDATA,"com.devbox.repomanager");mkdirSync(source);
  const owned=realpathSync.native(source),nonce=randomUUID(),marker=path.join(source,".workspace-fixture-owner");
  writeFileSync(marker,nonce,{flag:"wx"});
  const saved={schemaVersion:1,bounds:{x:120,y:90,width:1000,height:700},monitorId:"missing fixture monitor",monitorWorkArea:{x:0,y:0,width:1920,height:1080},scaleFactor:1,maximized:false};
  const bytes=JSON.stringify(saved),file=path.join(source,"window-state-v1.json");writeFileSync(file,bytes,{flag:"wx"});let removed=false;
  const remove=()=>{assert.equal(realpathSync.native(source),owned);assert.equal(readFileSync(marker,"utf8"),nonce);assert.equal(readFileSync(file,"utf8"),bytes);assert.deepEqual(readdirSync(source).sort(),[".workspace-fixture-owner","window-state-v1.json"]);unlinkSync(file);unlinkSync(marker);rmdirSync(source);removed=true;};
  const migration=(method,args={})=>call("workspace.migration",method,args);
  const click=async label=>{const expression=`Array.from(document.querySelectorAll('button')).find(b=>b.textContent.trim()===${JSON.stringify(label)}&&!b.disabled)`;await waitForRenderer(cdp,`!!(${expression})`,"Window import action unavailable");await cdp.evaluate(`(${expression}).click()`);};
  try {
    const registry=success(await call("workspace.registry","snapshot"));
    const job=success(await migration("prepare_legacy_snapshot",{source:"repo-manager"}));let state;
    const until=performance.now()+10_000;
    do {state=success(await migration("legacy_snapshot_job"));if(state.phase==="ready")break;assert.notEqual(state.phase,"failed");await delay(100);} while(performance.now()<until);
    assert.equal(state.phase,"ready");assert.equal(state.manifest.schemaVersion,2);assert.equal(state.manifest.files[0].records,1);remove();
    const cancelled=success(await migration("preview_window_import",{jobId:job.id}));assert.deepEqual(cancelled.source,saved);
    success(await migration("cancel_window_import",{previewId:cancelled.previewId}));
    assert.equal((await migration("apply_window_import",{previewId:cancelled.previewId,replaceExisting:true})).value.issue,"window_review_stale");
    const review=success(await migration("preview_window_import",{jobId:job.id}));assert.deepEqual(review.before,cancelled.before);
    success(await migration("cancel_window_import",{previewId:review.previewId}));
    await click("보관 상태 새로 고침");await click("기존 창 상태 검토");
    await waitForRenderer(cdp,'!!document.querySelector(\'section[aria-label="창 상태 변경 확인"]\')',"Native window review not shown");
    assert.equal(await cdp.evaluate('Array.from(document.querySelectorAll("button")).find(b=>b.textContent.trim()==="창 상태 적용").disabled'),true);
    await cdp.evaluate('document.querySelector(\'section[aria-label="창 상태 변경 확인"] input[type=checkbox]\').click()');
    await click("창 상태 적용");
    await waitForRenderer(cdp,'!!document.querySelector(\'section[aria-label="기존 창 상태 가져오기"]\')?.textContent.includes("검토한 창 위치와 크기를 적용했습니다.")',"Native window application failed");
    const current=success(await migration("preview_window_import",{jobId:job.id}));
    assert.deepEqual(current.before,review.after);success(await migration("cancel_window_import",{previewId:current.previewId}));
    const history=success(await migration("list_window_history"));const before=history.items.find(item=>JSON.stringify(item.state)===JSON.stringify(review.before));assert.ok(before);
    const restore=success(await migration("preview_window_restore",{backupId:before.id}));
    assert.deepEqual(restore.after,review.before);success(await migration("apply_window_import",{previewId:restore.previewId,replaceExisting:true}));
    const restored=success(await migration("preview_window_import",{jobId:job.id}));assert.deepEqual(restored.before,review.before);success(await migration("cancel_window_import",{previewId:restored.previewId}));
    assert.deepEqual(success(await call("workspace.registry","snapshot")),registry);
    return {originalBytesPreserved:true,verifiedSourceIndependent:true,nativeCurrentAndMonitorReview:true,cancelAndReplay:true,actualExplicitWindowApplyUi:true,previousNativeGeometryRestored:true,registryUnchanged:true};
  } finally {if(!removed)remove();}
}
