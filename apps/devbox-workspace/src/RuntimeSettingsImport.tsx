import {useCallback,useEffect,useRef,useState} from "react";
import type {Description} from "@devbox/product-shell/api";
import {componentCall,nativeCall} from "./native";
type Job={id:string;source:string;phase:string;issue:string|null};
type Review={token:string;before:{favorites?:number;refreshIntervalMs?:number;views?:number};after:{favorites?:number;refreshIntervalMs?:number;views?:number};unavailableSources:number};
type Catalog={snapshots:{id:string;manifest:{source:string}|null;issue:string|null}[]};
const running=(job:Job|null)=>!!job&&["reading","preserving","checking"].includes(job.phase);
const migration=<T,>(method:string,args:Record<string,unknown>={})=>nativeCall<T>("workspace.migration",method,args);
export default function RuntimeSettingsImport({description,active,kind,onImported}:{description:Description;active:boolean;kind:"runtime"|"logs";onImported:()=>void}){
  const [expanded,setExpanded]=useState(false);const [job,setJob]=useState<Job|null>(null);const [catalog,setCatalog]=useState<Catalog["snapshots"]>([]);
  const [review,setReview]=useState<Review|null>(null);const [replace,setReplace]=useState(false);const [busy,setBusy]=useState(false);const [notice,setNotice]=useState("");
  const latest=useRef(description);latest.current=description;
  const source=kind==="runtime"?"port-manager":"log-lens";const component=kind==="runtime"?"workspace.processes":"workspace.logs";
  const call=useCallback(<T,>(method:string,args:Record<string,unknown>={})=>componentCall<T>(latest.current,component,method,args,kind),[component,kind]);
  useEffect(()=>{
    if(!expanded||!active)return;let disposed=false;let timer:ReturnType<typeof setTimeout>|undefined;
    const tick=async()=>{try{const current=await migration<Job|null>("legacy_snapshot_job");if(disposed)return;setJob(current);if(running(current))timer=setTimeout(()=>void tick(),1000);}catch{if(!disposed)setNotice("보관 상태를 확인하지 못했습니다.");}};
    void tick();void migration<Catalog>("list_legacy_snapshots").then(value=>{if(!disposed)setCatalog(value.snapshots.filter(entry=>entry.manifest?.source===source&&!entry.issue));}).catch(()=>{});
    return()=>{disposed=true;clearTimeout(timer);};
  },[expanded,active,busy,source]);
  const act=async(operation:()=>Promise<void>)=>{setBusy(true);setNotice("");try{await operation();}catch(cause){setNotice(cause instanceof Error?cause.message:"설정 가져오기를 완료하지 못했습니다.");}finally{setBusy(false);}};
  const prepare=()=>act(async()=>{setReview(null);setReplace(false);await migration("prepare_legacy_snapshot",{source});});
  const preview=()=>act(async()=>{setReplace(false);setReview(await call<Review>("preview_legacy_runtime_settings",{jobId:job?.id}));});
  const apply=()=>act(async()=>{const result=await call<{alreadyImported:boolean}>("apply_legacy_runtime_settings",{token:review?.token,replace});setReview(null);setReplace(false);setNotice(result.alreadyImported?"이미 가져온 사본입니다. 이후 편집은 보존했습니다.":"설정을 가져왔습니다. 저장된 로그 뷰는 직접 불러온 뒤 재연결해 주세요.");onImported();});
  return <section aria-label="기존 보기 설정 가져오기"><button type="button" aria-expanded={expanded} onClick={()=>setExpanded(value=>!value)}>{kind==="runtime"?"Port Manager 보기 설정 가져오기":"Log Lens 저장된 뷰 가져오기"}</button>
    {expanded&&<div><p>기존 설정 사본을 보존한 뒤 변경 내용을 검토합니다. 로그 뷰는 연결되지 않은 상태로 가져오며, 현재 설정을 바꾸려면 아래에서 교체를 선택해 주세요.</p>
      <button type="button" disabled={busy||running(job)} onClick={()=>void prepare()}>기존 설정 보관</button>
      {running(job)&&<><p role="status">설정을 보관하고 있습니다…</p><button type="button" disabled={busy} onClick={()=>void act(async()=>{await migration("cancel_legacy_snapshot",{jobId:job?.id});})}>취소</button></>}
      {job?.source===source&&job.phase==="ready"&&<button type="button" disabled={busy} onClick={()=>void preview()}>변경 내용 검토</button>}
      {review&&<div>
        {kind==="runtime"?<p>즐겨찾기 {review.before.favorites} → {review.after.favorites}개 · 새로고침 {(review.before.refreshIntervalMs??0)/1000} → {(review.after.refreshIntervalMs??0)/1000}초</p>:<p>저장된 뷰 {review.before.views} → {review.after.views}개 · 현재 연결할 수 없는 실행 로그 {review.unavailableSources}개</p>}
        <label><input type="checkbox" checked={replace} onChange={event=>setReplace(event.target.checked)}/>현재 설정 교체 허용 (이전 설정 사본 보존)</label>
        <button type="button" disabled={busy} onClick={()=>void apply()}>검토한 설정 가져오기</button>
      </div>}
      {!!catalog.length&&<details><summary>보존한 사본 다시 검토</summary><ul>{catalog.map(entry=><li key={entry.id}><code>{entry.id.slice(0,12)}</code>{" "}<button type="button" disabled={busy||running(job)} onClick={()=>void act(async()=>{setReview(null);await migration("verify_legacy_snapshot",{snapshotId:entry.id});})}>내용 다시 확인</button></li>)}</ul></details>}
      {notice&&<p role="status">{notice}</p>}
      {job?.source===source&&job.phase==="failed"&&<p role="alert">기존 설정을 보관하지 못했습니다. 원본을 확인하고 다시 시도해 주세요.</p>}
    </div>}
  </section>;
}
