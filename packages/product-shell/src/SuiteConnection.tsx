import {lazy,Suspense,useCallback,useEffect,useRef,useState} from "react";
import {listen} from "@tauri-apps/api/event";
import {invoke} from "@tauri-apps/api/core";
import {makeRequest,nativeMode,type Description} from "./api";
import {isOperation,type Operation} from "./operation";
import catalog from "../../../apps/products.json";
const ShortcutSettings=lazy(()=>import("./ShortcutSettings"));
interface Review {token:string;version:string;root:string;installationId:string;generation:string;products:{product:string;available:boolean}[]}
export interface ConnectionStatus {connected:boolean;generation:string|null;mode:"auto"|"off";issue:string|null}
const labels:Record<string,string>=Object.fromEntries(catalog.products.map(product=>[product.id,product.label]));
const issues:Record<string,string>={
  suite_package_unavailable:"설치형 Suite로 실행할 때만 제품을 연결할 수 있습니다. 개발 빌드나 따로 푼 ZIP은 연결하지 않습니다.",
  suite_image_unavailable:"실행 파일 위치를 확인하지 못해 제품을 연결하지 않았습니다.",
};
export default function SuiteConnection({description,route}:{description:Description;route:string}){
  const [review,setReview]=useState<Review|null>(null),[status,setStatus]=useState<ConnectionStatus|null>(null);
  const [busy,setBusy]=useState(false),[issue,setIssue]=useState("");
  const statusRevision=useRef(0);
  const call=useCallback(async<T,>(method:object):Promise<T>=>{
    if(!nativeMode)throw new Error("브라우저 미리보기에서는 제품을 연결할 수 없습니다.");
    const header=makeRequest(description.handshake,route,Date.now(),description.context);
    header.deadlineMs=Date.now()+29000;
    const provenance={product:description.product.id,component:description.product.id+".commands",requestId:header.requestId,revision:catalog.catalogRevision};
    const response=await invoke<{operation:Operation;value:T}>("plugin:suite|connection",{request:{header,method}});
    if(!isOperation(response.operation,provenance)||response.operation.outcome.state!=="succeeded")throw new Error("제품 연결 응답을 확인할 수 없습니다.");
    return response.value;
  },[description,route]);
  const perform=async(action:()=>Promise<void>)=>{
    if(busy)return;statusRevision.current++;setBusy(true);setIssue("");
    try{await action();}catch{setIssue("제품 연결을 완료하지 못했습니다. 설치 상태를 확인한 뒤 다시 시도해 주세요.");}
    finally{statusRevision.current++;setBusy(false);}
  };
  useEffect(()=>{
    if(!nativeMode)return;
    let active=true;
    const removers:(()=>void)[]=[];
    const refresh=()=>{
      const request=++statusRevision.current;
      void call<ConnectionStatus>({kind:"status"}).then(value=>{if(active&&request===statusRevision.current)setStatus(value);}).catch(()=>{if(active&&request===statusRevision.current)setIssue("제품 연결 상태를 확인하지 못했습니다.");});
    };
    void (async()=>{
      try {
        for(const event of ["suite-connected","suite-disconnected","suite-connection-status"]){
          const remove=await listen(event,refresh);
          if(!active){remove();return;}removers.push(remove);
        }
      } catch { /* The initial status remains available if listening fails. */ }
      if(active)refresh();
    })();
    return()=>{active=false;statusRevision.current++;removers.forEach(remove=>remove());};
  },[call]);
  return <section aria-label="제품 연결">
    <h2>제품 연결</h2>
    <p>같은 설치 폴더의 제품은 자동으로 서로 연결되어 명령과 선택한 작업을 주고받습니다.</p>
    {status?.connected?<>
      <p role="status">이 설치의 제품이 연결되어 있습니다.</p>
      <button disabled={busy} onClick={()=>void perform(async()=>{setStatus(await call<ConnectionStatus>({kind:"disconnect"}));setReview(null);})}>자동 연결 끄기</button>
    </>:<>
      {status?.issue&&<p role="status">{issues[status.issue]??"이 실행 파일은 제품 연결을 사용할 수 없습니다."}</p>}
      {status?.mode==="off"&&<p role="status">자동 연결이 꺼져 있습니다.</p>}
      <button disabled={busy||!nativeMode} onClick={()=>void perform(async()=>{
        const current=await call<ConnectionStatus>({kind:"status"});setStatus(current);
        if(!current.connected)setReview(await call<Review>({kind:"preview"}));
      })}>이 설치 확인</button>
    </>}
    {review&&!status?.connected&&<div>
      <p>Devbox {review.version}</p><p>{review.root}</p>
      <ul>{review.products.map(product=><li key={product.product}>{labels[product.product]??product.product} · {product.available?"연결 가능":"파일 확인 필요"}</li>)}</ul>
      <button disabled={busy} onClick={()=>void perform(async()=>{setStatus(await call<ConnectionStatus>({kind:"approve",token:review.token,remember:true}));setReview(null);})}>연결 켜기</button>
      <button disabled={busy} onClick={()=>setReview(null)}>취소</button>
    </div>}
    {status?.connected&&<Suspense fallback={<p role="status">단축키 설정을 불러오고 있습니다…</p>}><ShortcutSettings description={description} route={route}/></Suspense>}
    {busy&&<p role="status">제품 연결을 확인하고 있습니다…</p>}{issue&&<p role="alert">{issue}</p>}
    {!nativeMode&&<p>브라우저 미리보기에서는 실제 제품을 연결하지 않습니다.</p>}
  </section>;
}
