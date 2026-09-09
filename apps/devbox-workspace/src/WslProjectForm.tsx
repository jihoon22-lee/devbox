import {useEffect, useRef, useState} from "react";
import {nativeCall} from "./native";
import type {Preview} from "./RegistryGate";

interface Distro {id:string;name:string;version:number;running:boolean}
export default function WslProjectForm({disabled,onBusyChange,onReviewed}:{disabled:boolean;onBusyChange:(busy:boolean)=>void;onReviewed:(preview:Preview,name:string)=>void}) {
  const [distros,setDistros]=useState<Distro[]>([]);
  const [id,setId]=useState("");
  const [root,setRoot]=useState("");
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
      if(alive.current&&request===generation.current){setDistros(values);setId(current=>values.some(distro=>distro.id===current)?current:"");}
    }catch(cause){if(alive.current&&request===generation.current)setError(cause instanceof Error?cause.message:"WSL 목록을 읽지 못했습니다.");}
    finally {if(alive.current&&request===generation.current)activity(false);}
  }
  useEffect(()=>{alive.current=true;void refresh();return()=>{alive.current=false;generation.current++;onBusyChange(false);};},[]);
  return <section aria-label="WSL 프로젝트 폴더" aria-busy={busy}>
    <p>등록된 배포판에서 Linux 폴더를 확인합니다.</p>
    {error&&<p role="alert">{error}</p>}
    <button disabled={disabled||busy} onClick={()=>void refresh()}>WSL 목록 새로 고침</button>
    {!busy&&!distros.length&&<p>등록된 WSL 배포판이 없습니다.</p>}
    <form onSubmit={event=>{event.preventDefault();if(disabled||busy||!selected||(!selected.running&&!start))return;activity(true);setError("");
      void (async()=>{
        try {
          const preview=await nativeCall<Preview>("workspace.registry","preview_wsl",{distroId:id,root,startStopped:!selected.running&&start});
          if(!alive.current){await nativeCall("workspace.registry","cancel_registration",{previewId:preview.previewId});return;}
          onReviewed(preview,root.split("/").filter(Boolean).pop()??selected.name);
        }catch(cause){if(alive.current)setError(cause instanceof Error?cause.message:"WSL 폴더를 확인하지 못했습니다.");}
        finally{if(alive.current)activity(false);}
      })();
    }}>
      <label>WSL 배포판 <select value={id} disabled={disabled||busy} required onChange={event=>{setId(event.target.value);setStart(false);}}>
        <option value="">배포판 선택</option>{distros.map(distro=><option key={distro.id} value={distro.id}>{distro.name} · {distro.running?"실행 중":"중지됨"}</option>)}
      </select></label>
      <label>Linux 프로젝트 폴더 <input value={root} disabled={disabled||busy} maxLength={32768} required placeholder="/home/user/project" onChange={event=>setRoot(event.target.value)}/></label>
      {selected&&!selected.running&&<label><input type="checkbox" checked={start} disabled={disabled||busy} onChange={event=>setStart(event.target.checked)}/>선택한 배포판을 시작하고 폴더 확인</label>}
      <button disabled={disabled||busy||!selected||!root.startsWith("/")||root==="/"||(!selected.running&&!start)}>WSL 폴더 확인</button>
    </form>
  </section>;
}
