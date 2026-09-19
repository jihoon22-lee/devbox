import {useCallback,useEffect,useRef,useState} from "react";
import {invoke} from "@tauri-apps/api/core";
import {makeRequest,nativeMode,type Description} from "./api";
import {isOperation} from "./operation";
import catalog from "../../../apps/products.json";
const phases={review:"검토 대기",running:"진행 중",cancelRequested:"취소 요청됨 · 종료 확인 중",cancelled:"취소 완료",uncancellable:"취소할 수 없는 단계",succeeded:"완료",failed:"실패",unknown:"담당 화면에서 상태 확인 필요"} as const;
interface Row {id:string;owner:string;component:string;route:string;label:string;phase:keyof typeof phases;revision:string}
function parse(value:unknown,owner:string):Row[]{
  if(!Array.isArray(value)||value.length>128)throw new Error("invalid operations");
  const ids=new Set<string>();
  for(const row of value){
    if(!row||typeof row!=="object"||Object.keys(row).sort().join(",")!=="component,id,label,owner,phase,revision,route"||typeof row.id!=="string"||!/^[A-Za-z0-9._:-]{1,128}$/.test(row.id)||ids.has(row.id)||row.owner!==owner||typeof row.label!=="string"||row.label.length>256||/[\u0000-\u001f\u007f]/.test(row.label)||!Object.prototype.hasOwnProperty.call(phases,row.phase)||typeof row.revision!=="string"||!/^[a-f0-9]{64}$/.test(row.revision)||!catalog.features.some(feature=>feature.owner===owner&&feature.route===row.route)||!catalog.components.some(component=>component.owner===owner&&component.id===row.component))throw new Error("invalid operation source");
    ids.add(row.id);
  }
  return value as Row[];
}
export default function Operations({description,route}:{description:Description;route:string}){
  const [sources,setSources]=useState<Record<string,{rows:Row[];unavailable:boolean}>>({});
  const [notice,setNotice]=useState(""),[issue,setIssue]=useState(""),[busy,setBusy]=useState(false),[revision,setRevision]=useState(0);
  const seen=useRef(new Map<string,string>());
  const call=useCallback(async<T,>(method:object):Promise<T>=>{
    const header=makeRequest(description.handshake,route,Date.now(),description.context);
    const provenance={product:description.product.id,component:description.product.id+".commands",requestId:header.requestId,revision:catalog.catalogRevision};
    const response=await invoke<{operation:unknown;value:T}>("plugin:suite|connection",{request:{header,method}});
    if(!isOperation(response.operation,provenance)||response.operation.outcome.state!=="succeeded")throw new Error("operation source unavailable");
    return response.value;
  },[description,route]);
  useEffect(()=>{
    if(!nativeMode)return;let active=true,timer:ReturnType<typeof setTimeout>|undefined,inFlight=false;
    const products=description.product.id==="control-center"?catalog.products.map(product=>product.id):[description.product.id];
    const refresh=async()=>{
      if(!active||inFlight||document.hidden)return;inFlight=true;
      const results=await Promise.allSettled(products.map(async product=>parse(await call({kind:"readOperations",product}),product)));
      if(active){
        const next:typeof sources={},notifications:string[]=[];
        results.forEach((result,index)=>{
          const product=products[index];next[product]={rows:result.status==="fulfilled"?result.value:[],unavailable:result.status!=="fulfilled"};
          if(result.status==="fulfilled")for(const row of result.value){const key=product+":"+row.id;const previous=seen.current.get(key);if(previous&&previous!==row.revision&&["succeeded","failed","cancelled"].includes(row.phase))notifications.push(`${row.label}: ${phases[row.phase]}`);seen.current.set(key,row.revision);}
        });
        while(seen.current.size>512)seen.current.delete(seen.current.keys().next().value!);
        setSources(next);if(notifications.length)setNotice(notifications.slice(0,3).join(" · "));
      }
      inFlight=false;if(active)timer=setTimeout(()=>void refresh(),2500);
    };
    const visible=()=>{if(!document.hidden){if(timer)clearTimeout(timer);void refresh();}};
    document.addEventListener("visibilitychange",visible);void refresh();
    return()=>{active=false;if(timer)clearTimeout(timer);document.removeEventListener("visibilitychange",visible);};
  },[call,description.product.id,revision]);
  const review=async(row:Row)=>{
    if(busy)return;setBusy(true);setIssue("");
    try{await call({kind:"reviewOperation",product:row.owner,id:row.id,revision:row.revision,operationId:crypto.randomUUID()});setNotice("담당 제품에 화면 열기 검토를 요청했습니다.");}
    catch{setIssue("작업 상태가 바뀌었거나 제품 연결을 확인하지 못했습니다. 새로고침 후 다시 확인해 주세요.");}
    finally{setBusy(false);}
  };
  return <section aria-label="작업 상태">
    <h2>작업 상태</h2><p>각 제품이 보고한 작업입니다. 승인과 취소는 담당 화면에서 확인해 주세요.</p>
    <button onClick={()=>setRevision(value=>value+1)}>새로고침</button>
    {!nativeMode&&<p>데스크톱 제품에서 작업 상태를 확인할 수 있습니다.</p>}
    {catalog.products.filter(product=>description.product.id==="control-center"||product.id===description.product.id).map(product=>{
      const source=sources[product.id];return <section key={product.id} aria-label={product.label}><h3>{product.label}</h3>
        {!source?<p role="status">확인 중…</p>:source.unavailable?<p>상태를 확인할 수 없습니다. 제품 실행 상태와 승인한 연결을 확인해 주세요.</p>:!source.rows.length?<p>현재 보고된 작업이 없습니다.</p>:<ul>{source.rows.map(row=><li key={row.id}><strong>{row.label}</strong> · {phases[row.phase]} <small>{row.id.slice(0,8)}</small> <button disabled={busy} onClick={()=>void review(row)}>담당 화면 열기</button></li>)}</ul>}
      </section>;
    })}
    <p role="status" aria-live="polite">{notice}</p>{issue&&<p role="alert">{issue}</p>}
  </section>;
}
