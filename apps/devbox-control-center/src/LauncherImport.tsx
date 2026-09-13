import {useState,useEffect} from "react";
import type {ShellContentProps} from "@devbox/product-shell";
import {importLauncher,type LauncherImport as ImportPlan} from "@devbox/product-shell/commands";
export default function LauncherImport({description,route}:ShellContentProps){
  const [plan,setPlan]=useState<ImportPlan|null>(null),[resume,setResume]=useState(false),[busy,setBusy]=useState(false),[issue,setIssue]=useState("");
  useEffect(()=>{let alive=true;void importLauncher(description,route,"status").then(value=>{if(alive){setPlan(value);setResume(Boolean(value&&!value.committed));}}).catch(()=>{if(alive)setIssue("이전 가져오기 상태를 읽지 못했습니다.");});return()=>{alive=false;};},[description,route]);
  const run=async(method:"preview"|"apply"|"resume")=>{
    setBusy(true);setIssue("");
    try{const value=await importLauncher(description,route,method,method==="preview"?null:plan?.id);setPlan(value);setResume(method==="resume"&&Boolean(value&&!value.committed));}
    catch{setIssue("가져오기를 완료하지 못했습니다. 원본·대상 설정이 변경되었거나 이전 작업을 재개해야 할 수 있습니다.");
      try{const journal=await importLauncher(description,route,"status");if(journal){setPlan(journal);setResume(!journal.committed);}}catch{/* Keep the original reviewed plan visible. */}}
    finally{setBusy(false);}
  };
  return <section><h1>Launcher 설정 가져오기</h1><p>기존 Launcher의 즐겨찾기·최근 기록을 현재 제품의 명령에 연결합니다. 원본 설정은 보존합니다.</p>
    {issue&&<p role="alert">{issue}</p>}
    <button disabled={busy||resume} onClick={()=>void run("preview")}>가져오기 미리 보기</button>
    {plan&&<><p role="status">{plan.committed?"설정을 가져왔습니다.":resume?"중단된 가져오기가 있습니다. 같은 대상 설정에서 재개합니다.":"아래 내용을 확인한 뒤 가져오세요."}</p>
      <p>즐겨찾기 {plan.plan.preferences.favorites.length}개 · 최근 기록 {plan.plan.preferences.recents.length}개</p>
      {plan.plan.legacyShortcut&&<p>기존 단축키: {plan.plan.legacyShortcut.accelerator} ({plan.plan.legacyShortcut.enabled?"사용":"사용 안 함"})</p>}
      {plan.plan.launcherTerminalConflict&&<p>기존 Terminal의 기본 단축키와 겹칠 수 있습니다. 공통 설정에서 Launcher 조합을 확인하고 Terminal Ctrl+Alt+T, 캡처 Ctrl+Alt+N을 사용할 수 있습니다.</p>}
      <p>{plan.plan.proposedShortcut?"가져온 단축키는 사용 안 함 상태로 저장합니다. 제품 연결과 충돌을 확인한 뒤 공통 설정에서 켜세요.":"기존 suite 단축키 설정을 유지합니다."}</p>
      {([['대응할 수 없는 즐겨찾기',plan.plan.unresolvedFavorites],['대응할 수 없는 최근 항목',plan.plan.unresolvedRecents],['개수 제한으로 제외한 즐겨찾기',plan.plan.capacityFavorites],['개수 제한으로 제외한 최근 항목',plan.plan.capacityRecents]] as const).map(([label,ids])=>ids.length>0&&<details key={label}><summary>{label} ({ids.length})</summary><ul>{ids.map(id=><li key={id}><code>{id}</code></li>)}</ul></details>)}
      {!plan.committed&&<button disabled={busy} onClick={()=>void run(resume?"resume":"apply")}>{resume?"가져오기 재개":"확인한 설정 가져오기"}</button>}
    </>}
  </section>;
}
