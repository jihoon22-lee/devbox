import {useEffect,useRef,useState} from "react";
import type {Description} from "@devbox/product-shell/api";
import {componentCall,issueMessage} from "./native";
type Preview={previewId:string;config:Record<string,unknown>;currentConfig:Record<string,unknown>;conflict:boolean;alreadyImported:boolean;restoring:boolean};
type Applied={reused:boolean;restored:boolean};
type History={items:{id:string;languages:number|null;issue:string|null}[];unrecognized:number};
export default function LegacyLspImport({description,disabled,onBusyChange,onApplied}:{description:Description;disabled:boolean;onBusyChange:(busy:boolean)=>void;onApplied:()=>void}) {
  const [preview,setPreview]=useState<Preview|null>(null),[replace,setReplace]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState(""),[result,setResult]=useState<Applied|null>(null);
  const [history,setHistory]=useState<History|null>(null);
  const alive=useRef(false),pending=useRef<string|null>(null);
  const call=<T,>(method:string,args:Record<string,unknown>={})=>componentCall<T>(description,"workspace.lsp",method,args,"files");
  useEffect(()=>{alive.current=true;return()=>{alive.current=false;if(pending.current)void call("cancel_lsp_config_import",{previewId:pending.current}).catch(()=>{});onBusyChange(false);};},[]);
  async function act(action:()=>Promise<void>) {
    if(busy)return;
    setBusy(true);onBusyChange(true);setError("");
    try{await action();}catch(cause){if(alive.current)setError(cause instanceof Error?cause.message:"LSP 설정 가져오기를 완료하지 못했습니다.");}
    finally{if(alive.current){setBusy(false);onBusyChange(false);}}
  }
  async function acceptPreview(next:Preview) {
    if(!alive.current){await call("cancel_lsp_config_import",{previewId:next.previewId});return;}
    pending.current=next.previewId;setPreview(next);setReplace(false);setResult(null);
  }
  const blocked=disabled||!description.context;
  return <section aria-label="Code Pad LSP 설정 가져오기" aria-busy={busy}>
    <h2>Code Pad LSP 설정 가져오기</h2>
    <p>프로젝트를 선택하고 열린 탭을 모두 닫은 뒤 Overview에서 확인한 서버·runtime 설정을 가져옵니다. 현재 프로젝트 경로로 연결하며 LSP는 비활성 상태로 저장합니다. 실행하려면 LSP 관리에서 설정과 실행 승인을 다시 확인해 주세요.</p>
    {error&&<p role="alert">{error}</p>}
    <button disabled={busy||!!preview||!description.context} onClick={()=>void act(async()=>{const value=await call<History>("list_lsp_config_history");if(alive.current)setHistory(value);})}>이전 LSP 설정 목록</button>
    {history&&<section aria-label="보관된 이전 LSP 설정">
      {history.items.length===0&&<p>보관된 이전 LSP 설정이 없습니다.</p>}
      <ul>{history.items.map((item,index)=><li key={item.id}>이전 LSP 설정 {index+1} · {item.issue?issueMessage(item.issue):`언어 설정 ${item.languages}개`}
        <button disabled={busy||blocked||!!preview||!!item.issue} aria-label={`이전 LSP 설정 ${index+1} 복원 검토`} onClick={()=>void act(async()=>acceptPreview(await call<Preview>("preview_lsp_config_restore",{backupId:item.id})))}>복원 검토</button>
      </li>)}</ul>
      {history.unrecognized>0&&<p>확인할 수 없는 항목 {history.unrecognized}개를 그대로 유지했습니다.</p>}
    </section>}
    {!preview&&<button disabled={busy||blocked} onClick={()=>void act(async()=>{
      const job=await componentCall<{id:string;source:string;phase:string}|null>(description,"workspace.migration","legacy_snapshot_job",{},"overview");
      if(!job||job.source!=="code-pad"||job.phase!=="ready")throw new Error("Overview에서 Code Pad 보관본의 내용을 먼저 확인해 주세요.");
      await acceptPreview(await call<Preview>("preview_lsp_config_import",{jobId:job.id}));
    })}>LSP 설정 가져오기 검토</button>}
    {preview&&<>
      <details open><summary>현재 설정과 가져올 설정 확인</summary><p>현재 설정</p><pre>{JSON.stringify(preview.currentConfig,null,2)}</pre><p>가져올 설정 · LSP 비활성</p><pre>{JSON.stringify(preview.config,null,2)}</pre></details>
      {preview.alreadyImported?<p>이미 가져온 설정입니다. 이후 변경한 설정을 유지합니다.</p>:preview.conflict&&<label><input type="checkbox" checked={replace} disabled={busy} onChange={event=>setReplace(event.target.checked)}/>현재 LSP 설정을 별도로 보관하고 검토한 설정으로 바꿉니다.</label>}
      <button disabled={busy||blocked||(!preview.alreadyImported&&preview.conflict&&!replace)} onClick={()=>void act(async()=>{
        const id=preview.previewId;pending.current=null;
        try{const applied=await call<Applied>("apply_lsp_config_import",{previewId:id,replaceExisting:replace});if(alive.current){setResult(applied);if(!applied.reused){setHistory(null);onApplied();}}}
        finally{if(alive.current)setPreview(null);}
      })}>{preview.alreadyImported?"LSP 가져오기 상태 확인":preview.restoring?"검토한 이전 LSP 설정 복원":"검토한 LSP 설정 가져오기"}</button>
      <button disabled={busy} onClick={()=>void act(async()=>{await call("cancel_lsp_config_import",{previewId:preview.previewId});pending.current=null;if(alive.current)setPreview(null);})}>LSP 설정 가져오기 취소</button>
    </>}
    {result&&<p role="status">{result.reused?"이미 가져온 항목입니다. 현재 LSP 설정을 유지했습니다.":`LSP 설정을 ${result.restored?"복원했습니다":"가져왔습니다"}. 서버는 시작하지 않았습니다.`}</p>}
  </section>;
}
