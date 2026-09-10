import {useEffect,useRef,useState} from "react";
import type {Description} from "@devbox/product-shell/api";
import {componentCall} from "./native";
interface Preview {previewId:string;branch:string;targetDir:string;root:string}
interface Props {description:Description;enabled:boolean;onBusyChange:(busy:boolean)=>void;onDirtyChange:(dirty:boolean)=>void;onPropose:(path:string)=>void}
export default function SourceWorktree({description,enabled,onBusyChange,onDirtyChange,onPropose}:Props) {
  const [branch,setBranch]=useState("");const [target,setTarget]=useState("");
  const [preview,setPreview]=useState<Preview|null>(null);const [created,setCreated]=useState<string|null>(null);
  const [busy,setBusy]=useState(false);const [error,setError]=useState("");const [notice,setNotice]=useState("");
  const busyRef=useRef(false);const alive=useRef(true);const pending=useRef<string|null>(null);const operation=useRef<string|null>(null);
  const call=<T,>(method:string,args:Record<string,unknown>)=>componentCall<T>(description,"workspace.source",method,args,"source");
  useEffect(()=>{onBusyChange(busy||preview!==null);},[busy,preview,onBusyChange]);
  useEffect(()=>{onDirtyChange(branch.length>0||target.length>0);},[branch,target,onDirtyChange]);
  useEffect(()=>{
    alive.current=true;
    return ()=>{
      alive.current=false;
      if(pending.current)void call("cancel_worktree",{previewId:pending.current}).catch(()=>{});
      if(operation.current)void call("repo_local_cancel",{request:{operationId:operation.current}}).catch(()=>{});
      onBusyChange(false);onDirtyChange(false);
    };
    // The parent keys this surface by native context and keeps it across routes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  },[]);
  async function act(action:()=>Promise<void>) {
    if(busyRef.current)return;
    busyRef.current=true;setBusy(true);setError("");setNotice("");
    try {await action();}
    catch(cause){if(alive.current)setError(cause instanceof Error?cause.message:"작업 폴더 생성을 완료하지 못했습니다.");}
    finally {if(alive.current){busyRef.current=false;setBusy(false);}}
  }
  return <section className="workspace-source-worktree" aria-label="새 Git 작업 폴더" aria-busy={busy}>
    <h2>새 작업 폴더</h2>
    <p>현재 HEAD에서 새 branch와 작업 폴더를 만듭니다. 생성 후 프로젝트 등록과 선택을 검토할 수 있습니다.</p>
    {error&&<p role="alert">{error} 생성 도중 실패하거나 취소했다면 저장소와 대상 폴더의 상태를 확인해 주세요.</p>}
    {notice&&<p role="status">{notice}</p>}
    <form onSubmit={event=>{event.preventDefault();if(!enabled)return;void act(async()=>{
      const next=await call<Preview>("preview_worktree",{branch,targetDir:target});
      if(!alive.current){void call("cancel_worktree",{previewId:next.previewId}).catch(()=>{});return;}
      pending.current=next.previewId;setPreview(next);
    });}}>
      <label htmlFor="source-worktree-branch">새 branch 이름</label>
      <input id="source-worktree-branch" value={branch} maxLength={1024} disabled={busy||preview!==null} onChange={event=>setBranch(event.target.value)} required/>
      <label htmlFor="source-worktree-target">생성할 폴더 경로</label>
      <input id="source-worktree-target" value={target} maxLength={4096} disabled={busy||preview!==null} onChange={event=>setTarget(event.target.value)} required/>
      <button disabled={!enabled||busy||preview!==null||!branch.trim()||!target.trim()}>작업 폴더 생성 검토</button>
      {(branch||target)&&!preview&&<button type="button" disabled={busy} onClick={()=>{setBranch("");setTarget("");setError("");}}>생성 입력 지우기</button>}
    </form>
    {preview&&<section aria-label="작업 폴더 생성 확인">
      <p>저장소: {preview.root}</p><p>새 branch: {preview.branch}</p><p>새 폴더: {preview.targetDir}</p>
      <p>확인한 Git 설정과 hook을 사용합니다. 현재 편집기 초안은 자동 저장하거나 commit하지 않습니다.</p>
      <button disabled={!enabled||busy} onClick={()=>void act(async()=>{
        const previewId=preview.previewId;pending.current=null;setPreview(null);
        const operationId=crypto.randomUUID();operation.current=operationId;
        try {
          const result=await call<{path:string}>("create_worktree",{previewId,operationId});
          if(alive.current){setCreated(result.path);setBranch("");setTarget("");setNotice("작업 폴더를 생성했습니다. 등록할 폴더를 검토해 주세요.");}
        } finally {operation.current=null;}
      })}>확인한 작업 폴더 생성</button>
      <button disabled={busy} onClick={()=>void act(async()=>{
        await call("cancel_worktree",{previewId:preview.previewId});
        if(alive.current){pending.current=null;setPreview(null);}
      })}>생성 검토 취소</button>
    </section>}
    {busy&&operation.current&&<button type="button" onClick={()=>{
      const operationId=operation.current;if(!operationId)return;
      setNotice("생성 취소를 요청했습니다. 결과를 확인하는 중입니다.");
      void call("repo_local_cancel",{request:{operationId}}).catch(()=>{if(alive.current)setNotice("취소 요청을 확인하지 못했습니다. 작업 결과를 기다리는 중입니다.");});
    }}>생성 취소 요청</button>}
    {created&&<div><p>{created}</p><button type="button" disabled={busy} onClick={()=>onPropose(created)}>생성한 폴더 등록 검토</button></div>}
  </section>;
}
