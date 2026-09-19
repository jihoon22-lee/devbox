import {useEffect,useState} from "react";
import {useIncomingReview} from "@devbox/product-shell/incoming";
import {componentInvoke} from "@devbox/knowledge-features/transport";
const invoke=componentInvoke("knowledge.notes");
interface Preview {state:"prepared"|"previewPending"|"saved";draft:{title:string;body:string}}
export default function IncomingSessionSummary({onNotes}:{onNotes:()=>void}){
  const {review}=useIncomingReview();
  const [preview,setPreview]=useState<Preview|null>(null),[issue,setIssue]=useState(""),[busy,setBusy]=useState(false),[dismissed,setDismissed]=useState<string>();
  const incoming=review?.route==="daily"&&review.target.kind==="entity"&&review.target.entity==="artifact"?review:null;
  const sourceId=incoming?.target.kind==="entity"?incoming.target.id:undefined;
  useEffect(()=>{
    setPreview(null);setIssue("");if(!incoming||!sourceId)return;
    let active=true;
    void invoke<Preview>("preview_session_summary",{sourceId,operationId:incoming.operationId,revision:incoming.commandRevision}).then(value=>{if(active)setPreview(value);}).catch(()=>{if(active)setIssue("원본 세션이나 요약의 현재 상태를 확인하지 못했습니다. Workspace에서 다시 확인해 주세요.");});
    return()=>{active=false;};
  },[incoming,sourceId]);
  if(!incoming||dismissed===incoming.operationId)return null;
  const open=async()=>{
    setBusy(true);setIssue("");
    try{const next=await invoke<Preview>("open_session_summary",{sourceId,operationId:incoming.operationId,revision:incoming.commandRevision});setPreview(next);if(next.state!=="saved")onNotes();}
    catch{setIssue("초안 미리보기를 열지 못했습니다. 열려 있는 초안을 먼저 확인하거나 Workspace에서 요약을 다시 확인해 주세요.");}
    finally{setBusy(false);}
  };
  return <section aria-label="전달받은 세션 요약"><h2>개발 세션 요약</h2>{issue&&<p role="alert">{issue}</p>}{preview?<><h3>{preview.draft.title}</h3><pre>{preview.draft.body}</pre><p>{preview.state==="saved"?"이미 저장한 요약입니다.":"확인한 요약을 별도 노트로 저장할 수 있습니다. 기존 일일 노트는 유지됩니다."}</p><button disabled={busy||preview.state==="saved"} onClick={()=>void open()}>초안 저장 화면 열기</button></>:!issue&&<p role="status">원본 세션의 요약을 확인하고 있습니다…</p>}<button disabled={busy} onClick={()=>setDismissed(incoming.operationId)}>닫기</button></section>;
}
