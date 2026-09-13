import {useEffect,useState} from "react";
import type {ShellContentProps} from "@devbox/product-shell";
import {makeRequest,nativeMode} from "@devbox/product-shell/api";
import {isOperation} from "@devbox/product-shell/operation";
import {invoke} from "@tauri-apps/api/core";
import catalog from "../../../apps/products.json";
interface Installer {app:string;version:string|null;scope:string;architecture:string;registrationId:string;binary:string;cleanup:string}
interface Snapshot {manager:Entry[]|null;installers:{entries:Installer[];complete:boolean;otherUsers:string}|null}
interface Entry {app:string;version:string;mode:"portable"|"installer"}
export default function LegacyInventory({description,route}:ShellContentProps){
 const [entries,setEntries]=useState<Snapshot|null>(null),[issue,setIssue]=useState("");
 useEffect(()=>{
  let active=true;setEntries(null);setIssue("");
  if(!nativeMode){setIssue("기존 설치 기록은 데스크톱 앱에서 확인할 수 있습니다.");return;}
  const header=makeRequest(description.handshake,route,Date.now(),description.context);
  void invoke<{operation:unknown;value:Snapshot}>("plugin:control-center|execute",{request:{header,method:"legacy_inventory",args:{}}}).then(response=>{
   if(!isOperation(response.operation,{product:"control-center",component:"control-center.delivery",requestId:header.requestId,revision:catalog.catalogRevision})||response.operation.outcome.state!=="succeeded")throw new Error("unavailable");
   if(active)setEntries(response.value);
  }).catch(()=>{if(active)setIssue("기존 Manager 설치 기록을 확인하지 못했습니다. 설치되지 않은 상태로 판단하지 않습니다.");});
  return()=>{active=false;};
 },[description,route]);
 return <section aria-label="기존 Manager 설치 기록"><h2>기존 Manager 설치 기록</h2>{issue&&<p role="alert">{issue}</p>}{entries&&<>
  {entries.manager===null?<p role="alert">Manager 등록 정보를 확인하지 못했습니다.</p>:entries.manager.length?<table><thead><tr><th>앱</th><th>버전</th><th>기록된 설치 방식</th></tr></thead><tbody>{entries.manager.map(entry=><tr key={`${entry.app}:${entry.mode}:${entry.version}`}><td>{entry.app}</td><td>{entry.version}</td><td>{entry.mode==="portable"?"Manager 관리 휴대용":"설치 프로그램 실행 기록"}</td></tr>)}</tbody></table>:<p>Manager가 관리하는 설치 기록이 없습니다.</p>}
  <h3>Windows 설치 프로그램 등록</h3>
  {entries.installers===null?<p>등록 상태를 확인하지 못했습니다.</p>:<>
   {!entries.installers.complete&&<p role="alert">일부 등록 영역을 읽지 못해 목록이 완전하지 않습니다.</p>}
   {entries.installers.entries.length?<table><thead><tr><th>앱</th><th>버전</th><th>등록 범위</th><th>파일 확인</th></tr></thead><tbody>{entries.installers.entries.map(entry=><tr key={entry.registrationId}><td>{entry.app}</td><td>{entry.version??"—"}</td><td>{entry.scope==="currentUser"?"현재 사용자":"컴퓨터"} · {entry.architecture}</td><td>추가 확인 필요</td></tr>)}</tbody></table>:<p>읽은 영역에 해당 앱의 등록이 없습니다.</p>}
   <p>다른 사용자 계정의 등록은 조사하지 않았습니다.</p>
  </>}
  <p>설치 프로그램 실행 기록만으로 현재 설치 위치나 제거 가능 여부를 확정하지 않습니다. 직접 복사한 미등록 앱은 이 목록에 포함되지 않습니다.</p>
 </>}</section>;
}
