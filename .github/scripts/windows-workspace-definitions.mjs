// Native definition trust consumes filesystem evidence, never executes sources.
import assert from "node:assert/strict";
import {mkdirSync,writeFileSync,existsSync} from "node:fs";
import path from "node:path";

export async function exerciseWorkspaceDefinitions({cdp,root,call,success,waitForRenderer}) {
  const definitions=(method,args={})=>call("workspace.definitions",method,args);
  mkdirSync(path.join(root,".devbox"));
  writeFileSync(path.join(root,".devbox/project.json"),JSON.stringify({schemaVersion:1,tasks:{dev:{kind:"package-script",source:"package.json",selector:"dev"}}}),{flag:"wx"});
  const source=path.join(root,"package.json");
  const writeSource=label=>writeFileSync(source,JSON.stringify({scripts:{dev:`node -e "require('fs').writeFileSync('unexpected-execution.txt','${label}')"`}}));
  writeSource("initial");
  const before=await cdp.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe")');
  const click=async label=>{
    const expression=`Array.from(document.querySelectorAll(".workspace-definitions button")).find(button=>button.textContent.trim()===${JSON.stringify(label)}&&!button.disabled)`;
    await waitForRenderer(cdp,`!!(${expression})`,"Definition action unavailable");
    await cdp.evaluate(`${expression}.click()`);
  };
  await click("프로젝트 설정");
  await click("실행 정의 검토");
  await waitForRenderer(cdp,'!!document.querySelector(".workspace-definitions [aria-label=\"실행 정의 승인 확인\"]")',"Definition review missing");
  assert.equal(success(await call("workspace.registry","snapshot")).worktrees[0].trustedDigest,null);
  await click("승인 취소");
  await waitForRenderer(cdp,'!document.querySelector(".workspace-definitions [aria-label=\"실행 정의 승인 확인\"]")',"Definition cancel missing");
  const stale=success(await definitions("preview_trust"));
  writeSource("changed");
  assert.equal((await definitions("approve_trust",{previewId:stale.previewId})).operation.outcome.state,"failed");
  assert.equal(success(await call("workspace.registry","snapshot")).worktrees[0].trustedDigest,null);
  await click("실행 정의 검토");
  await click("검토한 실행 정의 승인");
  await waitForRenderer(cdp,'document.querySelector(".workspace-definitions")?.textContent.includes("현재 실행 정의의 승인을 확인했습니다.")',"Definition approval did not finish");
  assert.equal(success(await definitions("load")).definitionsTrusted,true);
  const after=await cdp.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe")');
  assert.deepEqual(after.context,before.context);
  writeSource("changed-again");
  const changed=success(await definitions("load"));
  assert.equal(changed.definitionsTrusted,false);
  assert.equal(changed.hasApproval,true);
  success(await definitions("revoke_trust",{revision:changed.registryRevision}));
  assert.equal(success(await call("workspace.registry","snapshot")).worktrees[0].trustedDigest,null);
  assert.equal(existsSync(path.join(root,"unexpected-execution.txt")),false);
  await click("설정 닫기");
  return {explicitReviewCancel:true,changedSourceDenied:true,approvalRetainsWorktreeContext:true,changedSourceInvalidatesDigest:true,explicitRevoke:true,noSourceExecution:true};
}
