import {useEffect,useRef,useState} from "react";
import type {Description} from "@devbox/product-shell/api";
import {componentCall,issueMessage} from "./native";
type Entry={path:string;content:string;base_hash:string|null;snapshot_at_ms:number};
type Preview={previewId:string;candidate:{recovery:{entries:Entry[]};skippedEntries:number};currentEntries:Entry[];conflictingPaths:string[];conflict:boolean;alreadyImported:boolean;restoring:boolean};
type Applied={importedEntries:number;reused:boolean;restored?:boolean};
type History={items:{id:string;entries:number|null;issue:string|null}[];unrecognized:number};
export default function LegacyRecoveryImport({description,disabled,onBusyChange,onApplied}:{description:Description;disabled:boolean;onBusyChange:(busy:boolean)=>void;onApplied:()=>void}) {
  const [preview,setPreview]=useState<Preview|null>(null),[replace,setReplace]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState(""),[result,setResult]=useState<Applied|null>(null);
  const alive=useRef(false),pending=useRef<string|null>(null);
  const [history,setHistory]=useState<History|null>(null);
  const call=<T,>(method:string,args:Record<string,unknown>={})=>componentCall<T>(description,"workspace.files",method,args,"files");
  useEffect(()=>{alive.current=true;return()=>{alive.current=false;if(pending.current)void call("cancel_recovery_import",{previewId:pending.current}).catch(()=>{});onBusyChange(false);};},[]);
  async function act(action:()=>Promise<void>) {
    if(busy)return;
    setBusy(true);onBusyChange(true);setError("");
    try{await action();}catch(cause){if(alive.current)setError(cause instanceof Error?cause.message:"복구 버퍼 가져오기를 완료하지 못했습니다.");}
    finally{if(alive.current){setBusy(false);onBusyChange(false);}}
  }
  return <section aria-label="Code Pad 복구 버퍼 가져오기" aria-busy={busy}>
    <h2>Code Pad 복구 버퍼 가져오기</h2>
    <p>Overview에서 확인한 미저장 버퍼를 현재 프로젝트와 직접 선택한 파일에 연결합니다. 열린 탭을 모두 닫고 가져온 뒤 Files의 복구 화면에서 실제 파일에 적용할 내용을 다시 검토합니다.</p>
    {error&&<p role="alert">{error}</p>}
    <button disabled={busy||!!preview} onClick={()=>void act(async()=>{const value=await call<History>("list_recovery_history");if(alive.current)setHistory(value);})}>이전 복구 버퍼 목록</button>
    {history&&<section aria-label="보관된 이전 복구 버퍼">
      {history.items.length===0&&<p>보관된 이전 복구 버퍼이 없습니다.</p>}
      <ul>{history.items.map((item,index)=><li key={item.id}>이전 복구 버퍼 {index+1} · {item.issue?issueMessage(item.issue):`미저장 버퍼 ${item.entries}개`}
        <button disabled={busy||disabled||!!preview||!!item.issue} aria-label={`이전 복구 버퍼 ${index+1} 복원 검토`} onClick={()=>void act(async()=>{
          const next=await call<Preview>("preview_recovery_restore",{backupId:item.id});
          if(!alive.current){await call("cancel_recovery_import",{previewId:next.previewId});return;}
          pending.current=next.previewId;setPreview(next);setReplace(false);setResult(null);
        })}>복원 검토</button></li>)}</ul>
      {history.unrecognized>0&&<p>확인할 수 없는 항목 {history.unrecognized}개를 그대로 유지했습니다.</p>}
    </section>}
    {!preview&&<button disabled={busy||disabled} onClick={()=>void act(async()=>{
      const job=await componentCall<{id:string;source:string;phase:string}|null>(description,"workspace.migration","legacy_snapshot_job",{},"overview");
      if(!job||job.source!=="code-pad"||job.phase!=="ready")throw new Error("Overview에서 Code Pad 보관본의 내용을 먼저 확인해 주세요.");
      const next=await call<Preview>("preview_recovery_import",{jobId:job.id});
      if(!alive.current){await call("cancel_recovery_import",{previewId:next.previewId});return;}
      pending.current=next.previewId;setPreview(next);setReplace(false);setResult(null);
    })}>복구 버퍼 가져오기 검토</button>}
    {preview&&<>
      <p>가져올 미저장 버퍼 {preview.candidate.recovery.entries.length}개 · 현재 보관된 버퍼 {preview.currentEntries.length}개</p>
      <ul>{preview.candidate.recovery.entries.map(entry=><li key={entry.path}>{entry.path} · {entry.content.length}자
        <details><summary>버퍼 내용 확인</summary>{preview.conflictingPaths.includes(entry.path)&&<><p>현재 버퍼</p><pre>{preview.currentEntries.find(old=>old.path===entry.path)?.content}</pre><p>가져올 버퍼</p></>}<pre>{entry.content}</pre></details>
      </li>)}</ul>
      {preview.candidate.skippedEntries>0&&<p>현재 선택으로 가져오지 않는 버퍼 {preview.candidate.skippedEntries}개는 원본 보관본에 남아 있습니다.</p>}
      {preview.alreadyImported?<p>이 항목은 이미 가져왔습니다. 이후 변경하거나 폐기한 버퍼를 유지합니다.</p>:preview.conflict&&<label><input type="checkbox" checked={replace} disabled={busy} onChange={event=>setReplace(event.target.checked)}/>{preview.restoring?"현재 복구 버퍼 전체를 별도로 보관하고 검토한 이전 상태로 바꿉니다.":`같은 파일의 다른 버퍼 ${preview.conflictingPaths.length}개를 별도로 보관하고 검토한 내용으로 바꿉니다. 다른 파일의 버퍼는 유지합니다.`}</label>}
      <button disabled={busy||disabled||(!preview.alreadyImported&&preview.conflict&&!replace)} onClick={()=>void act(async()=>{
        const id=preview.previewId;pending.current=null;
        try{const applied=await call<Applied>("apply_recovery_import",{previewId:id,replaceExisting:replace});if(alive.current){setResult(applied);if(!applied.reused){setHistory(null);onApplied();}}}
        finally{if(alive.current)setPreview(null);}
      })}>{preview.alreadyImported?"가져오기 상태 확인":preview.restoring?"검토한 이전 복구 버퍼 복원":"검토한 복구 버퍼 가져오기"}</button>
      <button disabled={busy} onClick={()=>void act(async()=>{await call("cancel_recovery_import",{previewId:preview.previewId});pending.current=null;if(alive.current)setPreview(null);})}>복구 버퍼 가져오기 취소</button>
    </>}
    {result&&<p role="status">{result.reused?"이미 가져온 항목입니다. 현재 복구 버퍼을 유지했습니다.":`미저장 버퍼 ${result.importedEntries}개를 ${result.restored?"복원했습니다":"가져왔습니다"}.`}</p>}
  </section>;
}
