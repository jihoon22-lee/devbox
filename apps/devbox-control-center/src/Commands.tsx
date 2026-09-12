import {useEffect,useRef,useState} from "react";
import type {ShellContentProps} from "@devbox/product-shell";
import {searchCommands,searchCommandSource,previewCommand,openCommand,commandStatus,type CommandReceipt,type CommandSearch,type Command} from "@devbox/product-shell/commands";

const providers=["workspace","api-studio","knowledge"] as const;
const reasons:Record<string,string>={notInstalled:"제품 설치 필요",versionMismatch:"제품 버전 확인 필요",providerUnavailable:"제품 연결 확인 필요",contextRequired:"프로젝트 선택 필요",selectionRequired:"선택한 내용 필요",stale:"원본 새로 고침 필요",permissionDenied:"접근 권한 확인 필요"};
export default function Commands({description,route,navigate}:ShellContentProps){
  const [query,setQuery]=useState(""),[result,setResult]=useState<CommandSearch|null>(null);
  const [loading,setLoading]=useState(false),[busy,setBusy]=useState(false),[issue,setIssue]=useState("");
  const [sources,setSources]=useState<Record<string,{result?:CommandSearch;issue?:boolean}>>({});
  const [delivery,setDelivery]=useState<{product:string;receipt:CommandReceipt|{operationId:string;phase:"unknown"}}|null>(null);
  const generation=useRef(0);
  useEffect(()=>{
    const ticket=++generation.current;setLoading(true);
    setSources({});
    const timer=setTimeout(()=>{
      void searchCommands(description,route,query).then(value=>{
        if(ticket===generation.current){setResult(value);setIssue("");}
      }).catch(()=>{if(ticket===generation.current){setResult(null);setIssue("명령 목록을 읽지 못했습니다.");}})
        .finally(()=>{if(ticket===generation.current)setLoading(false);});
      for(const product of providers){
        void searchCommandSource(description,route,product,query,ticket).then(result=>{
          if(ticket===generation.current)setSources(values=>({...values,[product]:{result}}));
        }).catch(()=>{if(ticket===generation.current)setSources(values=>({...values,[product]:{issue:true}}));});
      }
    },150);
    return()=>{clearTimeout(timer);generation.current++;};
  },[description,route,query]);
  const open=async(item:Command)=>{
    if(busy||item.disabledReason)return;
    const ticket=generation.current;setBusy(true);setIssue("");
    try{
      const operationId=crypto.randomUUID();
      const current=await previewCommand(description,route,item,operationId);
      if(ticket!==generation.current)return;
      if(current.owner===description.product.id&&current.target.kind==="route"&&!current.destructive)
        navigate(current.target.route);
      else {
        setDelivery({product:current.owner,receipt:{operationId,phase:"unknown"}});
        const receipt=await openCommand(description,route,current,operationId);
        setDelivery({product:current.owner,receipt});
      }
    }catch{if(ticket===generation.current)setIssue("명령의 현재 상태를 확인하지 못했습니다. 목록을 새로 검색해 주세요.");}
    finally{setBusy(false);}
  };
  const items=[...(result?.results??[]).filter(item=>!sources[item.owner]?.result),...Object.values(sources).flatMap(source=>source.result?.results??[])].slice(0,256);
  const deliveryText=delivery?{unknown:"요청이 전달되었는지 아직 확인하지 못했습니다.",awaitingReview:"요청한 제품에서 화면 열기를 확인해 주세요.",opening:"요청한 제품에서 화면을 열고 있습니다.",opened:"요청한 제품에서 화면을 열었습니다.",rejected:"요청한 제품에서 열기를 거절했습니다.",expired:"화면 열기 요청이 만료되었습니다."}[delivery.receipt.phase]:"";
  return <section className="command-browser">
    <h1>제품 명령</h1>
    <label>명령 검색 <input type="search" value={query} maxLength={512} onChange={event=>setQuery(event.target.value)}/></label>
    {loading&&<p role="status">명령을 찾고 있습니다…</p>}
    {issue&&<p role="alert">{issue}</p>}
    {delivery&&<p role="status">{deliveryText} <button disabled={busy} onClick={()=>void commandStatus(description,route,delivery.product,delivery.receipt.operationId).then(receipt=>setDelivery({product:delivery.product,receipt})).catch(()=>setIssue("요청 결과를 확인하지 못했습니다. 제품 연결을 확인한 뒤 상태를 다시 확인해 주세요."))}>전달 상태 확인</button></p>}
    {providers.filter(product=>sources[product]?.issue).map(product=><p key={product}>{product}: 연결된 제품의 검색을 사용할 수 없습니다.</p>)}
    <ul>{items.map(item=><li key={item.id}>
      <button disabled={busy||loading||Boolean(item.disabledReason)} onClick={()=>void open(item)}>{item.label}</button>
      <small>{item.owner}{item.requiresReview?" · 실행 전 검토":""}{item.disabledReason?" · "+(reasons[item.disabledReason]??"현재 사용할 수 없음"):""}</small>
    </li>)}</ul>
    {(result?.truncated||Object.values(sources).some(source=>source.result?.truncated))&&<p>일부 결과만 표시했습니다. 검색어를 더 구체적으로 입력해 주세요.</p>}
    {result&&!loading&&!items.length&&<p>일치하는 명령이 없습니다.</p>}
  </section>;
}
