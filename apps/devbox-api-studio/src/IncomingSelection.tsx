import {useEffect,useState} from "react";
import {useIncomingReview} from "@devbox/product-shell/incoming";
import {componentInvoke} from "@devbox/api-studio-features/transport";
const invoke=componentInvoke("api-studio.transforms");
export default function IncomingSelection(){
  const {review}=useIncomingReview();const [issue,setIssue]=useState(""),[retry,setRetry]=useState(0),[notice,setNotice]=useState("");
  const incoming=review?.route==="transforms"&&review.target.kind==="entity"&&review.target.entity==="artifact"?review:null;
  const id=incoming?.target.kind==="entity"?incoming.target.id:undefined;
  useEffect(()=>{
    if(!incoming||!id)return;let active=true;setIssue("");setNotice("");
    void invoke<{state:string}>("open_workspace_selection",{id,operationId:incoming.operationId,revision:incoming.commandRevision}).then(value=>{if(active)setNotice(value.state==="applied"?"이미 적용한 선택 내용입니다.":"미리보기를 확인한 뒤 변환 도구에 적용해 주세요.");}).catch(()=>{if(active)setIssue("원본이 변경·닫힘·만료되었거나 기존 미리보기가 열려 있습니다. 원본 선택 내용은 Workspace에서 유지됩니다.");});
    return()=>{active=false;};
  },[incoming,id,retry]);
  if(!incoming)return null;
  return <section aria-label="Workspace 선택 내용">{notice&&<p role="status">{notice}</p>}{issue&&<><p role="alert">{issue}</p><button onClick={()=>setRetry(value=>value+1)}>선택 내용 다시 확인</button></>}</section>;
}
