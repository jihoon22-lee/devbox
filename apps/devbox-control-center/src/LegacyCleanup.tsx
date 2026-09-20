import {useRef,useState} from "react";
import type {ShellContentProps} from "@devbox/product-shell";
import {makeRequest} from "@devbox/product-shell/api";
import {isOperation} from "@devbox/product-shell/operation";
import {invoke} from "@tauri-apps/api/core";
import catalog from "../../../apps/products.json";
interface Operation {id:string;app:string;version:string;state:"reviewed"|"cleanupPending"|"complete";issue:string|null;preservesUserData:boolean}
interface Snapshot {manager:{app:string;version:string;mode:string}[]|null;installers:{entries:{app:string;version:string|null;registrationId:string;binary:string}[]}|null}
export default function LegacyCleanup({description,route,snapshot,onCompleted}:Pick<ShellContentProps,"description"|"route">&{snapshot:Snapshot;onCompleted:()=>void}) {
 const [operations,setOperations]=useState<Operation[]>([]),[review,setReview]=useState<Operation|null>(null),[confirmed,setConfirmed]=useState(false),[busy,setBusy]=useState(false),[issue,setIssue]=useState("");
 const running=useRef(false);
 const run=async(method:string,args:Record<string,string>={})=>{
  if(running.current)return;running.current=true;setBusy(true);setIssue("");
  try {
   const header=makeRequest(description.handshake,route,Date.now(),description.context);header.deadlineMs+=24000;
   const response=await invoke<{operation:unknown;value:Operation|Operation[]|{issue:string}}>("plugin:control-center|execute",{request:{header,method,args}});
   if(!isOperation(response.operation,{product:"control-center",component:"control-center.delivery",requestId:header.requestId,revision:catalog.catalogRevision})||response.operation.outcome.state!=="succeeded")throw new Error("unavailable");
   if(Array.isArray(response.value)){setOperations(response.value);setReview(null);}
   else if("id" in response.value) {
    const value=response.value;setOperations(previous=>[...previous.filter(row=>row.id!==value.id),value]);setReview(value.state==="complete"?null:value);
    if(value.state==="complete")onCompleted();
   } else throw new Error("invalid");
   setConfirmed(false);
  }catch{setIssue("정리를 완료하지 못했습니다. 기존 앱을 닫고 설치 상태·권한을 확인한 뒤 기록을 다시 불러오세요. 사용자 데이터는 보존됩니다.");}
  finally{setBusy(false);running.current=false;}
 };
 if(description.deliveryState!=="committed")return <p>새 Suite 설치를 확정한 뒤 구 설치 정리를 검토할 수 있습니다.</p>;
 return <section aria-label="구 설치 정리"><h3>구 설치 정리</h3><p>선택한 구 버전의 프로그램과 확인된 바로가기·설치 등록만 정리합니다. 원본 데이터와 보존본은 유지됩니다. 가져오지 않은 원본을 다시 열려면 해당 구 버전을 별도로 재설치해야 할 수 있습니다.</p>
  <button disabled={busy} onClick={()=>void run("legacy_cleanup_list")}>정리 기록 불러오기</button>
  {snapshot.manager?.filter(row=>row.mode==="portable").map(row=><p key={row.app}>{row.app} {row.version} · Manager 관리 휴대용 <button disabled={busy} onClick={()=>void run("legacy_cleanup_preview",{kind:"portable",id:row.app})}>이 휴대용 설치 정리 검토</button></p>)}
  {snapshot.installers?.entries.filter(row=>row.binary==="verified").map(row=><p key={row.registrationId}>{row.app} {row.version} · Windows 설치 <button disabled={busy} onClick={()=>void run("legacy_cleanup_preview",{kind:"installer",id:row.registrationId})}>이 설치 정리 검토</button></p>)}
  {operations.map(row=><p key={row.id}>{row.app} {row.version} · {row.state==="complete"?"정리 완료 · 데이터 보존":row.state==="cleanupPending"?"새 Suite 사용 가능 · 구 설치 정리 미완료":"정리 검토 보존됨"}{row.state!=="complete"&&<button disabled={busy} onClick={()=>{setReview(row);setConfirmed(false);}}>이 정리 계속 검토</button>}</p>)}
  {issue&&<p role="alert">{issue}</p>}
  {review&&<div><p>{review.app} {review.version}의 구 설치를 정리합니다. 실행 중인 기존 앱·서비스·작업은 자동 종료하지 않습니다.</p>
   {review.state==="cleanupPending"&&<p role="alert">일부 정리가 남았습니다. 변경되거나 잠긴 파일·등록은 삭제하지 않았습니다. 문제를 해결한 뒤 다시 시도하세요.</p>}
   <label><input type="checkbox" checked={confirmed} disabled={busy} onChange={event=>setConfirmed(event.target.checked)}/>이전 결과와 정리 대상을 확인했으며 기존 앱·작업을 종료했습니다.</label>{" "}
   <button disabled={!confirmed||busy} onClick={()=>void run("legacy_cleanup_apply",{id:review.id})}>선택한 구 설치 정리 실행</button>
  </div>}
 </section>;
}
