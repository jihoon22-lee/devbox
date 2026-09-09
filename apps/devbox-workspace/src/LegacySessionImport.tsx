import {useEffect,useRef,useState} from "react";
import type {Description} from "@devbox/product-shell/api";
import {componentCall,issueMessage} from "./native";
type Preview={previewId:string;candidate:{session:{docs:{id:string;path:string;cursor:number;bookmarks:number[]}[];recent_files:string[]};skippedDocuments:number;skippedRecentFiles:number};currentDocuments:number;currentRecentFiles:number;conflict:boolean;alreadyImported:boolean;restoring:boolean};
type Applied={importedDocuments:number;importedRecentFiles:number;reused:boolean;restored?:boolean};
type History={items:{id:string;documents:number|null;recentFiles:number|null;issue:string|null}[];unrecognized:number};
export default function LegacySessionImport({description,disabled,onBusyChange,onApplied}:{description:Description;disabled:boolean;onBusyChange:(busy:boolean)=>void;onApplied:()=>void}) {
  const [preview,setPreview]=useState<Preview|null>(null),[replace,setReplace]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState(""),[result,setResult]=useState<Applied|null>(null);
  const alive=useRef(false),pending=useRef<string|null>(null);
  const [history,setHistory]=useState<History|null>(null);
  const call=<T,>(method:string,args:Record<string,unknown>={})=>componentCall<T>(description,"workspace.files",method,args,"files");
  useEffect(()=>{alive.current=true;return()=>{alive.current=false;if(pending.current)void call("cancel_session_import",{previewId:pending.current}).catch(()=>{});onBusyChange(false);};},[]);
  async function act(action:()=>Promise<void>) {
    if(busy)return;
    setBusy(true);onBusyChange(true);setError("");
    try{await action();}catch(cause){if(alive.current)setError(cause instanceof Error?cause.message:"세션 가져오기를 완료하지 못했습니다.");}
    finally{if(alive.current){setBusy(false);onBusyChange(false);}}
  }
  return <section aria-label="Code Pad 세션 가져오기" aria-busy={busy}>
    <h2>Code Pad 세션 가져오기</h2>
    <p>Overview에서 확인한 보관본의 열린 파일·커서·북마크·최근 파일을 복원합니다. 현재 프로젝트와 직접 선택한 파일에 해당하는 항목을 검토하며, 열린 탭을 모두 닫아야 적용할 수 있습니다.</p>
    {error&&<p role="alert">{error}</p>}
    <button disabled={busy||!!preview} onClick={()=>void act(async()=>{const value=await call<History>("list_session_history");if(alive.current)setHistory(value);})}>이전 세션 목록</button>
    {history&&<section aria-label="보관된 이전 세션">
      {history.items.length===0&&<p>보관된 이전 세션이 없습니다.</p>}
      <ul>{history.items.map((item,index)=><li key={item.id}>이전 세션 {index+1} · {item.issue?issueMessage(item.issue):`파일 ${item.documents}개 · 최근 파일 ${item.recentFiles}개`}
        <button disabled={busy||disabled||!!preview||!!item.issue} aria-label={`이전 세션 ${index+1} 복원 검토`} onClick={()=>void act(async()=>{
          const next=await call<Preview>("preview_session_restore",{backupId:item.id});
          if(!alive.current){await call("cancel_session_import",{previewId:next.previewId});return;}
          pending.current=next.previewId;setPreview(next);setReplace(false);setResult(null);
        })}>복원 검토</button></li>)}</ul>
      {history.unrecognized>0&&<p>확인할 수 없는 항목 {history.unrecognized}개를 그대로 유지했습니다.</p>}
    </section>}
    {!preview&&<button disabled={busy||disabled} onClick={()=>void act(async()=>{
      const job=await componentCall<{id:string;source:string;phase:string}|null>(description,"workspace.migration","legacy_snapshot_job",{},"overview");
      if(!job||job.source!=="code-pad"||job.phase!=="ready")throw new Error("Overview에서 Code Pad 보관본의 내용을 먼저 확인해 주세요.");
      const next=await call<Preview>("preview_session_import",{jobId:job.id});
      if(!alive.current){await call("cancel_session_import",{previewId:next.previewId});return;}
      pending.current=next.previewId;setPreview(next);setReplace(false);setResult(null);
    })}>세션 가져오기 검토</button>}
    {preview&&<>
      <p>복원할 파일 {preview.candidate.session.docs.length}개 · 최근 파일 {preview.candidate.session.recent_files.length}개</p>
      <ul>{preview.candidate.session.docs.map(doc=><li key={doc.id}>{doc.path} · 북마크 {doc.bookmarks.length}개</li>)}</ul>
      {(preview.candidate.skippedDocuments>0||preview.candidate.skippedRecentFiles>0)&&<p>현재 선택으로 복원하지 않는 파일 {preview.candidate.skippedDocuments}개 · 최근 파일 {preview.candidate.skippedRecentFiles}개는 원본 보관본에 남아 있습니다.</p>}
      {preview.alreadyImported?<p>이 항목은 이미 가져왔습니다. 이후 변경한 세션을 유지합니다.</p>:preview.conflict&&<label><input type="checkbox" checked={replace} disabled={busy} onChange={event=>setReplace(event.target.checked)}/>현재 세션(파일 {preview.currentDocuments}개 · 최근 파일 {preview.currentRecentFiles}개)을 별도로 보관하고 검토한 내용으로 바꿉니다.</label>}
      <button disabled={busy||disabled||(!preview.alreadyImported&&preview.conflict&&!replace)} onClick={()=>void act(async()=>{
        const id=preview.previewId;pending.current=null;
        try{const applied=await call<Applied>("apply_session_import",{previewId:id,replaceExisting:replace});if(alive.current){setResult(applied);if(!applied.reused){setHistory(null);onApplied();}}}
        finally{if(alive.current)setPreview(null);}
      })}>{preview.alreadyImported?"가져오기 상태 확인":preview.restoring?"검토한 이전 세션 복원":"검토한 세션 가져오기"}</button>
      <button disabled={busy} onClick={()=>void act(async()=>{await call("cancel_session_import",{previewId:preview.previewId});pending.current=null;if(alive.current)setPreview(null);})}>세션 가져오기 취소</button>
    </>}
    {result&&<p role="status">{result.reused?"이미 가져온 항목입니다. 현재 세션을 유지했습니다.":`파일 ${result.importedDocuments}개 · 최근 파일 ${result.importedRecentFiles}개를 ${result.restored?"복원했습니다":"가져왔습니다"}.`}</p>}
  </section>;
}
