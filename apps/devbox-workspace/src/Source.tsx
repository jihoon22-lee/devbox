import {useEffect, useRef, useState} from "react";
import type {Description} from "@devbox/product-shell/api";
import SourcePanel from "@devbox/workspace-features/source-panel";
import {componentCall} from "./native";
import SourceWorktree from "./SourceWorktree";
import SourceCleanupScope from "./SourceCleanupScope";

interface Status {
  approved:boolean;
  hasApproval:boolean;
  review:{executable:string;sources:{path:string;kind:string;digest:string|null}[];executionKeys:string[];environmentKeys:string[]};
}
interface Preview {previewId:string;status:Status}
interface Props {description:Description; root:string; editorPending?:boolean; onBusyChange:(busy:boolean)=>void; onDirtyChange:(dirty:boolean)=>void; onOpenFile?:(path:string,line:number|null)=>void; onProposeWorktree?:(path:string)=>void}
export default function Source({description,root,editorPending=false,onBusyChange,onDirtyChange,onOpenFile,onProposeWorktree}:Props) {
  const [status,setStatus]=useState<Status|null>(null);
  const [preview,setPreview]=useState<Preview|null>(null);
  // Block clicks from the first committed frame until initial inspection finishes.
  const [busy,setBusy]=useState(true);
  const [panelsBusy,setPanelsBusy]=useState(false);
  const [panelsVisited,setPanelsVisited]=useState(false);
  const [dirty,setDirty]=useState(false);
  const [worktreeBusy,setWorktreeBusy]=useState(false);
  const [worktreeDirty,setWorktreeDirty]=useState(false);
  const [cleanupBusy,setCleanupBusy]=useState(false);
  const [cleanupDirty,setCleanupDirty]=useState(false);
  const [cleanupRevision,setCleanupRevision]=useState(0);
  const [error,setError]=useState("");
  const [notice,setNotice]=useState("");
  const sequence=useRef(0);
  const busyRef=useRef(false);
  const pending=useRef<string|null>(null);
  const contextKey=JSON.stringify(description.context);
  const call=<T,>(method:string,args:Record<string,unknown>={})=>componentCall<T>(description,"workspace.source",method,args,"source");
  useEffect(()=>{onBusyChange(busy||panelsBusy||worktreeBusy||cleanupBusy||preview!==null);},[busy,panelsBusy,worktreeBusy,cleanupBusy,preview,onBusyChange]);
  useEffect(()=>{onDirtyChange(dirty||worktreeDirty||cleanupDirty);},[dirty,worktreeDirty,cleanupDirty,onDirtyChange]);
  useEffect(()=>()=>{onBusyChange(false);onDirtyChange(false);},[onBusyChange,onDirtyChange]);
  async function load(request:number) {
    const next=await call<Status>("trust_status");
    if(sequence.current!==request)return;
    setStatus(next);if(next.approved)setPanelsVisited(true);
  }
  async function act(action:(request:number)=>Promise<void>) {
    if(busyRef.current||panelsBusy||worktreeBusy||cleanupBusy)return;
    busyRef.current=true;setBusy(true);setError("");setNotice("");
    const request=++sequence.current;
    try {await action(request);}
    catch(cause){if(sequence.current===request){setError(cause instanceof Error?cause.message:"Source 상태를 확인하지 못했습니다.");setStatus(previous=>previous?{...previous,approved:false}:null);}}
    finally{if(sequence.current===request){busyRef.current=false;setBusy(false);}}
  }
  useEffect(()=>{
    busyRef.current=false;
    void act(load);
    return ()=>{
      sequence.current+=1;
      if(pending.current)void call("cancel_trust",{previewId:pending.current}).catch(()=>{});
      pending.current=null;
    };
    // The parent keys this component by the native context; route changes retain it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  },[contextKey]);
  return <div className="workspace-native-source">
    <section className="workspace-source-trust" aria-label="Git 실행 승인" aria-busy={busy}>
      <h1>Source</h1><p>{root}</p>
      {editorPending&&<p role="status">편집기에 저장 전 변경 또는 진행 중인 작업이 있습니다. Git은 디스크와 index의 상태를 사용하며, 편집기 초안은 자동으로 저장하거나 stage하지 않습니다.</p>}
      <p role="status">{status?.approved?"현재 Git 실행 근거의 승인을 확인했습니다.":"Git 명령을 실행하기 전에 설정과 실행 파일을 검토해 주세요."}</p>
      {error&&<p role="alert">{error}</p>}
      {notice&&<p role="status">{notice}</p>}
      {!preview&&<>
        <button type="button" disabled={busy||panelsBusy||worktreeBusy||cleanupBusy} onClick={()=>void act(load)}>Git 승인 상태 확인</button>
        <button type="button" disabled={busy||panelsBusy||worktreeBusy||cleanupBusy} onClick={()=>void act(async request=>{
          const next=await call<Preview>("preview_trust");
          if(sequence.current!==request){void call("cancel_trust",{previewId:next.previewId}).catch(()=>{});return;}
          pending.current=next.previewId;setPreview(next);
        })}>Git 실행 검토</button>
        {(status?.hasApproval||error)&&<button type="button" disabled={busy||panelsBusy||worktreeBusy||cleanupBusy} onClick={()=>void act(async request=>{await call("revoke_trust");if(sequence.current===request){setStatus(previous=>previous?{...previous,approved:false,hasApproval:false}:null);setNotice("Git 실행 승인을 철회했습니다.");}})}>Git 실행 승인 철회</button>}
      </>}
      {preview&&<section aria-label="Git 실행 승인 확인">
        <h2>이 Git 실행 근거를 신뢰할까요?</h2>
        <p>Git 설정과 hook은 프로그램을 실행할 수 있습니다. 승인은 현재 프로젝트 정의·Git 설정·hook·실행 파일에 연결되며, 내용이나 파일이 바뀌면 다시 검토합니다. 승인 자체는 Git 명령이나 패키지 설치를 실행하지 않습니다.</p>
        <p>Git 실행 파일: {preview.status.review.executable}</p>
        <p>실행 관련 설정: {preview.status.review.executionKeys.join(", ")||"없음"}</p>
        <p>확인한 환경 변수 이름: {preview.status.review.environmentKeys.join(", ")||"없음"}. 값과 자격 증명은 표시하지 않습니다.</p>
        <details><summary>확인한 파일과 hook 경로 {preview.status.review.sources.length}개</summary><ul>{preview.status.review.sources.map((source,index)=><li key={`${source.kind}:${source.path}:${index}`}><code>{source.path}</code>{source.digest&&<small>SHA-256 {source.digest}</small>}</li>)}</ul></details>
        <button type="button" disabled={busy} onClick={()=>void act(async request=>{
          await call("approve_trust",{previewId:preview.previewId});
          if(sequence.current!==request)return;
          pending.current=null;setPreview(null);await load(request);
        })}>검토한 Git 실행 승인</button>
        <button type="button" disabled={busy} onClick={()=>void act(async request=>{
          await call("cancel_trust",{previewId:preview.previewId});
          if(sequence.current!==request)return;
          pending.current=null;setPreview(null);
        })}>Git 승인 검토 취소</button>
      </section>}
      {!status?.approved&&dirty&&!panelsBusy&&<button type="button" disabled={busy} onClick={()=>{setPanelsVisited(false);setDirty(false);}}>작성 중인 커밋 초안 버리기</button>}
    </section>
    {onProposeWorktree&&<SourceWorktree description={description} enabled={!!status?.approved&&!busy&&!panelsBusy&&!cleanupBusy&&preview===null} onBusyChange={setWorktreeBusy} onDirtyChange={setWorktreeDirty} onPropose={onProposeWorktree}/>}
    <SourceCleanupScope description={description} blocked={busy||panelsBusy||worktreeBusy||preview!==null} enabled={!!status?.approved&&!busy&&!panelsBusy&&!worktreeBusy&&preview===null} onBusyChange={setCleanupBusy} onDirtyChange={setCleanupDirty} onChange={()=>setCleanupRevision(value=>value+1)}/>
    {panelsVisited&&<fieldset className="workspace-source-actions" aria-label="Git 작업" disabled={!status?.approved||busy||worktreeBusy||cleanupBusy||preview!==null}>
      <SourcePanel cleanupRevision={cleanupRevision} repo={{path:root,canonicalKey:contextKey,hasWorktrees:false}} onBusyChange={setPanelsBusy} onDirtyChange={setDirty} onOpenFile={onOpenFile} onProposeWorktree={onProposeWorktree}/>
    </fieldset>}
  </div>;
}
