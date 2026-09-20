import {useEffect,useRef,useState} from "react";
import {invoke} from "@tauri-apps/api/core";
import type {ShellContentProps} from "@devbox/product-shell";
import {makeRequest,nativeMode} from "@devbox/product-shell/api";
import {isOperation} from "@devbox/product-shell/operation";
import catalog from "../../../apps/products.json";
interface Review {available:boolean;id?:string;version:string;sourceSha?:string;bytes?:number;received?:number;state?:string;issue?:string|null}
const messages:Record<string,string>={update_installed_suite_required:"설치형 Suite에서 업데이트할 수 있습니다. 독립 portable는 새 ZIP을 별도 폴더에서 검토하세요.",update_network_unavailable:"공식 릴리스 서버에 연결하지 못했습니다.",update_network_timeout:"릴리스 확인 시간이 초과되었습니다.",update_asset_changed:"다운로드한 파일의 크기 또는 해시가 공식 배포 정보와 다릅니다. 실행하지 않았습니다.",update_download_cancelled:"다운로드를 취소했습니다.",update_cache_review_required:"이전에 보관한 설치 파일의 용량·개수를 정리한 뒤 다시 시도하세요.",update_review_expired:"업데이트 정보를 다시 확인하세요.",update_busy:"진행 중인 업데이트 요청이 있습니다."};
export default function Updates({description,route}:Pick<ShellContentProps,"description"|"route">) {
 const [review,setReview]=useState<Review|null>(null),[busy,setBusy]=useState(false),[issue,setIssue]=useState(""),[confirmed,setConfirmed]=useState(false),[launching,setLaunching]=useState(false);
 const sequence=useRef(0);
 useEffect(()=>()=>{sequence.current++;},[]);
 const request=async<T,>(method:string,args:Record<string,string>={})=>{
  const header=makeRequest(description.handshake,route,Date.now(),description.context);header.deadlineMs+=24000;
  const result=await invoke<{operation:unknown;value:T&{issue?:string}}>("plugin:control-center|execute",{request:{header,method,args}});
  if(!isOperation(result.operation,{product:"control-center",component:"control-center.delivery",requestId:header.requestId,revision:catalog.catalogRevision})||result.operation.outcome.state!=="succeeded")throw new Error(result.value?.issue??"update_unavailable");
  return result.value;
 };
 useEffect(()=>{
  if(review?.state!=="downloading"||!review.id)return;
  let active=true;const id=review.id;
  const timer=setTimeout(()=>{void request<Review>("suite_update_status",{id}).then(value=>{if(active)setReview(value);}).catch(()=>{if(active){setIssue("다운로드 상태를 읽지 못했습니다. 상태를 다시 확인할 수 있습니다.");setReview(previous=>previous?.id===id?{...previous,state:"statusUnknown"}:previous);}});},700);
  return()=>{active=false;clearTimeout(timer);};
 },[review,description,route]);
 const run=async(method:string)=>{
  if(busy)return;const current=++sequence.current;setBusy(true);setIssue("");
  try {
   if(method==="launch_suite_update") {
    if(!confirmed||!review?.id)return;
    await request("launch_suite_update",{id:review.id});if(current===sequence.current)setLaunching(true);
   } else {
    const value=await request<Review>(method,method==="check_suite_update"?{}:{id:review?.id??""});
    if(current===sequence.current){setReview(value);setConfirmed(false);}
   }
  } catch(error){if(current===sequence.current)setIssue(messages[error instanceof Error?error.message:""]??"업데이트 요청을 완료하지 못했습니다. 현재 설치는 유지됩니다.");}
  finally {if(current===sequence.current)setBusy(false);}
 };
 return <section aria-label="Suite 업데이트"><h2>Suite 업데이트</h2>
  <p>공식 정식 릴리스의 네 제품을 함께 업데이트합니다. 다운로드만으로 설치를 시작하지 않습니다.</p>
  <button disabled={!nativeMode||busy||review?.state==="downloading"||launching} onClick={()=>void run("check_suite_update")}>업데이트 확인</button>
  {issue&&<p role="alert">{issue}</p>}
  {review&&!review.available&&<p>설치된 버전보다 새로운 정식 릴리스가 없습니다. 공개 버전: {review.version}</p>}
  {review?.available&&<>
   <p>새 버전 {review.version} · {((review.bytes??0)/1024/1024).toFixed(1)} MiB</p>
   {review.state==="downloading"?<><progress aria-label="설치 파일 다운로드" max={review.bytes} value={review.received}/><button disabled={busy} onClick={()=>void run("cancel_suite_update")}>다운로드 취소</button></>:review.state!=="ready"&&<button disabled={busy||launching} onClick={()=>void run(review.state==="statusUnknown"?"suite_update_status":"download_suite_update")}>{review.state==="statusUnknown"?"다운로드 상태 다시 확인":"검토한 설치 파일 다운로드"}</button>}
   {review.issue&&<p role="alert">{messages[review.issue]??"다운로드를 완료하지 못했습니다. 현재 설치는 변경되지 않았습니다."}</p>}
   {review.state==="ready"&&<div><p>공식 크기와 SHA-256을 확인했습니다. 네 제품의 작업을 저장하고 닫은 뒤 설치를 진행하세요. 기존 패키지와 데이터는 복구용으로 보존됩니다.</p>
    <label><input type="checkbox" disabled={busy||launching} checked={confirmed} onChange={event=>setConfirmed(event.target.checked)}/>버전과 제품 종료를 확인했습니다.</label>{" "}
    <button disabled={!confirmed||busy||launching} onClick={()=>void run("launch_suite_update")}>Control Center를 닫고 설치 프로그램 열기</button>
   </div>}
  </>}
  {launching&&<p role="status">설치 프로그램을 열었습니다. Control Center가 닫힙니다.</p>}
 </section>;
}
