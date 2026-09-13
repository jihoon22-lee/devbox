import {useState} from "react";
import {invoke} from "@tauri-apps/api/core";
import {makeRequest,nativeMode,type Description} from "./api";
import {isOperation,type Operation} from "./operation";
import catalog from "../../../apps/products.json";
interface Review {token:string;version:string;root:string;installationId:string;generation:string;products:{product:string;available:boolean}[]}
interface Status {connected:boolean;generation:string|null}
const labels:Record<string,string>=Object.fromEntries(catalog.products.map(product=>[product.id,product.label]));
export default function SuiteConnection({description,route}:{description:Description;route:string}){
  const [review,setReview]=useState<Review|null>(null),[status,setStatus]=useState<Status|null>(null);
  const [remember,setRemember]=useState(true);
  const [busy,setBusy]=useState(false),[issue,setIssue]=useState("");
  const call=async<T,>(method:object):Promise<T>=>{
    if(!nativeMode)throw new Error("브라우저 미리보기에서는 제품을 연결할 수 없습니다.");
    const header=makeRequest(description.handshake,route,Date.now(),description.context);
    header.deadlineMs=Date.now()+29000;
    const provenance={product:description.product.id,component:description.product.id+".commands",requestId:header.requestId,revision:catalog.catalogRevision};
    const response=await invoke<{operation:Operation;value:T}>("plugin:suite|connection",{request:{header,method}});
    if(!isOperation(response.operation,provenance)||response.operation.outcome.state!=="succeeded")throw new Error("제품 연결 응답을 확인할 수 없습니다.");
    return response.value;
  };
  const perform=async(action:()=>Promise<void>)=>{
    if(busy)return;setBusy(true);setIssue("");
    try{await action();}catch{setIssue("설치 폴더와 제품 버전을 확인해 주세요. 검토한 연결이 만료되었으면 다시 확인해 주세요.");}
    finally{setBusy(false);}
  };
  return <section aria-label="제품 연결">
    <h2>제품 연결</h2>
    <p>같은 설치 폴더의 제품끼리 명령과 선택한 작업을 전달합니다. 각 제품에서 연결할 폴더를 먼저 확인해 주세요. 기억한 연결은 제품 파일이나 설치가 바뀌면 다시 확인합니다.</p>
    {status?.connected?<><p role="status">이 설치의 제품 연결이 켜져 있습니다.</p><button disabled={busy} onClick={()=>void perform(async()=>{setStatus(await call<Status>({kind:"disconnect"}));setReview(null);})}>연결 끄기</button></>:
      <button disabled={busy||!nativeMode} onClick={()=>void perform(async()=>{
        const current=await call<Status>({kind:"status"});setStatus(current);
        if(!current.connected)setReview(await call<Review>({kind:"preview"}));
      })}>이 설치 확인</button>}
    {review&&!status?.connected&&<div>
      <p>Devbox {review.version}</p><p>{review.root}</p>
      <ul>{review.products.map(product=><li key={product.product}>{labels[product.product]??product.product} · {product.available?"연결 가능":"파일 확인 필요"}</li>)}</ul>
      <p>확인한 설치의 연결을 허용합니다. 개별 작업의 실행·저장 검토는 해당 제품에서 진행합니다.</p>
      <label><input type="checkbox" checked={remember} onChange={event=>setRemember(event.target.checked)}/>다음 실행에도 같은 설치 연결 유지</label>
      <button disabled={busy} onClick={()=>void perform(async()=>{setStatus(await call<Status>({kind:"approve",token:review.token,remember}));setReview(null);})}>확인한 제품 연결</button>
      <button disabled={busy} onClick={()=>setReview(null)}>취소</button>
    </div>}
    {busy&&<p role="status">제품 연결을 확인하고 있습니다…</p>}{issue&&<p role="alert">{issue}</p>}
    {!nativeMode&&<p>브라우저 미리보기에서는 실제 제품을 연결하지 않습니다.</p>}
  </section>;
}
