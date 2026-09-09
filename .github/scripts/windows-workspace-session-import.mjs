// Synthetic legacy data is created exclusively on the hosted acceptance runner.
import assert from "node:assert/strict";
import {mkdirSync,writeFileSync,readFileSync,readdirSync,realpathSync,unlinkSync,rmdirSync} from "node:fs";
import {randomUUID} from "node:crypto";
import path from "node:path";
import {setTimeout as delay} from "node:timers/promises";

export async function exerciseWorkspaceSessionImport({cdp,root,call,success,waitForRenderer}) {
  assert.equal(process.env.GITHUB_ACTIONS,"true");assert.equal(process.env.RUNNER_ENVIRONMENT,"github-hosted");
  const sourceRoot=path.join(process.env.LOCALAPPDATA,"com.devbox.codepad");
  // mkdir is exclusive. An existing legacy folder is never inspected or changed.
  mkdirSync(sourceRoot);
  const ownedRoot=realpathSync.native(sourceRoot),nonce=randomUUID();
  const marker=path.join(sourceRoot,".workspace-fixture-owner");
  writeFileSync(marker,nonce,{flag:"wx"});
  const file=path.join(root,"imported 한글 bookmarks.txt");
  const text="first\r\nsecond\r\nthird\r\nfourth\r\n";
  writeFileSync(file,text,{flag:"wx"});
  const session={version:1,workspace_folder:root,docs:[{id:"original-codepad-document",path:file,cursor:7,bookmarks:[1,2]}],views:[[],["original-codepad-document"]],active_view:1,active_doc_by_view:[null,"original-codepad-document"],recent_files:[file]};
  const bytes=JSON.stringify(session);
  writeFileSync(path.join(sourceRoot,"session.json"),bytes,{flag:"wx"});
  let removed=false;
  const removeOwnedSource=()=>{
    assert.equal(realpathSync.native(sourceRoot),ownedRoot);
    assert.equal(readFileSync(marker,"utf8"),nonce);
    assert.equal(readFileSync(path.join(sourceRoot,"session.json"),"utf8"),bytes);
    assert.deepEqual(readdirSync(sourceRoot).sort(),[".workspace-fixture-owner","session.json"]);
    unlinkSync(path.join(sourceRoot,"session.json"));unlinkSync(marker);rmdirSync(sourceRoot);removed=true;
  };
  const files=(method,args={})=>call("workspace.files",method,args);
  const click=async label=>{
    const expression=`(() => {const button=Array.from(document.querySelectorAll('.workspace-feature-files button')).find(b=>b.textContent.trim()===${JSON.stringify(label)}&&!b.disabled);if(!button)return false;button.click();return true;})()`;
    const deadline=performance.now()+10_000;
    while(performance.now()<deadline){if(await cdp.evaluate(expression))return;await delay(100);}
    throw new Error(`Session fixture button unavailable: ${label}`);
  };
  try {
    const job=success(await call("workspace.migration","prepare_legacy_snapshot",{source:"code-pad"}));
    const deadline=performance.now()+10_000;
    let finished;
    do {finished=success(await call("workspace.migration","legacy_snapshot_job"));if(finished.phase==="ready")break;assert.notEqual(finished.phase,"failed");await delay(100);} while(performance.now()<deadline);
    assert.equal(finished.phase,"ready");assert.equal(finished.id,job.id);
    removeOwnedSource();
    // Let the previous editor close's documented one-second debounce settle.
    await delay(1300);
    const before=success(await files("load_session"));
    assert.equal(before.session.docs.length,0);
    await cdp.evaluate(`(async()=>{const d=await window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe");const label=d.features.find(f=>f.route==="files").label;Array.from(document.querySelectorAll('nav[aria-label="제품 화면"] button')).find(b=>b.textContent.trim()===label).click();})()`);
    await click("세션 가져오기 검토");
    await waitForRenderer(cdp,`!!document.querySelector('section[aria-label="Code Pad 세션 가져오기"] input[type=checkbox]')`,"Existing session conflict was not reviewed");
    await cdp.evaluate(`document.querySelector('section[aria-label="Code Pad 세션 가져오기"] input[type=checkbox]').click()`);
    await click("검토한 세션 가져오기");
    await waitForRenderer(cdp,'document.querySelectorAll(".workspace-feature-files .cm-bookmark-marker-dot").length===2',"Imported bookmarks did not reach the actual CodeMirror editor");
    const loaded=success(await files("load_session"));
    assert.equal(loaded.session.docs[0].id,session.docs[0].id);assert.deepEqual(loaded.session.docs[0].bookmarks,[1,2]);
    assert.equal(loaded.session.active_view,1);
    assert.equal((await files("save_session",{session:before.session,nativeRevision:before.nativeRevision})).value.issue,"files_session_changed");
    assert.equal(readFileSync(file,"utf8"),text);
    await cdp.evaluate('document.querySelector(".workspace-feature-files .document-tab .tab-action").click()');
    await waitForRenderer(cdp,'!document.querySelector(".workspace-feature-files [role=tab]")',"Imported tab did not close");
    await delay(1300);
    const repeated=success(await files("preview_session_import",{jobId:job.id}));
    assert.equal(repeated.alreadyImported,true);
    assert.equal(success(await files("apply_session_import",{previewId:repeated.previewId,replaceExisting:false})).reused,true);
    await click("이전 세션 목록");await click("복원 검토");
    await waitForRenderer(cdp,`!!document.querySelector('section[aria-label="Code Pad 세션 가져오기"] input[type=checkbox]')`,"Session history replacement lacked review");
    await cdp.evaluate(`document.querySelector('section[aria-label="Code Pad 세션 가져오기"] input[type=checkbox]').click()`);
    await click("검토한 이전 세션 복원");
    await waitForRenderer(cdp,`Array.from(document.querySelectorAll('section[aria-label="Code Pad 세션 가져오기"] [role=status]')).some(node=>node.textContent.includes("복원했습니다"))`,"Session preimage was not restored");
    assert.deepEqual(success(await files("load_session")).session,before.session);
    assert.equal(readFileSync(file,"utf8"),text);
    return {verifiedSnapshotSurvivesSourceRemoval:true,explicitConflictReview:true,originalDocumentIdAndTwoViews:true,actualEditorBookmarks:true,staleAutosaveRejected:true,repeatPreservesCurrentSession:true,previousSessionRestored:true,repositoryBytesUnchanged:true};
  } finally {if(!removed)removeOwnedSource();}
}
