import {useEffect,useRef,useState} from "react";
import type {Description} from "@devbox/product-shell/api";
import {componentCall} from "./native";
interface Status {available:{id:string;root:string}[];hasApproval:boolean;selectedIds:string[]}
interface Preview {previewId:string;members:{id:string;root:string;review:{executionKeys:string[];sources:{path:string;digest:string|null}[]}}[]}
interface Props {description:Description;enabled:boolean;blocked?:boolean;onBusyChange:(busy:boolean)=>void;onDirtyChange:(dirty:boolean)=>void;onChange:()=>void}
export default function SourceCleanupScope({description,enabled,blocked=false,onBusyChange,onDirtyChange,onChange}:Props) {
  const [status,setStatus]=useState<Status|null>(null);const [selected,setSelected]=useState<string[]>([]);
  const [preview,setPreview]=useState<Preview|null>(null);const [busy,setBusy]=useState(false);
  const [error,setError]=useState("");const [notice,setNotice]=useState("");
  const alive=useRef(true);const busyRef=useRef(false);const pending=useRef<string|null>(null);
  const call=<T,>(method:string,args:Record<string,unknown>={})=>componentCall<T>(description,"workspace.source",method,args,"source");
  const dirty=status!==null&&(selected.length!==status.selectedIds.length||selected.some(id=>!status.selectedIds.includes(id)));
  useEffect(()=>{onBusyChange(busy||preview!==null);},[busy,preview,onBusyChange]);
  useEffect(()=>{onDirtyChange(dirty);},[dirty,onDirtyChange]);
  useEffect(()=>{
    alive.current=true;
    return ()=>{
      alive.current=false;
      if(pending.current)void call("cancel_cleanup_scope",{previewId:pending.current}).catch(()=>{});
      onBusyChange(false);onDirtyChange(false);
    };
    // Parent retains this surface across routes and keys it by native context.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  },[]);
  async function load() {
    const next=await call<Status>("cleanup_scope_status");
    if(alive.current){setStatus(next);setSelected(next.selectedIds);}
  }
  async function act(action:()=>Promise<void>) {
    if(busyRef.current)return;
    busyRef.current=true;setBusy(true);setError("");setNotice("");
    try {await action();}
    catch(cause){if(alive.current)setError(cause instanceof Error?cause.message:"정리 범위를 확인하지 못했습니다.");}
    finally {if(alive.current){busyRef.current=false;setBusy(false);}}
  }
  return <section className="workspace-source-cleanup-scope" aria-label="다른 작업 폴더 정리 범위" aria-busy={busy}>
    <h2>다른 작업 폴더 정리 범위</h2>
    <p>같은 저장소에 등록된 작업 폴더를 선택해 Git 실행 근거를 검토합니다. 범위 승인은 폴더를 삭제하지 않습니다. 아래 정리 후보 검사와 최종 확인에서 삭제할 대상을 선택합니다.</p>
    <p>열린 파일 탭이 있는 폴더는 정리할 수 없습니다. 연결할 수 없는 승인 폴더가 있으면 범위를 다시 선택하거나 철회할 수 있습니다.</p>
    {error&&<p role="alert">{error}</p>}{notice&&<p role="status">{notice}</p>}
    {!preview&&<>
      <button type="button" disabled={blocked||busy||dirty} onClick={()=>void act(load)}>정리 범위 확인</button>
      {status&&<>
        <p role="status">{status.hasApproval?"저장된 정리 범위가 있습니다. 실제 검사는 실행 직전에 다시 검증합니다.":"다른 작업 폴더의 정리 범위를 승인하지 않았습니다."}</p>
        <fieldset disabled={blocked||busy}><legend>검토할 등록 폴더 (최대 8개)</legend>
          {status.available.map(tree=><label key={tree.id}><input type="checkbox" checked={selected.includes(tree.id)} onChange={event=>setSelected(current=>event.target.checked?[...current,tree.id]:current.filter(id=>id!==tree.id))}/>{tree.root}</label>)}
          {status.available.length===0&&<p>같은 저장소에 등록된 다른 작업 폴더가 없습니다.</p>}
        </fieldset>
        {dirty&&<button type="button" disabled={blocked||busy} onClick={()=>setSelected(status.selectedIds)}>범위 선택 되돌리기</button>}
        <button type="button" disabled={blocked||!enabled||busy||selected.length===0||selected.length>8} onClick={()=>void act(async()=>{
          const next=await call<Preview>("preview_cleanup_scope",{worktreeIds:selected});
          if(!alive.current){void call("cancel_cleanup_scope",{previewId:next.previewId}).catch(()=>{});return;}
          pending.current=next.previewId;setPreview(next);
        })}>선택한 정리 범위 검토</button>
      </>}
      {(status?.hasApproval||error)&&<button type="button" disabled={blocked||busy} onClick={()=>void act(async()=>{
        await call("revoke_cleanup_scope");if(!alive.current)return;
        onChange();await load();if(alive.current)setNotice("다른 작업 폴더의 정리 승인을 철회했습니다.");
      })}>정리 범위 승인 철회</button>}
    </>}
    {preview&&<section aria-label="정리 범위 승인 확인">
      <p>이 폴더들의 Git 설정과 hook으로 상태를 검사하고, 별도로 확인한 안전한 폴더만 정리할 수 있습니다. 설정·실행 파일·프로젝트 정의가 바뀌면 다시 검토합니다.</p>
      {preview.members.map(member=><div key={member.id}><h3>{member.root}</h3>
        <p>실행 관련 설정: {member.review.executionKeys.join(", ")||"없음"}</p>
        <details><summary>확인한 파일과 hook 경로</summary><ul>{member.review.sources.map((source,index)=><li key={index}><code>{source.path}</code>{source.digest&&<small>SHA-256 {source.digest}</small>}</li>)}</ul></details>
      </div>)}
      <button type="button" disabled={blocked||!enabled||busy} onClick={()=>void act(async()=>{
        const previewId=preview.previewId;pending.current=null;setPreview(null);
        await call("approve_cleanup_scope",{previewId});if(!alive.current)return;
        onChange();await load();if(alive.current)setNotice("검토한 정리 범위를 승인했습니다. 정리 후보를 새로 검사해 주세요.");
      })}>검토한 정리 범위 승인</button>
      <button type="button" disabled={blocked||busy} onClick={()=>void act(async()=>{
        await call("cancel_cleanup_scope",{previewId:preview.previewId});
        if(alive.current){pending.current=null;setPreview(null);}
      })}>정리 범위 검토 취소</button>
    </section>}
  </section>;
}
