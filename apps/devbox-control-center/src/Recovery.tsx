import {useEffect,useState} from "react";
import type {ShellContentProps} from "@devbox/product-shell";
import {makeRequest,nativeMode} from "@devbox/product-shell/api";
import {isOperation} from "@devbox/product-shell/operation";
import {invoke} from "@tauri-apps/api/core";
import catalog from "../../../apps/products.json";
interface Status {state:"none"|"recorded";phase?:string;committed?:boolean;recovery?:string;backupCount?:number;importCount?:number;cleanupPending?:number;failure?:string|null}
const phases:Record<string,string>={inventory:"설치 상태 조사",stage:"패키지 준비",verify:"패키지 확인",snapshot:"일관된 백업",import:"데이터 이전",validate:"이전 결과 확인",quiesce:"실행 중인 작업 정지 확인",activate:"새 버전 활성화",health:"제품 상태 확인",commit:"활성화 확정",cleanup:"구 설치 정리",complete:"완료",recover:"복구 필요",recovered:"복구 완료"};
export default function Recovery({description,route}:ShellContentProps){
 const [status,setStatus]=useState<Status|null>(null),[error,setError]=useState("");
 useEffect(()=>{
  let active=true;setStatus(null);setError("");
  if(!nativeMode){setError("설치·복구 기록은 데스크톱 앱에서 확인할 수 있습니다.");return;}
  const header=makeRequest(description.handshake,route,Date.now(),description.context);
  void invoke<{operation:unknown;value:Status}>("plugin:control-center|execute",{request:{header,method:"suite_recovery",args:{}}}).then(response=>{
   if(!isOperation(response.operation,{product:"control-center",component:"control-center.delivery",requestId:header.requestId,revision:catalog.catalogRevision})||response.operation.outcome.state!=="succeeded")throw new Error("unavailable");
   if(active)setStatus(response.value);
  }).catch(()=>{if(active)setError("복구 기록을 읽지 못했습니다. 기존 기록과 백업은 보존됩니다.");});
  return()=>{active=false;};
 },[description,route]);
 return <section aria-label="Suite 복구 기록"><h2>설치·데이터 복구</h2>{error&&<p role="alert">{error}</p>}{status?.state==="none"?<p>이 설치에서 진행한 Suite 전환 기록이 없습니다.</p>:status?.state==="recorded"?<>
  <p>현재 단계: {phases[status.phase??""]??"확인되지 않음"}</p>
  <p>백업 {status.backupCount}개 · 이전 기록 {status.importCount}개 · 구 설치 정리 대기 {status.cleanupPending}개</p>
  {status.failure&&<p role="alert">{status.committed?"새 Suite 활성화는 완료됐지만 구 설치 정리가 남아 있습니다.":"전환을 완료하지 못했습니다. 이전 패키지와 백업을 보존한 상태에서 복구해야 합니다."}</p>}
 </>:!error&&<p role="status">복구 기록을 확인하고 있습니다…</p>}</section>;
}
