import {useEffect,useState} from "react";
import {useIncomingReview} from "@devbox/product-shell/incoming";
import type {Description} from "@devbox/product-shell/api";
import type {RuntimeLogOpenRequest} from "@devbox/workspace-features/logs";
import {componentCall} from "./native";
export default function IncomingWebhookLog({description,onOpen}:{description:Description;onOpen:(request:RuntimeLogOpenRequest)=>void}) {
  const {review,clear}=useIncomingReview();
  const [issue,setIssue]=useState(""),[retry,setRetry]=useState(0);
  const incoming=review?.route==="logs"&&review.target.kind==="entity"&&review.target.entity==="artifact"?review:null;
  const id=incoming?.target.kind==="entity"?incoming.target.id:undefined;
  useEffect(()=>{
    if(!incoming||!id)return;
    let active=true;setIssue("");
    void componentCall<RuntimeLogOpenRequest["source"]>(description,"workspace.logs","open_webhook_log",{id,revision:incoming.commandRevision,operationId:incoming.operationId},"logs")
      .then(source=>{if(active){if(source.kind!=="webhookCapture")throw new Error("invalid source");onOpen({id:incoming.operationId,source});clear();}})
      .catch(()=>{if(active)setIssue("Webhook 로그의 원본·연결·유효 시간을 확인하지 못했습니다. API Studio에서 다시 전달할 수 있습니다. 기존 로그는 유지됩니다.");});
    return()=>{active=false;};
  },[description,incoming,id,retry,onOpen,clear]);
  if(!incoming)return null;
  return <section aria-label="Webhook 로그 전달">{issue?<><p role="alert">{issue}</p><button onClick={()=>setRetry(value=>value+1)}>다시 확인</button></>:<p role="status">검토한 Webhook 로그를 가져오고 있습니다…</p>}</section>;
}
