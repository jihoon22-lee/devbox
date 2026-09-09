import {useEffect,useRef,useState} from "react";
import type {ProjectProfile} from "@devbox/workspace-features/overview-types";
import {nativeCall} from "./native";

export type ImportedProfile={id:string;sourceSnapshotId:string;sourceTemplateId?:string|null;profile:ProjectProfile};
export type ProfileBinding={importedId:string;target:"windows"|"wsl";worktreeId:string};
type Decision="skip"|"import"|"keep-both"|"reuse";
type Row={profile:ProjectProfile;disposition:"new"|"identical"|"conflict";existingIds:string[]};
type Preview={previewId:string;plan:{sourceSnapshotId:string;registryRevision:number;rows:Row[]}};
type Applied={added:number;reused:number;skipped:number;mappings:{sourceId:string;importedId:string|null}[]};
const call=<T,>(method:string,args:Record<string,unknown>)=>nativeCall<T>("workspace.migration",method,args);
const disposition={new:"새 프로필",identical:"같은 내용이 이미 있습니다",conflict:"이전 ID 또는 경로가 기존 항목과 겹칩니다"};
export function ProfileMetadata({profile}:{profile:ProjectProfile}) {
  return <dl>
    <dt>Windows 폴더</dt><dd>{profile.windowsPath??"없음"}</dd>
    <dt>WSL 폴더</dt><dd>{profile.wsl?`${profile.wsl.distro}: ${profile.wsl.path}`:"없음"}</dd>
    <dt>Git 폴더</dt><dd>{profile.gitRoot??"없음"}</dd>
    <dt>사용 포트</dt><dd>{profile.expectedPorts.join(", ")||"없음"}</dd>
    <dt>서비스 참조</dt><dd>{profile.runManagerServiceIds.join(", ")||"없음"}</dd>
    <dt>환경 설정</dt><dd>{profile.environment?<>
      <span>{profile.environment.source} · {profile.environment.enabled?"기존 활성 설정":"기존 비활성 설정"}</span>
      <ul>{profile.environment.variables.map(variable=><li key={variable.name}>{variable.name} · {variable.source}{variable.secretReference?` · 보관된 참조: ${variable.secretReference.name}`:""}{variable.conflict!=="none"?" · 충돌 확인 필요":""}</li>)}</ul>
    </>:"없음"}</dd>
  </dl>;
}
export default function LegacyProfileImport({jobId,existing,onImported,onBusyChange,disabled=false}:{jobId:string;existing:ImportedProfile[];onImported:()=>Promise<void>;onBusyChange:(busy:boolean)=>void;disabled?:boolean}) {
  const [preview,setPreview]=useState<Preview|null>(null),[choices,setChoices]=useState<Record<string,Decision>>({});
  const [busy,setBusy]=useState(false),[error,setError]=useState(""),[result,setResult]=useState<Applied|null>(null);
  const alive=useRef(false),pending=useRef<string|null>(null);
  useEffect(()=>{alive.current=true;return()=>{alive.current=false;if(pending.current)void call("cancel_profile_import",{previewId:pending.current}).catch(()=>{});onBusyChange(false);};},[onBusyChange]);
  async function act(action:()=>Promise<void>) {
    if(busy||disabled)return;
    setBusy(true);onBusyChange(true);setError("");
    try {await action();}catch(cause){if(alive.current)setError(cause instanceof Error?cause.message:"프로필 가져오기를 완료하지 못했습니다.");}
    finally {if(alive.current){setBusy(false);onBusyChange(false);}}
  }
  return <section aria-label="Workbench 프로필 가져오기" aria-busy={busy}>
    <h3>Workbench 프로필 가져오기</h3>
    <p>프로필을 보관한 뒤 프로젝트 폴더 연결을 별도로 확인합니다. 환경 파일과 서비스 참조는 연결 검토에 사용할 설정으로 보관합니다.</p>
    {error&&<p role="alert">{error}</p>}
    {!preview&&<button disabled={busy||disabled} onClick={()=>void act(async()=>{
      const next=await call<Preview>("preview_profile_import",{jobId});
      if(!alive.current){await call("cancel_profile_import",{previewId:next.previewId});return;}
      pending.current=next.previewId;setPreview(next);setChoices({});setResult(null);
    })}>프로필 가져오기 검토</button>}
    {preview&&<>
      <p>가져올 항목과 처리 방법을 선택하세요. 기존 항목은 유지됩니다.</p>
      {preview.plan.rows.length===0&&<p>가져올 프로필이 없습니다.</p>}
      {preview.plan.rows.map(row=><fieldset key={row.profile.id} disabled={busy||disabled}>
        <legend>{row.profile.name}</legend><p>{disposition[row.disposition]}</p>
        <ProfileMetadata profile={row.profile}/>
        {row.existingIds.map(id=>{const saved=existing.find(entry=>entry.id===id);return saved?<details key={id}><summary>기존 항목: {saved.profile.name}</summary><ProfileMetadata profile={saved.profile}/></details>:null;})}
        <label>처리 방법 <select aria-label={`${row.profile.name} 처리 방법`} value={choices[row.profile.id]??"skip"} onChange={event=>setChoices({...choices,[row.profile.id]:event.target.value as Decision})}>
          <option value="skip">건너뛰기</option>
          {row.disposition==="new"&&<option value="import">가져오기</option>}
          {row.disposition==="identical"&&<option value="reuse">기존 항목 사용</option>}
          {row.disposition==="conflict"&&<option value="keep-both">둘 다 보관</option>}
        </select></label>
      </fieldset>)}
      <button disabled={busy||disabled||!Object.values(choices).some(value=>value!=="skip")} onClick={()=>void act(async()=>{
        const id=preview.previewId;
        try {
          const applied=await call<{result:Applied}>("apply_profile_import",{previewId:id,choices:Object.entries(choices).map(([sourceId,decision])=>({sourceId,decision}))});
          if(alive.current)setResult(applied.result);
        } finally {
          pending.current=null;if(alive.current)setPreview(null);
          await call("cancel_profile_import",{previewId:id}).catch(()=>{});
          await onImported();
        }
      })}>선택한 프로필 가져오기</button>
      <button disabled={busy||disabled} onClick={()=>void act(async()=>{await call("cancel_profile_import",{previewId:preview.previewId});pending.current=null;if(alive.current)setPreview(null);})}>가져오기 검토 취소</button>
    </>}
    {result&&<p role="status">프로필 {result.added}개 추가, {result.reused}개 기존 항목 사용, {result.skipped}개 건너뜀. 프로젝트 목록에서 폴더 연결을 확인할 수 있습니다.</p>}
  </section>;
}
