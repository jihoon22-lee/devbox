import {useCallback, useEffect, useRef, useState} from "react";
import type {Description} from "@devbox/product-shell/api";
import {componentCall} from "./native";
type Summary={jobs:number;services:number;tasks:number;runs:number;activeHistory:number;secretsRequiringReview:number;logBytes:number;missingLogs:number;orphanLogDirectories:number};
type Job={id:string;phase:string;summary:Summary|null;issue:string|null;alreadyImported:boolean};
const activePhase=(job:Job|null)=>!!job && ["prepare","resume","apply"].includes(job.phase);
const issues:Record<string,string>={
  runtime_import_snapshot_failed:"기존 Run Manager 데이터베이스의 일관된 사본을 만들지 못했습니다. 기존 앱을 종료하고 다시 시도해 주세요.",
  runtime_import_source_changed:"로그가 읽는 동안 변경되었습니다. 기존 앱을 종료하고 다시 가져와 주세요.",
  runtime_import_destination_conflict:"이미 가져온 데이터가 있거나 현재 데이터와 ID가 충돌합니다. 현재 내용은 덮어쓰지 않았습니다.",
  runtime_import_schema_unsupported:"지원되지 않는 데이터 버전입니다. 원본은 보존되어 있습니다.",
  runtime_import_limit:"가져오기 용량 한도를 초과했습니다. 원본은 보존되어 있습니다.",
  runtime_import_cancelled:"가져오기를 취소했습니다. 보존된 원본 사본은 남아 있습니다.",
};
export default function RuntimeImport({description,active,blocked}:{description:Description;active:boolean;blocked:boolean}) {
  const [expanded,setExpanded]=useState(false);const [job,setJob]=useState<Job|null>(null);
  const [catalog,setCatalog]=useState<string[]>([]);const [error,setError]=useState("");const [pending,setPending]=useState(false);
  const latest=useRef(description);latest.current=description;
  const call=useCallback(<T,>(method:string,args:Record<string,unknown>={})=>componentCall<T>(latest.current,"workspace.runtime",method,args,"tasks"),[]);
  const refresh=useCallback(async()=>{setJob(await call<Job|null>("runtime_import_status"));setCatalog(await call<string[]>("runtime_import_catalog"));},[call]);
  useEffect(()=>{
    if(!active||!expanded)return;let disposed=false;let timer:ReturnType<typeof setTimeout>|undefined;
    const tick=async()=>{try {const value=await call<Job|null>("runtime_import_status");if(disposed)return;setJob(value);if(activePhase(value))timer=setTimeout(()=>void tick(),1000);}catch{if(!disposed)setError("가져오기 상태를 확인하지 못했습니다.");}};
    void tick();void call<string[]>("runtime_import_catalog").then(value=>{if(!disposed)setCatalog(value);}).catch(()=>{});
    return()=>{disposed=true;clearTimeout(timer);};
  },[active,expanded,call,pending]);
  const invoke=async(method:string,id?:string)=>{setPending(true);setError("");try{await call(method,id?{id}:{});await refresh();}catch(cause){setError(cause instanceof Error?cause.message:"가져오기 요청을 처리하지 못했습니다.");}finally{setPending(false);}};
  return <section aria-label="기존 실행 데이터 가져오기">
    <button type="button" aria-expanded={expanded} onClick={()=>setExpanded(value=>!value)}>기존 Run Manager 데이터 가져오기</button>
    {expanded&&<div>
      <p>작업·서비스·실행 이력과 로그를 보존한 뒤 비활성 상태로 가져옵니다. 원래 활성화 설정은 기록하며, 실행 중인 프로세스를 인계받지 않습니다. 가져온 작업의 실행 신뢰와 비밀 환경 변수는 다시 검토해야 합니다.</p>
      <button type="button" disabled={blocked||pending||activePhase(job)} onClick={()=>void invoke("runtime_import_prepare")}>기존 데이터 사본 만들기</button>
      {activePhase(job)&&<><p role="status">{job?.phase==="apply"?"비활성 데이터 저장 중…":"데이터와 로그 보존 중…"}</p><button type="button" disabled={pending} onClick={()=>void invoke("runtime_import_cancel")}>취소</button></>}
      {job?.summary&&<p>작업 {job.summary.jobs} · 서비스 {job.summary.services} · 프로젝트 작업 {job.summary.tasks} · 실행 이력 {job.summary.runs} · 로그 {(job.summary.logBytes/1024/1024).toFixed(1)} MiB<br/>이전 실행 상태 검토 {job.summary.activeHistory} · 비밀 재설정 {job.summary.secretsRequiringReview} · 누락된 로그 {job.summary.missingLogs} · 연결 없는 로그 폴더 {job.summary.orphanLogDirectories}</p>}
      {job?.phase==="ready"&&<button type="button" disabled={blocked||pending} onClick={()=>void invoke("runtime_import_apply",job.id)}>검토한 데이터를 비활성 상태로 가져오기</button>}
      {job?.phase==="complete"&&<p role="status">{job.alreadyImported?"이미 가져온 사본입니다. 이후 수정한 데이터는 보존했습니다.":"가져왔습니다. 작업 목록을 새로고침해 확인해 주세요."} 비밀이 있던 작업은 편집에서 환경 변수를 교체하거나 명시적으로 지워야 실행할 수 있습니다.</p>}
      {(error||job?.issue)&&<p role="alert">{error||(job?.issue&&(issues[job.issue]??"가져오기를 완료하지 못했습니다. 원본은 보존되어 있습니다."))}</p>}
      {!!catalog.length&&<details><summary>보존된 사본 다시 검토</summary><ul>{catalog.map(id=><li key={id}><code>{id.slice(0,12)}</code>{" "}<button type="button" disabled={pending||blocked||activePhase(job)} onClick={()=>void invoke("runtime_import_resume",id)}>다시 확인</button></li>)}</ul></details>}
    </div>}
  </section>;
}
