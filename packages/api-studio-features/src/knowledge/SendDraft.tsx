import {useState} from "react";
import {sendDraft,type KnowledgeDraft} from "./api";
export function SendDraft({draft,disabled=false}:{draft:KnowledgeDraft;disabled?:boolean}){
  const [busy,setBusy]=useState(false),[notice,setNotice]=useState("");
  return <><button className="btn" disabled={busy||disabled} onClick={()=>{
    setBusy(true);setNotice("");
    void sendDraft(draft.artifact.provenance.component,draft.artifact.id).then(()=>setNotice("Knowledge에서 초안 열기를 확인해 주세요. 아직 노트로 저장하지 않았습니다.")).catch(()=>setNotice("전달 결과를 확인하지 못했습니다. Knowledge 설치·연결과 원본 상태를 확인해 주세요. API Studio 보관 사본은 유지되며 직접 내보낼 수 있습니다.")).finally(()=>setBusy(false));
  }}>Knowledge에서 초안 검토</button>{notice&&<p role="status">{notice}</p>}</>;
}
