import {useEffect, useRef, useState} from "react";
import {nativeCall} from "./native";
import type {Preview} from "./RegistryGate";
import {TemplateMetadata,type ImportedTemplate} from "./LegacyTemplateImport";
import type {ImportedProfile} from "./LegacyProfileImport";

const distroName=(value:string)=>value.replace(/[A-Z]/g,letter=>letter.toLowerCase());
interface Distro {id:string;name:string;version:number;running:boolean}
export default function WslProjectForm({disabled,onBusyChange,onReviewed,templates=[],profile}:{disabled:boolean;onBusyChange:(busy:boolean)=>void;onReviewed:(preview:Preview,name:string)=>void;templates?:ImportedTemplate[];profile?:ImportedProfile}) {
  const [distros,setDistros]=useState<Distro[]>([]);
  const [id,setId]=useState("");
  const [root,setRoot]=useState(profile?.profile.wsl?.path??"");
  const [templateId,setTemplateId]=useState("");
  const template=templates.find(entry=>entry.id===templateId&&!entry.archived);
  const [start,setStart]=useState(false);
  const [busy,setBusy]=useState(true);
  const [error,setError]=useState("");
  const alive=useRef(true);
  const generation=useRef(0);
  const selected=distros.find(distro=>distro.id===id);
  function activity(value:boolean){setBusy(value);onBusyChange(value);}
  async function refresh(){
    const request=++generation.current;
    activity(true);setError("");setStart(false);
    try {
      const values=await nativeCall<Distro[]>("workspace.registry","list_wsl_distros");
      if(alive.current&&request===generation.current){setDistros(values);setId(current=>profile?values.find(distro=>distroName(distro.name)===distroName(profile.profile.wsl?.distro??""))?.id??"":values.some(distro=>distro.id===current)?current:"");}
    }catch(cause){if(alive.current&&request===generation.current)setError(cause instanceof Error?cause.message:"WSL 목록을 읽지 못했습니다.");}
    finally {if(alive.current&&request===generation.current)activity(false);}
  }
  useEffect(()=>{alive.current=true;void refresh();return()=>{alive.current=false;generation.current++;onBusyChange(false);};},[]);
  return <section aria-label="WSL 프로젝트 폴더" aria-busy={busy}>
    <p>{profile?`${profile.profile.name} 프로필에 보관된 WSL 폴더를 연결합니다.`:"등록된 배포판에서 Linux 폴더를 확인합니다."}</p>
    {profile&&!busy&&!selected&&<p role="status">프로필의 배포판 {profile.profile.wsl?.distro}을 찾을 수 없습니다. 보관된 프로필은 유지됩니다.</p>}
    {error&&<p role="alert">{error}</p>}
    <button disabled={disabled||busy} onClick={()=>void refresh()}>WSL 목록 새로 고침</button>
    {!busy&&!distros.length&&<p>등록된 WSL 배포판이 없습니다.</p>}
    <form onSubmit={event=>{event.preventDefault();if(disabled||busy||!selected||(!selected.running&&!start))return;activity(true);setError("");
      void (async()=>{
        try {
          if(templateId&&!template)throw new Error("선택한 템플릿이 변경되었습니다. 다시 선택하세요.");
          const startStopped=!selected.running&&start;
          const preview=profile
            ?await nativeCall<Preview>("workspace.registry","preview_imported_profile_wsl",{importedId:profile.id,distroId:id,startStopped})
            :template?await nativeCall<Preview>("workspace.registry","preview_template_profile_wsl",{templateId:template.id,distroId:id,root,name:template.template.name,startStopped})
            :await nativeCall<Preview>("workspace.registry","preview_wsl",{distroId:id,root,startStopped});
          if(!alive.current){await nativeCall("workspace.registry","cancel_registration",{previewId:preview.previewId});return;}
          onReviewed(preview,profile?.profile.name??template?.template.name??root.split("/").filter(Boolean).pop()??selected.name);
        }catch(cause){if(alive.current)setError(cause instanceof Error?cause.message:"WSL 폴더를 확인하지 못했습니다.");}
        finally{if(alive.current)activity(false);}
      })();
    }}>
      <label>WSL 배포판 <select value={id} disabled={disabled||busy||!!profile} required onChange={event=>{setId(event.target.value);setStart(false);}}>
        <option value="">배포판 선택</option>{distros.map(distro=><option key={distro.id} value={distro.id}>{distro.name} · {distro.running?"실행 중":"중지됨"}</option>)}
      </select></label>
      {!profile&&<><label>WSL 프로젝트 템플릿 <select value={templateId} disabled={disabled||busy} onChange={event=>setTemplateId(event.target.value)}>
        <option value="">템플릿 없이 등록</option>{templates.filter(entry=>!entry.archived).map(entry=><option key={entry.id} value={entry.id}>{entry.template.name}</option>)}
      </select></label>{template&&<TemplateMetadata template={template.template}/>}</>}
      <label>Linux 프로젝트 폴더 <input value={root} readOnly={!!profile} disabled={disabled||busy} maxLength={32768} required placeholder="/home/user/project" onChange={event=>setRoot(event.target.value)}/></label>
      {selected&&!selected.running&&<label><input type="checkbox" checked={start} disabled={disabled||busy} onChange={event=>setStart(event.target.checked)}/>선택한 배포판을 시작하고 폴더 확인</label>}
      <button disabled={disabled||busy||!selected||!root.startsWith("/")||root==="/"||(!selected.running&&!start)}>WSL 폴더 확인</button>
    </form>
  </section>;
}
