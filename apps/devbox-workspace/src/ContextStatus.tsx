import { useEffect, useState } from "react";
import type { Description } from "@devbox/product-shell/api";
import { componentCall } from "./native";
import { sameRuntimeContext } from "./runtimeNavigation";
import type { ProblemsSnapshot } from "./Problems";
import "./Problems.css";
/** Reads only native caches and already-initialized Runtime metadata. */
export default function ContextStatus({description,name,root,navigate}:{description:Description;name:string;root:string;navigate:(route:string)=>void}) {
  const [snapshot,setSnapshot]=useState<ProblemsSnapshot|null>(null);
  useEffect(()=>{if(!description.context)return;let disposed=false,pending=false;const read=async()=>{
    if(pending)return;pending=true;
    try{const value=await componentCall<ProblemsSnapshot>(description,"workspace.problems","snapshot",{},"overview");if(!disposed)setSnapshot(value);}catch{if(!disposed)setSnapshot(null);}finally{pending=false;}
  };void read();const timer=setInterval(()=>void read(),5000);return()=>{disposed=true;clearInterval(timer);};},[description]);
  const current=snapshot&&sameRuntimeContext(snapshot.context,description.context)?snapshot:null;
  if(!description.context)return null;
  return <div className="workspace-context-status" aria-label="현재 개발 context">
    <span title={root}>{name} · {root.split(/[\\/]/).filter(Boolean).slice(-1)[0]}</span>
    <span>{description.context.target.kind==="windows"?"Windows":"WSL"}</span>
    <span>{current?.summary?"환경 참조 "+(current.summary.environmentReference?"지정됨":"없음"):"환경 참조 확인 전"}</span>
    <span>비밀 참조 {current?.summary?current.summary.secretReferences+"개 지정됨":"확인 전"}</span>
    <span title={current?.summary?.toolchains.join(", ")}>도구 요구 {current?.summary?current.summary.toolchains.length+"개 선언됨":"확인 전"}</span>
    <span>실행 중 작업 {current?.runningTasks??"확인 전"}</span>
    <button onClick={()=>navigate("problems")}>문제 {current?.initialized?current.problems.filter(item=>!item.stale).length+(current.sources.some(source=>source.state!=="ready")?" + 확인 필요":""):"수집 전"}</button>
  </div>;
}
