import {useEffect,useState} from "react";
import {useIncomingReview} from "@devbox/product-shell/incoming";
import type {Description} from "@devbox/product-shell/api";
import {componentCall} from "./native";
interface Run {id:string;jobId:string;status:string;logsAvailable:boolean;startedAt:number|null;createdAt:number}
export default function IncomingRuntimeReview({description}:{description:Description}){
 const {review,clear}=useIncomingReview();
 const [run,setRun]=useState<Run|null>(null),[issue,setIssue]=useState(""),[busy,setBusy]=useState(false);
 const id=review?.route==="tasks"&&review.target.kind==="entity"&&review.target.entity==="run"?review.target.id:null;
 useEffect(()=>{
  let current=true;setRun(null);setIssue("");
  if(id)void componentCall<Run|null>(description,"workspace.runtime","get_run",{id},"tasks").then(value=>{
   if(current){if(value?.id===id)setRun(value);else setIssue("실행 기록이 더 이상 없습니다.");}
  }).catch(()=>{if(current)setIssue("실행 기록을 확인하지 못했습니다.");});
  return()=>{current=false;};
 },[description,id]);
 if(!id)return null;
 const openLog=async(stream:string)=>{
  setBusy(true);setIssue("");try{await componentCall(description,"workspace.runtime","open_run_log_in_log_lens",{runId:id,stream},"tasks");clear();}
  catch{setIssue("현재 실행 로그를 열지 못했습니다.");}finally{setBusy(false);}
 };
 const labels:Record<string,string>={queued:"대기",starting:"시작 중",running:"실행 중",stopping:"종료 중",succeeded:"성공",failed:"실패",cancelled:"취소",skipped:"건너뜀"};
 return <section aria-label="받은 실행 기록"><h2>{review?.label}</h2>
  {run?<><p>{labels[run.status]??"상태 확인 필요"} · {new Date(run.startedAt??run.createdAt).toLocaleString()}</p>
   <button disabled={busy||!run.logsAvailable} onClick={()=>void openLog("stdout")}>출력 로그 열기</button>{" "}<button disabled={busy||!run.logsAvailable} onClick={()=>void openLog("stderr")}>오류 로그 열기</button></>:!issue&&<p role="status">실행 기록을 확인하고 있습니다…</p>}
  {issue&&<p role="alert">{issue}</p>} <button disabled={busy} onClick={clear}>닫기</button>
 </section>;
}
