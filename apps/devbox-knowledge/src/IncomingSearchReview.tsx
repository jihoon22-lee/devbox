import type {SavedSearchInput} from "@devbox/knowledge-features/search";
import {useEffect,useState} from "react";
import {useIncomingReview} from "@devbox/product-shell/incoming";
import {componentInvoke} from "@devbox/knowledge-features/transport";
const invoke=componentInvoke("knowledge.search");
interface Reference {reference:string;source:"notes"|"files";name:string;path:string}
/** The received ID only selects an owner-issued search reference for review. */
export default function IncomingSearchReview({onNoteOpen,onSaved}:{onNoteOpen:()=>void;onSaved:(value:SavedSearchInput)=>void}){
 const {review,clear}=useIncomingReview();
 const [reference,setReference]=useState<Reference|null>(null),[issue,setIssue]=useState("");
 const [busy,setBusy]=useState(false);
 const [saved,setSaved]=useState<(SavedSearchInput&{name:string})|null>(null);
 const savedTarget=review?.target.kind==="entity"&&review.target.entity==="savedQuery";
 const id=review?.route==="search"&&review.target.kind==="entity"&&["note","file","savedQuery"].includes(review.target.entity)?review.target.id:null;
 useEffect(()=>{
  let current=true;setReference(null);setSaved(null);setIssue("");
  if(id&&savedTarget)void invoke<SavedSearchInput&{name:string}>("source_saved_reference",{id,revision:review?.revision}).then(value=>{if(current)setSaved(value);}).catch(()=>{if(current)setIssue("저장한 검색이 변경되었거나 삭제되었습니다. 다시 검색해 주세요.");});
  if(id&&!savedTarget)void invoke<Reference>("source_reference",{reference:id}).then(value=>{if(current)setReference(value);}).catch(()=>{if(current)setIssue("검색 결과가 만료되었거나 원본이 바뀌었습니다. 다시 검색해 주세요.");});
  return()=>{current=false;};
 },[id,savedTarget,review?.revision]);
 if(!id)return null;
 const open=async()=>{
  if(!reference||busy)return;setBusy(true);setIssue("");
  try{await invoke("open_file",{reference:reference.reference});clear();if(reference.source==="notes")onNoteOpen();}
  catch{setIssue("원본을 확인하거나 열지 못했습니다. 다시 검색해 주세요.");}
  finally{setBusy(false);}
 };
 return <section aria-label="받은 검색 결과 확인">
  <h2>검색 결과 열기</h2>
  {saved&&<><p>{saved.name}</p><p>{saved.query}</p><button onClick={()=>{onSaved({...saved,id:review!.operationId});clear();}}>저장한 검색 적용</button></>}
  {reference?<><p>{reference.name}</p><p>{reference.path}</p>
   <button disabled={busy} onClick={()=>void open()}>{reference.source==="notes"?"노트에서 열기":"기본 앱에서 열기"}</button></>:!issue&&!saved&&<p role="status">원본을 확인하고 있습니다…</p>}
  {issue&&<p role="alert">{issue}</p>} <button disabled={busy} onClick={clear}>닫기</button>
 </section>;
}
