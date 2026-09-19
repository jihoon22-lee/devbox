import {useEffect,useRef,useState} from "react";
import type {ShellContentProps} from "@devbox/product-shell";
import {makeRequest,nativeMode} from "@devbox/product-shell/api";
import {isOperation} from "@devbox/product-shell/operation";
import {invoke} from "@tauri-apps/api/core";
import catalog from "../../../apps/products.json";
interface Backup {id:string;acquisition:string}
interface Row {owner:string;backups:Backup[]|null}
interface Verified {owner:string;id:string;bytes:number;sha256:string}
const labels:Record<string,string>={"sqlite-online-backup/v1":"원본 데이터베이스","sqlite-and-logs-copy/v1":"원본 데이터베이스·로그","normalized-import-bundle/v1":"가져오기 복구 스냅샷","closed-leveldb-exclusive-copy/v1":"원본 브라우저 저장소","stable-json-pair/v1":"원본 Launcher 설정","stable-json-files/v1":"원본 설정 파일"};
export default function Backups({description,route}:ShellContentProps){
 const [rows,setRows]=useState<Row[]>([]),[busy,setBusy]=useState(false),[states,setStates]=useState<Record<string,string>>({});
 const generation=useRef(0);
 useEffect(()=>()=>{generation.current++;},[]);
 const call=async<T,>(method:Record<string,string>):Promise<T>=>{
  const header=makeRequest(description.handshake,route,Date.now(),description.context);header.deadlineMs+=24000;
  const result=await invoke<{operation:unknown;value:T}>("plugin:suite|connection",{request:{header,method}});
  if(!isOperation(result.operation,{product:"control-center",component:"control-center.commands",requestId:header.requestId,revision:catalog.catalogRevision})||result.operation.outcome.state!=="succeeded")throw new Error("unavailable");
  return result.value;
 };
 const refresh=async()=>{
  if(busy||!nativeMode)return;const current=++generation.current;setBusy(true);setRows([]);setStates({});
  const next=await Promise.all(catalog.products.map(async product=>{
   try{
    const backups=await call<Backup[]>({kind:"listMigrationBackups",product:product.id});
    if(!Array.isArray(backups)||backups.length>128||backups.some(item=>!item||!/^[A-Za-z0-9_-]{1,128}$/.test(item.id)||!labels[item.acquisition]))throw new Error("invalid");
    return {owner:product.id,backups};
   }catch{return {owner:product.id,backups:null};}
  }));
  if(current===generation.current){setRows(next);setBusy(false);}
 };
 const verify=async(owner:string,backup:Backup)=>{
  if(busy)return;setBusy(true);const current=generation.current,key=`${owner}:${backup.id}`;
  setStates(previous=>({...previous,[key]:"보존된 내용 확인 중…"}));
  let status:string;
  try{
   const value=await call<Verified>({kind:"verifyMigrationBackup",product:owner,id:backup.id});
   if(value.owner!==owner||value.id!==backup.id||!Number.isSafeInteger(value.bytes)||value.bytes<0||!/^[a-f0-9]{64}$/.test(value.sha256))throw new Error("invalid");
   status=`보존된 내용 확인됨 · ${new Intl.NumberFormat("ko-KR").format(value.bytes)}바이트`;
  }catch{status="확인 실패: 파일 누락·변경 또는 읽기 오류";}
  if(current===generation.current){setStates(previous=>({...previous,[key]:status}));setBusy(false);}
 };
 return <section aria-label="제품별 이전 백업"><h2>이전 원본·복구 보존본</h2>
  <p>제품을 열고 현재 설치의 제품 연결을 승인하면 보존본을 확인할 수 있습니다. 이 화면에서 원본을 수정하거나 보존본을 삭제하지 않습니다.</p>
  <button disabled={busy||!nativeMode} onClick={()=>void refresh()}>보존본 목록 읽기</button>
  {rows.map(row=><section key={row.owner}><h3>{catalog.products.find(product=>product.id===row.owner)?.label}</h3>{row.backups===null?<p role="alert">보존본 목록을 읽지 못했습니다.</p>:row.backups.length===0?<p>이 제품에 기록된 보존본이 없습니다.</p>:<ul>{row.backups.map((backup,index)=><li key={backup.id}>
   {labels[backup.acquisition]} {index+1} <button disabled={busy} onClick={()=>void verify(row.owner,backup)}>내용 확인</button>
   <span role="status">{states[`${row.owner}:${backup.id}`]??"내용 확인 전"}</span>
  </li>)}</ul>}</section>)}
  <p>보존본 확인은 원본의 이후 변경이나 전체 이전 완료를 증명하지 않습니다. 설치 활성화 전에 별도로 확인합니다.</p>
 </section>;
}
