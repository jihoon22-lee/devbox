import {useEffect,useRef,useState} from "react";
import {useIncomingReview} from "@devbox/product-shell/incoming";
import {openReceivedFile} from "@devbox/product-shell/commands";
import type {Description,ProjectContext} from "@devbox/product-shell/api";
interface Open {id:string;contextKey:string;path:string;line:null;receivedReference:string}
export default function IncomingFileReview({description,onOpen}:{description:Description;onOpen:(request:Open)=>void}){
  const {review}=useIncomingReview();
  const [issue,setIssue]=useState(""),[retry,setRetry]=useState(0),[busy,setBusy]=useState(false);
  const current=useRef<{context:ProjectContext|null|undefined}>({context:description.context});current.current.context=description.context;
  const incoming=review?.route==="files"&&review.target.kind==="entity"&&review.target.entity==="file"?review:null;
  const reference=incoming?.target.kind==="entity"?incoming.target.id:undefined;
  useEffect(()=>{
    if(!incoming||!reference)return;
    let active=true;setBusy(true);setIssue("");
    void openReceivedFile(description,"files",reference).then(value=>{
      if(!active)return;
      if(JSON.stringify(value.context)!==JSON.stringify(current.current.context??null))throw new Error("context changed");
      onOpen({id:incoming.operationId,contextKey:JSON.stringify(value.context),path:value.path,line:null,receivedReference:reference});
    }).catch(()=>{if(active)setIssue("원본 파일·연결·현재 작업 폴더를 확인하지 못했습니다. WSL 파일은 해당 작업 폴더를 선택한 뒤 다시 확인해 주세요. 기존 편집 내용은 유지됩니다.");}).finally(()=>{if(active)setBusy(false);});
    return()=>{active=false;};
  },[description,incoming,reference,retry,onOpen]);
  if(!incoming)return null;
  return <section aria-label="Knowledge 파일 열기">{busy&&<p role="status">선택한 파일의 현재 상태를 확인하고 있습니다…</p>}{issue&&<><p role="alert">{issue}</p><button onClick={()=>setRetry(value=>value+1)}>파일 다시 확인</button></>}</section>;
}
