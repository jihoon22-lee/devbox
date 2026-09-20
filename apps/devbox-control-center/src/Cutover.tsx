import {useRef,useState} from "react";
import {invoke} from "@tauri-apps/api/core";
import type {ShellContentProps} from "@devbox/product-shell";
import {makeRequest,nativeMode} from "@devbox/product-shell/api";
import {isOperation} from "@devbox/product-shell/operation";
import catalog from "../../../apps/products.json";
import legacy from "../../../apps/legacy-v0.7-catalog.json";
interface Source {identifier:string;owner:string;currentImport:boolean;backupCount:number}
interface Review {revision:string;sources:Source[];prepared:boolean;choices?:{identifier:string;disposition:Choice}[]}
type Choice="acceptedImport"|"keepLegacy";
const messages:Record<string,string>={cutover_source_changed:"기존 원본이 바뀌었습니다. 해당 제품에서 다시 가져오거나 원본 보존을 검토한 뒤 제품별 결과를 새로 기록하세요.",cutover_owner_review_required:"네 제품의 이전 또는 신규 사용 선택을 마치고 제품별 결과를 모두 기록하세요.",cutover_source_observation_required:"제품별 결과를 새로 기록해야 현재 원본과 비교할 수 있습니다.",cutover_review_stale:"기록이 바뀌었습니다. 전환 계획을 다시 불러오세요.",legacy_writers_must_close:"기존 앱과 실행 중인 작업을 저장하고 닫아 주세요."};
export default function Cutover({description,route}:Pick<ShellContentProps,"description"|"route">) {
 const [review,setReview]=useState<Review|null>(null),[choices,setChoices]=useState<Record<string,Choice|"">>({}),[issue,setIssue]=useState(""),[busy,setBusy]=useState(false),[confirmed,setConfirmed]=useState(false),[leaving,setLeaving]=useState(false);
 const running=useRef(false);
 const request=async<T,>(method:string,args:unknown)=>{
  const header=makeRequest(description.handshake,route,Date.now(),description.context);header.deadlineMs+=24000;
  const response=await invoke<{operation:unknown;value:T&{issue?:string}}>("plugin:control-center|execute",{request:{header,method,args}});
  if(!isOperation(response.operation,{product:"control-center",component:"control-center.delivery",requestId:header.requestId,revision:catalog.catalogRevision})||response.operation.outcome.state!=="succeeded")throw new Error(response.value?.issue??"cutover_unavailable");
  return response.value;
 };
 const run=async(action:"load"|"prepare"|"activate")=>{
  if(running.current)return;running.current=true;setBusy(true);setIssue("");
  try {
   if(action==="activate") {
    if(!review?.prepared||!confirmed)return;
    const value=await request<{accepted:boolean}>("restore_action",{action:"activateReviewed",id:""});
    if(!value.accepted)throw new Error("cutover_unavailable");
    setLeaving(true);
   } else {
    if(action==="prepare"&&(!review||!confirmed||review.sources.some(row=>!choices[row.identifier])))return;
    const value=await request<Review>(action==="load"?"cutover_review":"prepare_cutover",action==="load"?{}:{revision:review!.revision,choices:review!.sources.map(row=>({identifier:row.identifier,disposition:choices[row.identifier]}))});
    setReview(value);setConfirmed(false);
    setChoices(Object.fromEntries((value.choices??[]).map(choice=>[choice.identifier,choice.disposition])));
   }
  } catch(error) {setIssue(messages[error instanceof Error?error.message:""]??"전환 계획을 준비하지 못했습니다. 제품별 결과를 다시 확인하세요. 원본과 기존 설치는 보존됩니다.");}
  finally {setBusy(false);running.current=false;}
 };
 return <section aria-label="기존 데이터 전환 계획"><h2>기존 데이터 전환 계획</h2>
  <p>먼저 각 제품에서 가져올 항목과 충돌을 검토하고 위에서 결과를 기록하세요. 기존 앱의 작업·서비스를 저장하고 종료한 뒤 원본별 선택을 확정합니다.</p>
  <button disabled={!nativeMode||busy||leaving} onClick={()=>void run("load")}>전환 계획 불러오기</button>
  {issue&&<p role="alert">{issue}</p>}
  {review&&<>
   <p>원본 일치는 가져오기 당시와 현재 원본의 일치를 뜻합니다. 가져오지 않은 설정·다시 연결할 자격 증명·재생성할 검색 색인은 각 제품의 이전 결과에서 확인하세요. 기존 원본은 모든 선택에서 보존됩니다.</p>
   {review.sources.map(row=><div key={row.identifier}><label>{legacy.apps.find(app=>app.identifier===row.identifier)?.displayName??(row.identifier==="com.workbench.codepad"?"Code Pad 이전 프로필":row.identifier)}{" "}
    <select aria-label={`${row.identifier} 전환 선택`} disabled={busy||leaving} value={choices[row.identifier]??""} onChange={event=>{setChoices(previous=>({...previous,[row.identifier]:event.target.value as Choice}));setReview({...review,prepared:false});setConfirmed(false);}}>
     <option value="">이전 결과를 검토하고 선택</option>
     {row.currentImport&&<option value="acceptedImport">원본이 일치하는 가져오기 결과 사용</option>}
     <option value="keepLegacy">이전 완료로 계산하지 않고 기존 원본 보존</option>
    </select></label><p>{row.currentImport?`가져온 원본 일치 · 보존본 ${row.backupCount}개` : "현재 원본과 일치하는 가져오기 기록 없음 · 다시 가져오기 또는 원본 보존 선택 필요"}</p></div>)}
   {review.sources.length===0&&<p>발견된 기존 데이터 원본이 없습니다. 기존 설치 여부와 제품별 신규 사용 선택을 확인하세요.</p>}
   <label><input type="checkbox" disabled={busy||leaving} checked={confirmed} onChange={event=>setConfirmed(event.target.checked)}/>{review.prepared?"전환 계획을 확인했으며 기존 앱과 네 제품을 종료하겠습니다.":"항목별 이전 결과와 제외한 데이터를 확인했습니다. 원본 보존 선택은 이전 완료가 아님을 확인합니다."}</label>{" "}
   {review.prepared?<button disabled={!confirmed||busy||leaving} onClick={()=>void run("activate")}>Control Center를 닫고 전환 준비</button>:<button disabled={!confirmed||busy||leaving||review.sources.some(row=>!choices[row.identifier])} onClick={()=>void run("prepare")}>선택한 전환 계획 보존</button>}
  </>}
  {leaving&&<p role="status">전환 도우미를 시작했습니다. 나머지 제품도 닫아 주세요. 이후 네 제품의 상태를 확인한 뒤 설치를 확정합니다.</p>}
 </section>;
}
