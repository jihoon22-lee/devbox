import {useEffect,useState} from "react";
import {nativeCall} from "./native";

type Proposal={path:string;target:"windows"|"wsl"|"unsupported"};
export default function LegacyWorkspace({jobId,disabled,onReview}:{jobId:string;disabled:boolean;onReview:(jobId:string)=>void}) {
  const [proposal,setProposal]=useState<Proposal|null>(null);
  const [error,setError]=useState("");
  useEffect(()=>{
    let disposed=false;
    setProposal(null);setError("");
    void nativeCall<Proposal|null>("workspace.migration","legacy_workspace",{jobId})
      .then(value=>{if(!disposed)setProposal(value);})
      .catch(cause=>{if(!disposed)setError(cause instanceof Error?cause.message:"마지막 작업 폴더를 확인하지 못했습니다.");});
    return()=>{disposed=true;};
  },[jobId]);
  return <section aria-label="Code Pad 마지막 작업 폴더">
    <h3>마지막 작업 폴더</h3>
    {error&&<p role="alert">{error}</p>}
    {proposal&&<>
      <p>{proposal.path}</p>
      {proposal.target==="windows"?<button disabled={disabled} onClick={()=>onReview(jobId)}>마지막 작업 폴더 등록 검토</button>:
        <p>{proposal.target==="wsl"?"WSL 폴더는 보관되어 있으며 연결 기능을 준비 중입니다.":"이 경로 형식은 자동 연결을 지원하지 않습니다. 보관한 경로는 유지됩니다."}</p>}
    </>}
  </section>;
}
