import {useEffect, useRef, useState} from "react";
import type {Description} from "@devbox/product-shell/api";
import DefinitionEditor from "./DefinitionEditor";
import {componentCall, nativeCall} from "./native";

interface View {
  registryRevision: number;
  editRevision: string;
  project: Record<string, unknown>;
  local: Record<string, unknown>;
  effective: {tasks:Record<string,{source:string;selector:string}>;toolchains:Record<string,{tool:string;version:string}>};
  sources: string[];
  unavailableSources: string[];
  definitionsTrusted: boolean;
  hasApproval: boolean;
}
interface Preview {previewId:string; definition:View}
interface Props {description:Description; onDirtyChange:(dirty:boolean)=>void; onChanged:()=>void}

export default function ProjectDefinitions({description,onDirtyChange,onChanged}:Props) {
  const [open,setOpen]=useState(false);
  const [view,setView]=useState<View|null>(null);
  const [preview,setPreview]=useState<Preview|null>(null);
  const [editing,setEditing]=useState(false);
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState("");
  const sequence=useRef(0);
  const pending=useRef<string|null>(null);
  const busyRef=useRef(false);
  const contextKey=JSON.stringify(description.context);
  const call=<T,>(method:string,args:Record<string,unknown>={})=>componentCall<T>(description,"workspace.definitions",method,args,"overview");
  useEffect(()=>{onDirtyChange(busy||!!preview||editing);},[busy,preview,editing,onDirtyChange]);
  useEffect(()=>()=>onDirtyChange(false),[onDirtyChange]);
  useEffect(()=>{
    setOpen(false);setView(null);setPreview(null);setError("");setBusy(false);busyRef.current=false;
    return ()=>{
      sequence.current+=1;
      if(pending.current) void nativeCall("workspace.definitions","cancel",{previewId:pending.current}).catch(()=>{});
      pending.current=null;
    };
  },[contextKey]);
  async function act(action:(request:number)=>Promise<void>) {
    if(busyRef.current)return;
    busyRef.current=true;setBusy(true);setError("");
    const request=++sequence.current;
    try {await action(request);}
    catch(cause){if(sequence.current===request)setError(cause instanceof Error?cause.message:"프로젝트 설정을 확인하지 못했습니다.");}
    finally{if(sequence.current===request){busyRef.current=false;setBusy(false);}}
  }
  async function load(request:number) {const value=await call<View>("load");if(sequence.current===request)setView(value);}
  async function cancel(request:number) {
    if(!preview)return;
    await call("cancel",{previewId:preview.previewId});if(sequence.current!==request)return;pending.current=null;setPreview(null);
  }
  return <section className="workspace-definitions" aria-label="프로젝트 설정" aria-busy={busy}>
    <button type="button" disabled={busy||editing||!!preview} onClick={()=>void act(async request=>{setOpen(true);await load(request);})}>프로젝트 설정</button>
    {open&&<>
      <h2>프로젝트 실행 정의</h2>
      {error&&<p role="alert">{error}</p>}
      {view&&<>
        <p role="status">{view.definitionsTrusted?"현재 실행 정의의 승인을 확인했습니다.":"현재 실행 정의를 검토해야 합니다."}</p>
        <p>승인은 현재 작업 폴더와 참조 파일 내용에 연결됩니다. 파일 내용이 바뀌면 다시 검토합니다.</p>
        <ul>{Object.entries(view.effective.tasks).map(([id,task])=><li key={id}>{id}: {task.source} · {task.selector}</li>)}</ul>
        {view.unavailableSources.length>0&&<p role="status">확인할 수 없는 실행 소스: {view.unavailableSources.join(", ")}</p>}
        <details><summary>공유 프로젝트 정의</summary><pre>{JSON.stringify(view.project,null,2)}</pre></details>
        <details><summary>이 컴퓨터의 설정</summary><pre>{JSON.stringify(view.local,null,2)}</pre></details>
        {!preview&&<DefinitionEditor key={contextKey} description={description} view={view} disabled={busy} onDirtyChange={setEditing} onSaved={()=>{onChanged();void act(load);}}/>}
        {!preview&&!editing&&<button type="button" disabled={busy||view.unavailableSources.length>0} onClick={()=>void act(async request=>{
          const next=await call<Preview>("preview_trust");
          if(sequence.current!==request){void nativeCall("workspace.definitions","cancel",{previewId:next.previewId}).catch(()=>{});return;}
          pending.current=next.previewId;setPreview(next);
        })}>실행 정의 검토</button>}
        {!preview&&!editing&&view.hasApproval&&<button type="button" disabled={busy} onClick={()=>void act(async request=>{await call("revoke_trust",{revision:view.registryRevision});if(sequence.current!==request)return;onChanged();await load(request);})}>실행 정의 승인 철회</button>}
      </>}
      {preview&&<section aria-label="실행 정의 승인 확인">
        <h3>이 실행 정의를 승인할까요?</h3>
        <p>아래 소스에서 선언한 작업과 도구 설정을 신뢰합니다. 승인 자체가 명령을 실행하거나 패키지를 설치하지는 않습니다.</p>
        <ul>{preview.definition.sources.map(source=><li key={source}>{source}</li>)}</ul>
        {preview.definition.sources.length===0&&<p>현재 참조하는 실행 소스가 없습니다.</p>}
        <button type="button" disabled={busy} onClick={()=>void act(async request=>{
          await call("approve_trust",{previewId:preview.previewId});if(sequence.current!==request)return;pending.current=null;setPreview(null);onChanged();await load(request);
        })}>검토한 실행 정의 승인</button>
        <button type="button" disabled={busy} onClick={()=>void act(cancel)}>승인 취소</button>
      </section>}
      {!preview&&!editing&&<button type="button" disabled={busy} onClick={()=>setOpen(false)}>설정 닫기</button>}
    </>}
  </section>;
}
