import {useCallback,useEffect,useState} from "react";
import {invoke} from "@tauri-apps/api/core";
import {listen} from "@tauri-apps/api/event";
import {makeRequest,nativeMode,isProjectContext,type Description} from "./api";
import {isOperation} from "./operation";
import catalog from "../../../apps/products.json";
import type {IncomingReview as Review} from "./incoming";
export default function IncomingCommands({description,route,navigate,onReview}:{description:Description;route:string;navigate:(route:string)=>void;onReview:(review:Review)=>void}){
  const [reviews,setReviews]=useState<Review[]>([]),[opening,setOpening]=useState<{id:string;route:string}|null>(null);
  const [busy,setBusy]=useState(false),[issue,setIssue]=useState("");
  const call=useCallback(async<T,>(method:object):Promise<T>=>{
    const header=makeRequest(description.handshake,route,Date.now(),description.context);
    const provenance={product:description.product.id,component:description.product.id+".commands",requestId:header.requestId,revision:catalog.catalogRevision};
    const response=await invoke<{operation:unknown;value:T}>("plugin:suite|connection",{request:{header,method}});
    if(!isOperation(response.operation,provenance)||response.operation.outcome.state!=="succeeded")throw new Error("invalid response");
    return response.value;
  },[description,route]);
  useEffect(()=>{
    if(!nativeMode)return;
    let active=true,remove:(()=>void)|undefined;
    const refresh=()=>{void call<Review[]>({kind:"pending"}).then(value=>{
      if(!Array.isArray(value)||value.length>32||value.some(row=>typeof row.commandRevision!=="string"||!/^[a-f0-9]{64}$/.test(row.commandRevision)||!description.features.some(feature=>feature.route===row.route)||!row.target||!["route","entity"].includes(row.target.kind)||(row.context!==null&&!isProjectContext(row.context))))throw new Error("invalid navigation");
      if(active)setReviews(value);
    }).catch(()=>{if(active)setIssue("다른 제품의 열기 요청을 확인하지 못했습니다.");});};
    void listen("suite-navigation",refresh).then(unlisten=>{if(!active)unlisten();else{remove=unlisten;refresh();}}).catch(()=>{if(active)setIssue("제품 요청 알림을 연결하지 못했습니다.");});
    return()=>{active=false;remove?.();};
  },[call,description]);
  useEffect(()=>{
    if(!opening||opening.route!==route)return;
    const id=opening.id;setOpening(null);
    void call({kind:"acknowledge",id}).catch(()=>setIssue("화면을 열었지만 요청 제품에 결과를 전달하지 못했습니다."));
  },[call,opening,route]);
  const decide=async(review:Review,accept:boolean)=>{
    if(busy)return;setBusy(true);setIssue("");
    try{
      const result=await call<{operationId:string;route:string|null}>({kind:"decide",id:review.operationId,revision:review.revision,accept});
      if(result.operationId!==review.operationId||result.route!==(accept?review.route:null))throw new Error("invalid navigation");
      setReviews(values=>values.filter(value=>value.operationId!==review.operationId));
      if(result.route){onReview(review);setOpening({id:review.operationId,route:result.route});navigate(result.route);}
    }catch{setIssue("요청이 변경되었거나 만료되었습니다. 요청 제품에서 다시 확인해 주세요.");}
    finally{setBusy(false);}
  };
  if(!reviews.length&&!issue)return null;
  return <section aria-label="다른 제품의 열기 요청">
    {reviews.map(review=><div key={review.operationId}><strong>{review.label}</strong><dl><dt>담당 제품</dt><dd>{description.product.label}</dd><dt>대상</dt><dd>{review.target.kind==="route"?review.target.route:`${review.target.entity} · ${review.target.id}`}</dd><dt>변경 내용</dt><dd>담당 화면을 열고 현재 대상을 확인합니다. 실행·저장·삭제는 해당 화면에서 별도로 검토합니다.</dd><dt>확인한 버전</dt><dd>{review.commandRevision.slice(0,12)}</dd></dl>
      <button disabled={busy} onClick={()=>void decide(review,true)}>화면 열기</button><button disabled={busy} onClick={()=>void decide(review,false)}>거절</button></div>)}
    {issue&&<p role="alert">{issue}</p>}
  </section>;
}
