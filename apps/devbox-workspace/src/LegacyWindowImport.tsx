import {useEffect,useRef,useState} from "react";
import {nativeCall,issueMessage} from "./native";
export type WindowState={schemaVersion:number;bounds:{x:number;y:number;width:number;height:number};monitorId:string;monitorWorkArea:{x:number;y:number;width:number;height:number};scaleFactor:number;maximized:boolean};
type Preview={previewId:string;before:WindowState;after:WindowState;source:WindowState;restoring:boolean};
type History={items:{id:string;state:WindowState|null;issue:string|null}[];unrecognized:number};
const call=<T,>(method:string,args:Record<string,unknown>={})=>nativeCall<T>("workspace.migration",method,args);
const describe=(state:WindowState)=>`${state.bounds.width} × ${state.bounds.height} · 위치 ${state.bounds.x}, ${state.bounds.y} · 배율 ${Math.round(state.scaleFactor*100)}%${state.maximized?" · 최대화":""}`;
export default function LegacyWindowImport({jobId,disabled=false,onBusyChange}:{jobId?:string;disabled?:boolean;onBusyChange:(busy:boolean)=>void}) {
  const [preview,setPreview]=useState<Preview|null>(null),[history,setHistory]=useState<History>({items:[],unrecognized:0});
  const [replace,setReplace]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState(""),[status,setStatus]=useState("");
  const alive=useRef(false),token=useRef<string|null>(null),epoch=useRef(0),busyChange=useRef(onBusyChange);busyChange.current=onBusyChange;
  useEffect(()=>{alive.current=true;return()=>{alive.current=false;epoch.current++;if(token.current)void call("cancel_window_import",{previewId:token.current}).catch(()=>{});busyChange.current(false);};},[]);
  useEffect(()=>{const previous=token.current;token.current=null;epoch.current++;setPreview(null);setReplace(false);setStatus("");setBusy(false);busyChange.current(false);if(previous)void call("cancel_window_import",{previewId:previous}).catch(()=>{});},[jobId]);
  useEffect(()=>{let disposed=false;const current=epoch.current;void call<History>("list_window_history").then(value=>{if(!disposed&&epoch.current===current)setHistory(value);}).catch(cause=>{if(!disposed&&epoch.current===current)setError(cause instanceof Error?cause.message:"창 위치 복원 기록을 읽지 못했습니다.");});return()=>{disposed=true;};},[jobId]);
  async function act(method:string,args:Record<string,unknown>={}) {
    if(busy||disabled)return;
    setBusy(true);busyChange.current(true);setError("");setStatus("");const current=++epoch.current;
    try {
      if(method==="list_window_history"){const value=await call<History>(method);if(alive.current&&epoch.current===current)setHistory(value);return;}
      if(method.startsWith("preview_")&&token.current){const previous=token.current;token.current=null;setPreview(null);await call("cancel_window_import",{previewId:previous});}
      const result=await call<Preview>(method,args);
      if(!alive.current||epoch.current!==current){if(method.startsWith("preview_")&&result?.previewId)void call("cancel_window_import",{previewId:result.previewId}).catch(()=>{});return;}
      if(method.startsWith("preview_")){token.current=result.previewId;setPreview(result);setReplace(false);}
      else {token.current=null;setPreview(null);setReplace(false);if(method==="apply_window_import")setStatus("검토한 창 위치와 크기를 적용했습니다.");}
    } catch(cause){if(alive.current&&epoch.current===current){setError(cause instanceof Error?cause.message:"창 상태를 변경하지 못했습니다. 이전 창 상태를 확인해 주세요.");if(method==="apply_window_import"||method==="cancel_window_import"){token.current=null;setPreview(null);setReplace(false);}}}
    finally {
      if(alive.current&&epoch.current===current){
        if(method!=="list_window_history")try {const next=await call<History>("list_window_history");if(alive.current&&epoch.current===current)setHistory(next);}catch{ /* Preserve the action's error and the last verified list. */ }
        if(alive.current&&epoch.current===current){setBusy(false);busyChange.current(false);}
      }
    }
  }
  return <section aria-label="기존 창 상태 가져오기" aria-busy={busy}>
    <h3>창 위치와 크기</h3>
    <p>기존 기본 창의 위치와 크기를 현재 화면 배율에 맞춰 검토합니다. 적용 전 현재 창 상태를 보관합니다.</p>
    <button disabled={disabled||busy} onClick={()=>void act("list_window_history")}>이전 창 상태 새로 고침</button>
    {jobId&&<button disabled={disabled||busy} onClick={()=>void act("preview_window_import",{jobId})}>기존 창 상태 검토</button>}
    {preview&&<section aria-label="창 상태 변경 확인">
      <dl><dt>현재 창</dt><dd>{describe(preview.before)}</dd><dt>{preview.restoring?"복원할 창":"가져올 창"}</dt><dd>{describe(preview.after)}</dd></dl>
      <p>창을 이동하거나 화면 배율을 바꾸면 다시 검토해야 합니다.</p>
      <label><input type="checkbox" checked={replace} disabled={busy||disabled} onChange={event=>setReplace(event.target.checked)}/>현재 창 위치와 크기 변경</label>
      <button disabled={busy||disabled||!replace} onClick={()=>void act("apply_window_import",{previewId:preview.previewId,replaceExisting:true})}>창 상태 적용</button>
      <button disabled={busy||disabled} onClick={()=>void act("cancel_window_import",{previewId:preview.previewId})}>창 상태 검토 취소</button>
    </section>}
    {history.items.length>0&&<section aria-label="이전 창 상태"><h4>이전 창 상태</h4><ul>{history.items.map((item,index)=><li key={item.id}>{item.state?describe(item.state):issueMessage(item.issue??"window_history_unavailable")} <button disabled={busy||disabled||!!item.issue||!item.state} aria-label={`이전 창 상태 ${index+1} 복원 검토`} onClick={()=>void act("preview_window_restore",{backupId:item.id})}>복원 검토</button></li>)}</ul></section>}
    {history.unrecognized>0&&<p>확인할 수 없는 창 상태 기록 {history.unrecognized}개를 유지했습니다.</p>}
    {error&&<p role="alert">{error}</p>}{status&&<p role="status">{status}</p>}
  </section>;
}
