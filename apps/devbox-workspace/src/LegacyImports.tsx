import {useEffect,useRef,useState} from "react";
import {nativeCall,issueMessage} from "./native";
import LegacyProfileImport,{type ImportedProfile} from "./LegacyProfileImport";

type Source="workbench"|"code-pad"|"repo-manager";
type Entry={name:string;bytes:number;sha256:string;records:number|null;issue:"corrupt"|"unsupported-schema"|"limit"|null};
type Manifest={source:Source;files:Entry[];missing:string[]};
type Job={id:string;source:Source;operation:"preserve"|"verify";phase:"reading"|"preserving"|"checking"|"ready"|"cancelled"|"failed";snapshotId:string|null;manifest:Manifest|null;issue:string|null};
type Catalog={snapshots:{id:string;manifest:Manifest|null;issue:string|null}[];unrecognized:number};
const sources:Record<Source,string>={workbench:"Workbench","code-pad":"Code Pad","repo-manager":"Repo Manager"};
const labels:Record<string,string>={"project-profiles.json":"프로젝트 프로필","profile-templates.json":"프로필 템플릿","session.json":"열린 파일·커서·북마크","recovery.json":"미저장 복구 내용","lsp/config.json":"언어 서버 설정"};
const active=(job:Job|null)=>job?.phase==="reading"||job?.phase==="preserving"||job?.phase==="checking";
const call=<T,>(method:string,args:Record<string,unknown>={})=>nativeCall<T>("workspace.migration",method,args);

export default function LegacyImports({selected=false,existingProfiles=[],onImported=async()=>{}}:{selected?:boolean;existingProfiles?:ImportedProfile[];onImported?:()=>Promise<void>}) {
  const [source,setSource]=useState<Source>("workbench");
  const [job,setJob]=useState<Job|null>(null);
  const [catalog,setCatalog]=useState<Catalog>({snapshots:[],unrecognized:0});
  const [catalogError,setCatalogError]=useState("");
  const [catalogRevision,setCatalogRevision]=useState(0);
  const [busy,setBusy]=useState(false);
  const [profileBusy,setProfileBusy]=useState(false);
  const [error,setError]=useState("");
  const alive=useRef(false),epoch=useRef(0);
  useEffect(()=>{
    alive.current=true;const current=epoch.current;
    void call<Job|null>("legacy_snapshot_job").then(value=>{if(alive.current&&epoch.current===current)setJob(value);}).catch(cause=>{if(alive.current&&epoch.current===current)setError(cause instanceof Error?cause.message:"기존 설정 보관 상태를 확인하지 못했습니다.");});
    return()=>{alive.current=false;epoch.current++;};
  },[]);
  useEffect(()=>{
    if(active(job))return;
    let disposed=false;
    void call<Catalog>("list_legacy_snapshots").then(value=>{
      if(!disposed){setCatalog(value);setCatalogError("");}
    }).catch(cause=>{if(!disposed)setCatalogError(cause instanceof Error?cause.message:"보관 목록을 확인하지 못했습니다.");});
    return()=>{disposed=true;};
  },[job?.id,job?.phase,catalogRevision]);
  useEffect(()=>{
    if(!active(job))return;
    const current=epoch.current;
    const timer=setTimeout(()=>{
      void call<Job|null>("legacy_snapshot_job").then(value=>{if(alive.current&&epoch.current===current)setJob(value);}).catch(cause=>{if(alive.current&&epoch.current===current)setError(cause instanceof Error?cause.message:"진행 상태를 확인하지 못했습니다.");});
    },400);
    return()=>clearTimeout(timer);
  },[job]);
  async function act(method:string,args:Record<string,unknown>) {
    if(busy||profileBusy)return;
    setBusy(true);setError("");const current=++epoch.current;
    try {
      await call(method,args);
      const value=await call<Job|null>("legacy_snapshot_job");
      if(alive.current&&epoch.current===current){setJob(value);setCatalogRevision(value=>value+1);}
    } catch(cause) {if(alive.current&&epoch.current===current)setError(cause instanceof Error?cause.message:"기존 설정을 보관하지 못했습니다.");}
    finally {if(alive.current&&epoch.current===current)setBusy(false);}
  }
  return <section aria-label="기존 설정 보관" aria-busy={busy||profileBusy||active(job)}>
    <h2>기존 설정 보관</h2>
    <p>기존 앱의 설정과 미저장 복구 내용을 읽어 별도로 보관합니다. 원본 파일은 유지됩니다.</p>
    <label>기존 앱 <select value={source} disabled={busy||profileBusy||active(job)} onChange={event=>setSource(event.target.value as Source)}>{Object.entries(sources).map(([key,label])=><option key={key} value={key}>{label}</option>)}</select></label>
    <button disabled={busy||profileBusy||active(job)} onClick={()=>void act("prepare_legacy_snapshot",{source})}>설정 확인 및 보관</button>
    <button disabled={busy||profileBusy} onClick={()=>void act("legacy_snapshot_job",{})}>보관 상태 새로 고침</button>
    {error&&<p role="alert">{error}</p>}
    {catalogError&&<p role="alert">{catalogError}</p>}
    {catalog.snapshots.length>0&&<section aria-label="이전 보관 기록">
      <h3>이전 보관 기록</h3>
      <p>보관 내용을 다시 확인하면 모든 파일을 재검증합니다.</p>
      <ul>{catalog.snapshots.map((entry,index)=><li key={entry.id}>
        {entry.manifest?`${sources[entry.manifest.source]} · ${entry.manifest.files.length}개 설정 파일`:`내용 확인 필요 · 보관 ${index+1}`}
        {entry.issue&&<span> — {issueMessage(entry.issue)}</span>}
        <button disabled={busy||profileBusy||active(job)||!!entry.issue||!entry.manifest} aria-label={`보관 ${index+1} 내용 다시 확인`} onClick={()=>void act("verify_legacy_snapshot",{snapshotId:entry.id})}>내용 다시 확인</button>
      </li>)}</ul>
    </section>}
    {catalog.unrecognized>0&&<p>확인할 수 없는 보관 항목 {catalog.unrecognized}개를 그대로 유지했습니다.</p>}
    {job&&<>
      <h3>{sources[job.source]}</h3>
      {active(job)&&<><p role="status">{job.phase==="reading"?"기존 설정을 확인하고 있습니다…":job.phase==="checking"?"보관 파일을 다시 확인하고 있습니다…":"확인한 설정을 보관하고 있습니다…"}</p><button disabled={busy} onClick={()=>void act("cancel_legacy_snapshot",{jobId:job.id})}>보관 취소</button></>}
      {job.phase==="cancelled"&&<p role="status">보관을 취소했습니다. 다시 시도하면 일치하는 보관 파일부터 이어서 확인합니다.</p>}
      {job.phase==="failed"&&<p role="alert">{issueMessage(job.issue??"operation_failed")}</p>}
      {job.phase==="ready"&&job.manifest&&<>
        <p role="status">{job.operation==="verify"?"보관 파일 확인이 완료되었습니다.":"설정 보관이 완료되었습니다."}</p>
        {job.source==="repo-manager"&&<p>Repo Manager의 스캔 폴더와 선택 상태는 앱을 닫으면 사라지는 항목으로, 이전할 저장 설정이 없습니다.</p>}
        {job.manifest.files.length>0&&<table><thead><tr><th>항목</th><th>확인 결과</th></tr></thead><tbody>{job.manifest.files.map(entry=><tr key={entry.name}><td>{labels[entry.name]??entry.name}</td><td>{entry.issue==="unsupported-schema"?"지원하지 않는 형식 — 원본 보관":entry.issue==="corrupt"?"내용 확인 필요 — 원본 보관":entry.issue==="limit"?"크기 제한 초과":`${entry.records??0}개 항목`}</td></tr>)}</tbody></table>}
        {job.manifest.missing.length>0&&<p>저장 파일 없음: {job.manifest.missing.map(name=>labels[name]??name).join(", ")}</p>}
        {job.source==="workbench"&&job.manifest.files.some(file=>file.name==="project-profiles.json"&&!file.issue)&&(selected?<LegacyProfileImport key={job.id} jobId={job.id} existing={existingProfiles} onImported={onImported} onBusyChange={setProfileBusy}/>:<p>Workspace를 시작한 뒤 보관한 프로필 가져오기를 검토할 수 있습니다.</p>)}
      </>}
    </>}
  </section>;
}
