import {useEffect,useRef,useState} from "react";
import type {ProjectProfile} from "@devbox/workspace-features/overview-types";
import {nativeCall} from "./native";
import {ProfileMetadata} from "./LegacyProfileImport";
export type ProfileTemplate=Omit<ProjectProfile,"environment">;
export type ImportedTemplate={id:string;sourceSnapshotId?:string|null;local?:boolean;archived?:boolean;template:ProfileTemplate};
type Decision="skip"|"import"|"keep-both"|"reuse";
type Row={template:ProfileTemplate;disposition:"new"|"identical"|"conflict";existingIds:string[];alreadyImported?:boolean};
type Preview={previewId:string;plan:{sourceSnapshotId:string;registryRevision:number;rows:Row[]}};
type Applied={added:number;reused:number;skipped:number};
const call=<T,>(method:string,args:Record<string,unknown>)=>nativeCall<T>("workspace.migration",method,args);
const disposition={new:"새 템플릿",identical:"같은 내용이 이미 있습니다",conflict:"이전 ID 또는 이름이 기존 항목과 겹칩니다"};
export function TemplateMetadata({template}:{template:ProfileTemplate}) {
  return <ProfileMetadata profile={{...template,environment:null}}/>;
}
export default function LegacyTemplateImport({jobId,existing,onImported,onBusyChange,disabled=false}:{jobId:string;existing:ImportedTemplate[];onImported:()=>Promise<void>;onBusyChange:(busy:boolean)=>void;disabled?:boolean}) {
  const [preview,setPreview]=useState<Preview|null>(null),[choices,setChoices]=useState<Record<string,Decision>>({});
  const [busy,setBusy]=useState(false),[error,setError]=useState(""),[result,setResult]=useState<Applied|null>(null);
  const alive=useRef(false),pending=useRef<string|null>(null);
  useEffect(()=>{alive.current=true;return()=>{alive.current=false;if(pending.current)void call("cancel_template_import",{previewId:pending.current}).catch(()=>{});onBusyChange(false);};},[onBusyChange]);
  async function act(action:()=>Promise<void>) {
    if(busy||disabled)return;
    setBusy(true);onBusyChange(true);setError("");
    try {await action();}catch(cause){if(alive.current)setError(cause instanceof Error?cause.message:"템플릿 가져오기를 완료하지 못했습니다.");}
    finally {if(alive.current){setBusy(false);onBusyChange(false);}}
  }
  return <section aria-label="Workbench 템플릿 가져오기" aria-busy={busy}>
    <h3>Workbench 템플릿 가져오기</h3>
    <p>템플릿의 기본값을 보관합니다. 프로젝트를 만들 때 실제 폴더와 적용할 설정을 확인할 수 있습니다.</p>
    {error&&<p role="alert">{error}</p>}
    {!preview&&<button disabled={busy||disabled} onClick={()=>void act(async()=>{
      const next=await call<Preview>("preview_template_import",{jobId});
      if(!alive.current){await call("cancel_template_import",{previewId:next.previewId});return;}
      pending.current=next.previewId;setPreview(next);setChoices({});setResult(null);
    })}>템플릿 가져오기 검토</button>}
    {preview&&<>
      <p>가져올 항목과 처리 방법을 선택하세요. 기존 항목은 유지됩니다.</p>
      {preview.plan.rows.length===0&&<p>가져올 템플릿이 없습니다.</p>}
      {preview.plan.rows.map(row=><fieldset key={row.template.id} disabled={busy||disabled}>
        <legend>{row.template.name}</legend><p>{row.alreadyImported?"이미 가져온 템플릿 · Workspace 변경 유지":disposition[row.disposition]}</p>
        <TemplateMetadata template={row.template}/>
        {row.existingIds.map(id=>{const saved=existing.find(entry=>entry.id===id);return saved?<details key={id}><summary>기존 항목: {saved.template.name}</summary><TemplateMetadata template={saved.template}/></details>:null;})}
        <label>처리 방법 <select aria-label={`${row.template.name} 처리 방법`} value={choices[row.template.id]??"skip"} onChange={event=>setChoices({...choices,[row.template.id]:event.target.value as Decision})}>
          <option value="skip">건너뛰기</option>
          {row.disposition==="new"&&<option value="import">가져오기</option>}
          {row.disposition==="identical"&&<option value="reuse">기존 항목 사용</option>}
          {row.disposition==="conflict"&&<option value="keep-both">둘 다 보관</option>}
        </select></label>
      </fieldset>)}
      <button disabled={busy||disabled||!Object.values(choices).some(value=>value!=="skip")} onClick={()=>void act(async()=>{
        const id=preview.previewId;
        try {
          const applied=await call<{result:Applied}>("apply_template_import",{previewId:id,choices:Object.entries(choices).map(([sourceId,decision])=>({sourceId,decision}))});
          if(alive.current)setResult(applied.result);
        } finally {
          pending.current=null;if(alive.current)setPreview(null);
          await call("cancel_template_import",{previewId:id}).catch(()=>{});
          await onImported();
        }
      })}>선택한 템플릿 가져오기</button>
      <button disabled={busy||disabled} onClick={()=>void act(async()=>{await call("cancel_template_import",{previewId:preview.previewId});pending.current=null;if(alive.current)setPreview(null);})}>가져오기 검토 취소</button>
    </>}
    {result&&<p role="status">템플릿 {result.added}개 추가, {result.reused}개 기존 항목 사용, {result.skipped}개 건너뜀. 프로젝트 관리에서 템플릿으로 프로젝트를 만들 수 있습니다.</p>}
  </section>;
}
