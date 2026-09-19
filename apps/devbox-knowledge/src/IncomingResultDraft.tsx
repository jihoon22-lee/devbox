import {useEffect,useState} from "react";
import {useIncomingReview} from "@devbox/product-shell/incoming";
import {componentInvoke} from "@devbox/knowledge-features/transport";
const invoke=componentInvoke("knowledge.notes");
export default function IncomingResultDraft(){
  const {review}=useIncomingReview();const [issue,setIssue]=useState(""),[retry,setRetry]=useState(0),[notice,setNotice]=useState("");
  const incoming=review?.route==="notes"&&review.target.kind==="entity"&&review.target.entity==="artifact"?review:null;
  const id=incoming?.target.kind==="entity"?incoming.target.id:undefined;
  useEffect(()=>{
    if(!incoming||!id)return;let active=true;setIssue("");setNotice("");
    void invoke<{state:string}>("open_result_draft",{id,operationId:incoming.operationId,revision:incoming.commandRevision}).then(value=>{if(active)setNotice(value.state==="saved"?"이미 저장한 결과 초안입니다.":"본문과 태그를 확인한 뒤 노트로 저장해 주세요.");}).catch(()=>{if(active)setIssue("초안이 변경·삭제·만료되었거나 기존 미리보기가 열려 있습니다. 원본은 API Studio 보관함에서 확인하고 내보낼 수 있습니다.");});
    return()=>{active=false;};
  },[incoming,id,retry]);
  if(!incoming)return null;
  return <section aria-label="API Studio 결과 초안">{notice&&<p role="status">{notice}</p>}{issue&&<><p role="alert">{issue}</p><button onClick={()=>setRetry(value=>value+1)}>초안 다시 확인</button></>}</section>;
}
