import { useCallback, useEffect, useRef, useState } from "react";
import type { Description } from "@devbox/product-shell/api";
import type { RuntimeLogOpenRequest } from "@devbox/workspace-features/logs";
import { componentCall } from "./native";
import { sameRuntimeContext, type RuntimeFocusTarget } from "./runtimeNavigation";
import "./Problems.css";

type FileTarget={kind:"file";relativePath:string;line:number;column:number|null;documentVersion:number|null};
type Target=FileTarget|RuntimeFocusTarget|{kind:"sessionResource";sessionId:string;resourceKey:string}|{kind:"route";route:string}|{kind:"matcher";runId:string;index:number}|{kind:"run";runId:string;stream:string};
interface Item{id:string;source:string;revision:string;severity:"error"|"warning"|"information";message:string;target:Target;log:Target|null;stale:boolean}
export interface ProblemsSnapshot{context:Description["context"];initialized:boolean;truncated?:boolean;runningTasks:number|null;summary?:{toolchains:string[];secretReferences:number;environmentReference:boolean}|null;problems:Item[];sources:Array<{source:string;identity:string;state:string;revision:string;truncated:boolean}>}
const labels:Record<string,string>={definitions:"프로젝트 정의",lsp:"LSP",git:"Git",dependencies:"의존성",matcher:"작업 진단",preflight:"시작 조건",session:"개발 세션"};
const severity:Record<string,string>={error:"오류",warning:"경고",information:"정보"};

export default function Problems({description,onFile,onLog,onRuntime,navigate}:{description:Description;onFile:(request:{id:string;relativePath:string;line:number;column:number|null})=>void;onLog:(request:RuntimeLogOpenRequest)=>void;onRuntime?:(target:RuntimeFocusTarget)=>void;navigate:(route:string)=>void}) {
  const [snapshot,setSnapshot]=useState<ProblemsSnapshot|null>(null);
  const [issue,setIssue]=useState("");
  const [busy,setBusy]=useState(false);
  const [unavailable,setUnavailable]=useState(false);
  const [filter,setFilter]=useState("");
  const [level,setLevel]=useState("all");
  const latest=useRef(description.context);latest.current=description.context;
  const mounted=useRef(true),pending=useRef(false);
  const call=useCallback(<T,>(method:string,args:Record<string,unknown>={})=>componentCall<T>(description,"workspace.problems",method,args,"problems"),[description]);
  const refresh=useCallback(async()=>{
    if(!description.context||pending.current)return;
    pending.current=true;
    try{const value=await call<ProblemsSnapshot>("snapshot");if(mounted.current&&sameRuntimeContext(value.context,latest.current)){setSnapshot(value);setIssue("");setUnavailable(false);}}
    catch{if(mounted.current&&sameRuntimeContext(description.context,latest.current)){setUnavailable(true);setIssue("문제 목록을 읽지 못했습니다. 마지막 결과를 유지합니다.");}}
    finally{pending.current=false;}
  },[call,description.context]);
  useEffect(()=>{mounted.current=true;void refresh();const timer=setInterval(()=>void refresh(),2000);return()=>{mounted.current=false;clearInterval(timer);};},[refresh]);
  const current=snapshot&&sameRuntimeContext(snapshot.context,description.context)?snapshot:null;
  const open=async(item:Item,log=false)=>{
    if(busy||item.stale||unavailable)return;
    setBusy(true);setIssue("");const context=description.context;
    try{
      const result=await call<{context:Description["context"];target:Target|{kind:"log";request:RuntimeLogOpenRequest}}>("resolve",{id:item.id,revision:item.revision,log});
      if(!mounted.current||!sameRuntimeContext(context,latest.current)||!sameRuntimeContext(result.context,latest.current))return;
      if(result.target.kind==="file")onFile({id:crypto.randomUUID(),relativePath:result.target.relativePath,line:result.target.line,column:result.target.column});
      else if(result.target.kind==="route")navigate(result.target.route);
      else if(result.target.kind==="log")onLog(result.target.request);
      else if(result.target.kind==="task"||result.target.kind==="port")onRuntime?.(result.target);
    }catch{if(mounted.current&&sameRuntimeContext(context,latest.current))setIssue("원본 버전이 바뀌었거나 파일·로그를 더 이상 사용할 수 없습니다. 원본에서 진단을 다시 확인해 주세요.");}
    finally{if(mounted.current)setBusy(false);}
  };
  if(!description.context)return <section><h1>문제</h1><p>문제를 확인할 프로젝트를 선택해 주세요.</p></section>;
  const rows=(current?.problems??[]).filter(item=>(level==="all"||item.severity===level)&&(!filter||item.message.toLocaleLowerCase().includes(filter.toLocaleLowerCase())));
  return <section className="workspace-problems">
    <h1>문제</h1>
    <p>열린 문서와 최근 Source·의존성·작업·세션 조회 결과를 모아 표시합니다.</p>
    <div className="problem-controls"><label>문제 검색 <input value={filter} onChange={event=>setFilter(event.target.value)}/></label>
      <label>수준 <select value={level} onChange={event=>setLevel(event.target.value)}><option value="all">전체</option><option value="error">오류</option><option value="warning">경고</option><option value="information">정보</option></select></label>
      <button onClick={()=>void refresh()}>현재 결과 새로 고침</button>
    </div>
    {issue&&<p role="status">{issue}</p>}
    {!current?.initialized&&<p>아직 수집된 진단이 없습니다. 원본 화면을 사용하면 해당 결과가 여기에 반영됩니다.</p>}
    {current?.sources.some(source=>source.state!=="ready")&&<p role="status">갱신 중이거나 확인할 수 없는 원본이 있습니다. 이전 결과는 위치 이동에 사용하지 않습니다.</p>}
    {(current?.truncated||current?.sources.some(source=>source.truncated))&&<p role="status">원본 또는 표시 한도에 도달해 일부 문제만 표시합니다.</p>}
    <ul className="problem-list">{rows.map(item=><li key={item.id} className={item.stale?"problem-stale":""}>
      <span className={"problem-severity problem-"+item.severity}>{severity[item.severity]}</span><span>{labels[item.source]??item.source}</span>
      <p>{item.message}</p>
      {item.target.kind==="file"&&<small>{item.target.relativePath}:{item.target.line}{item.target.column!==null?":"+item.target.column:" · 열 위치 미확인"}</small>}
      {item.stale&&<span>이전 결과</span>}
      <div><button disabled={busy||item.stale||unavailable} onClick={()=>void open(item)}>{item.target.kind==="route"?"원본 화면 열기":"위치 열기"}</button>{item.log&&<button disabled={busy||item.stale||unavailable} onClick={()=>void open(item,true)}>연결된 실행 로그</button>}</div>
    </li>)}</ul>
    {current?.initialized&&!rows.length&&<p>수집된 결과 중 표시할 문제가 없습니다.</p>}
  </section>;
}
