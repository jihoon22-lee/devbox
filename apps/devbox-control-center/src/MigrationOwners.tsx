import {useEffect,useRef,useState} from "react";
import type {ShellContentProps} from "@devbox/product-shell";
import {makeRequest,nativeMode} from "@devbox/product-shell/api";
import {isOperation} from "@devbox/product-shell/operation";
import {invoke} from "@tauri-apps/api/core";
import catalog from "../../../apps/products.json";
interface Summary {schemaVersion:number;owner:string;suiteVersion:string;busy:boolean;setupSelected:boolean;reviewRequired:boolean;revision:string;mappings?:{recordCount:number;revision:string}|null}
interface Row {owner:string;summary:Summary|null}
const owners=["workspace","api-studio","knowledge"];
export default function MigrationOwners({description,route}:ShellContentProps){
 const [rows,setRows]=useState<Row[]>([]),[busy,setBusy]=useState(false);
 const generation=useRef(0);
 useEffect(()=>()=>{generation.current++;},[]);
 const refresh=async()=>{
  if(busy||!nativeMode)return;const id=++generation.current;setBusy(true);
  const next=await Promise.all(owners.map(async owner=>{
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
  <button disabled={busy||!nativeMode} onClick={()=>void refresh()}>{busy?"상태 확인 중…":"제품별 이전 상태 읽기"}</button>
  {rows.length>0&&<table><thead><tr><th>제품</th><th>상태</th><th>항목 이전 기록</th></tr></thead><tbody>{rows.map(row=><tr key={row.owner}><td>{catalog.products.find(p=>p.id===row.owner)?.label??row.owner}</td><td>{!row.summary?"확인되지 않음":row.summary.busy?"이전 작업 진행 중":row.summary.reviewRequired?"이전 계획 검토 필요":row.summary.setupSelected?"저장소 선택 완료":"저장소 선택 필요"}</td><td>{row.summary?.mappings?`${row.summary.mappings.recordCount}개`:"확인 필요"}</td></tr>)}</tbody></table>}
  <p>이 상태는 제품의 현재 보고입니다. 최종 데이터·제품 상태 확인이 끝나기 전에는 Suite 활성화를 확정하지 않습니다.</p>
 </section>;
}
