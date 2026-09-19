import {useEffect,useRef,useState} from "react";
import type {ShellContentProps} from "@devbox/product-shell";
import {makeRequest,nativeMode} from "@devbox/product-shell/api";
import {isOperation} from "@devbox/product-shell/operation";
import {invoke} from "@tauri-apps/api/core";
import catalog from "../../../apps/products.json";
interface Summary {schemaVersion:number;owner:string;suiteVersion:string;busy:boolean;setupSelected:boolean;reviewRequired:boolean;revision:string;mappings?:{recordCount:number;revision:string}|null}
interface Row {owner:string;summary:Summary|null}
const owners=["workspace","api-studio","knowledge","control-center"];
export default function MigrationOwners({description,route}:ShellContentProps){
 const [rows,setRows]=useState<Row[]>([]),[busy,setBusy]=useState(false);
 const [recording,setRecording]=useState<string|null>(null),[recordMessage,setRecordMessage]=useState("");
 const generation=useRef(0);
 const record=async(owner:string)=>{
  if(recording||!nativeMode)return;
  setRecording(owner);setRecordMessage("");const id=generation.current;
  try{
   const header=makeRequest(description.handshake,route,Date.now(),description.context);header.deadlineMs=Date.now()+30_000;
   const response=await invoke<{operation:unknown;value:{owner:string;recorded:boolean}}>("plugin:control-center|execute",{request:{header,method:"record_migration_owner",args:{product:owner}}});
   if(!isOperation(response.operation,{product:"control-center",component:"control-center.delivery",requestId:header.requestId,revision:catalog.catalogRevision})||response.operation.outcome.state!=="succeeded"||response.value.owner!==owner||response.value.recorded!==true)throw new Error("unavailable");
   if(id===generation.current)setRecordMessage("제품의 이전 상태와 보존 백업 확인 결과를 설치 기록에 저장했습니다. 최종 원본 확인과 활성화는 아직 필요합니다.");
  }catch{if(id===generation.current)setRecordMessage("기록하지 못했습니다. 제품 연결·진행 중인 이전·보존 백업 상태를 확인한 뒤 다시 시도해 주세요.");}
  finally{if(id===generation.current)setRecording(null);}
 };
 useEffect(()=>()=>{generation.current++;},[]);
 const refresh=async()=>{
  if(busy||recording||!nativeMode)return;const id=++generation.current;setBusy(true);
  const next=await Promise.all(owners.filter(owner=>owner!=="control-center").map(async owner=>{
   try {
    const header=makeRequest(description.handshake,route,Date.now(),description.context);
    const response=await invoke<{operation:unknown;value:Summary}>("plugin:suite|connection",{request:{header,method:{kind:"readMigrationStatus",product:owner}}});
    const summary=response.value;
    if(!isOperation(response.operation,{product:"control-center",component:"control-center.commands",requestId:header.requestId,revision:catalog.catalogRevision})||response.operation.outcome.state!=="succeeded"
      ||summary.schemaVersion!==1||summary.owner!==owner||typeof summary.busy!=="boolean"||typeof summary.setupSelected!=="boolean"||typeof summary.reviewRequired!=="boolean"||!/^[a-f0-9]{64}$/.test(summary.revision))throw new Error("unavailable");
    if(summary.mappings!=null&&(!Number.isSafeInteger(summary.mappings.recordCount)||summary.mappings.recordCount<0||summary.mappings.recordCount>1_000_000||!/^[a-f0-9]{64}$/.test(summary.mappings.revision)))throw new Error("unavailable");
    return {owner,summary};
   } catch {return {owner,summary:null};}
  }));
  if(id===generation.current){setRows(next);setBusy(false);}
 };
 return <section aria-label="제품별 이전 상태"><h2>제품별 데이터 이전</h2>
  <p>각 제품에서 원본·충돌·비밀정보 재연결을 검토한 뒤 이전을 진행합니다. 제품을 열고 현재 설치의 제품 연결을 승인하면 상태를 읽을 수 있습니다.</p>
  <button disabled={busy||recording!==null||!nativeMode} onClick={()=>void refresh()}>{busy?"상태 확인 중…":"제품별 이전 상태 읽기"}</button>
  <div>{owners.map(owner=><button key={owner} disabled={busy||recording!==null||!nativeMode||description.deliveryState!=="import"} onClick={()=>void record(owner)}>{recording===owner?"백업 확인·기록 중…":`${catalog.products.find(p=>p.id===owner)?.label??owner} 이전 결과 기록`}</button>)}</div>
  {recordMessage&&<p role="status">{recordMessage}</p>}
  {rows.length>0&&<table><thead><tr><th>제품</th><th>상태</th><th>항목 이전 기록</th></tr></thead><tbody>{rows.map(row=><tr key={row.owner}><td>{catalog.products.find(p=>p.id===row.owner)?.label??row.owner}</td><td>{!row.summary?"확인되지 않음":row.summary.busy?"이전 작업 진행 중":row.summary.reviewRequired?"이전 계획 검토 필요":row.summary.setupSelected?"저장소 선택 완료":"저장소 선택 필요"}</td><td>{row.summary?.mappings?`${row.summary.mappings.recordCount}개`:"확인 필요"}</td></tr>)}</tbody></table>}
  <p>이 상태는 제품의 현재 보고입니다. 최종 데이터·제품 상태 확인이 끝나기 전에는 Suite 활성화를 확정하지 않습니다.</p>
 </section>;
}
