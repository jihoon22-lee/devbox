import {useState} from "react";
import {invoke} from "@tauri-apps/api/core";
import {makeRequest,nativeMode,type Description} from "./api";
import {isOperation} from "./operation";
import catalog from "../../../apps/products.json";
import ShortcutControls from "./ShortcutControls";
import type {ShortcutConfig,ShortcutStatus} from "./launcher/types";
export default function ShortcutSettings({description,route}:{description:Description;route:string}){
 const [status,setStatus]=useState<ShortcutStatus|null>(null),[busy,setBusy]=useState(false),[issue,setIssue]=useState("");
 const read=async(config?:ShortcutConfig)=>{
  setBusy(true);setIssue("");
  try{
   const header=makeRequest(description.handshake,route,Date.now(),description.context);header.deadlineMs=Date.now()+29000;
   const provenance={product:description.product.id,component:description.product.id+".commands",requestId:header.requestId,revision:catalog.catalogRevision};
   const response=await invoke<{operation:unknown;value:ShortcutStatus}>("plugin:suite|connection",{request:{header,method:config?{kind:"configureShortcuts",config}:{kind:"shortcutStatus"}}});
   const value=response.value;
   if(!isOperation(response.operation,provenance)||response.operation.outcome.state!=="succeeded"||!value||!["Ctrl+Alt+Space","Ctrl+Alt+L","Ctrl+Alt+J"].includes(value.accelerator)||![value.enabled,value.terminal,value.capture,value.project].every(value=>typeof value==="boolean")||!["registered","disabled","pending","unavailable","unsupported"].includes(value.registration))throw new Error("invalid shortcut owner");
   setStatus(value);
  }catch{setIssue("Control Center의 설치와 제품 연결을 확인해 주세요. 아직 연결하지 않았다면 열린 Control Center에서 이 설치를 확인한 뒤 다시 불러오세요.");}
  finally{setBusy(false);}
 };
 return <section aria-label="공용 단축키 설정"><h3>전역 단축키</h3>
  <p>같은 설치의 Control Center가 네 단축키를 함께 관리합니다.</p>
  <button disabled={busy||!nativeMode} onClick={()=>void read()}>이 설치의 단축키 설정 불러오기</button>
  {status&&<><ShortcutControls status={status} busy={busy} onChange={config=>void read(config)}/>
   <p role="status">{status.issue?"다른 설치 또는 앱이 단축키를 사용 중이거나 설정을 적용하지 못했습니다. 기존 조합과 등록 상태를 확인해 주세요.":status.registration==="registered"?"이 설치에서 전역 단축키를 사용 중입니다.":status.registration==="disabled"?"전역 단축키가 꺼져 있습니다.":"단축키 등록 상태를 확인해 주세요."}</p></>}
  {issue&&<p role="alert">{issue}</p>}
 </section>;
}
