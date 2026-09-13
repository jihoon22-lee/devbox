import {useCallback,useEffect,useState} from "react";
import type {ShellContentProps} from "@devbox/product-shell";
import {makeRequest,nativeMode} from "@devbox/product-shell/api";
import {isOperation} from "@devbox/product-shell/operation";
import {invoke} from "@tauri-apps/api/core";
interface Product {id:string;name:string;bundleIdentifier:string;binary:string;version:string|null;runtime:string}
interface Component {id:string;owner:string;binary:string;runtime:string;version:string|null}
interface Snapshot {installationKey:string|null;generation:string|null;suiteVersion:string|null;declaration:string;installerRegistration:string;products:Product[];components:Component[]}
const label=(state:string)=>state==="verified"?"파일 확인됨":state==="notIncluded"?"이 패키지에 포함되지 않음":"확인되지 않음";
export default function Inventory({description,route}:ShellContentProps){
 const [snapshot,setSnapshot]=useState<Snapshot|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState("");
 const [refresh,setRefresh]=useState(0);
 const reload=useCallback(()=>setRefresh(value=>value+1),[]);
 useEffect(()=>{
  let active=true;setBusy(true);setError("");
  if(!nativeMode){setBusy(false);setError("설치 상태는 데스크톱 앱에서 확인할 수 있습니다.");return;}
  const header=makeRequest(description.handshake,route,Date.now(),description.context);
  const provenance={product:"control-center",component:"control-center.delivery",requestId:header.requestId,revision:catalog.catalogRevision};
  void invoke<{operation:unknown;value:Snapshot}>("plugin:control-center|execute",{request:{header,method:"suite_inventory",args:{}}}).then(response=>{
   if(!isOperation(response.operation,provenance)||response.operation.outcome.state!=="succeeded")throw new Error("inventory_unavailable");
   if(active)setSnapshot(response.value);
  }).catch(()=>{if(active)setError("설치 상태를 확인하지 못했습니다. 마지막 결과가 있다면 그대로 표시합니다.");}).finally(()=>{if(active)setBusy(false);});
  return()=>{active=false;};
 },[description,route,refresh]);
 return <section aria-label={route==="components"?"구성 요소":"제품 설치 상태"}>
  <h2>{route==="components"?"구성 요소":"제품 설치 상태"}</h2>
  <button onClick={reload} disabled={busy}>{busy?"확인 중…":"설치 상태 새로고침"}</button>
  {error&&<p role="alert">{error}</p>}
  {snapshot&&<>
   <p>Suite {snapshot.suiteVersion??"버전 확인되지 않음"} · 패키지 {label(snapshot.declaration)}</p>
   {route==="components"?<table><thead><tr><th>구성 요소</th><th>담당 제품</th><th>버전</th><th>파일</th><th>실행 상태</th></tr></thead><tbody>{snapshot.components.map(item=><tr key={item.id}><td>{item.id}</td><td>{item.owner}</td><td>{item.version??"—"}</td><td>{label(item.binary)}</td><td>{label(item.runtime)}</td></tr>)}</tbody></table>:
   <table><thead><tr><th>제품</th><th>버전</th><th>파일</th><th>실행 상태</th></tr></thead><tbody>{snapshot.products.map(item=><tr key={item.id}><td>{item.name}</td><td>{item.version??"—"}</td><td>{label(item.binary)}</td><td>{label(item.runtime)}</td></tr>)}</tbody></table>}
   <p>설치 프로그램 등록: {label(snapshot.installerRegistration)}. 파일 확인만으로 실행 중인 상태를 판단하지 않습니다.</p>
  </>}
 </section>;
}
import catalog from "../../../apps/products.json";
