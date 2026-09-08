import {useEffect,useRef,useState} from "react";
import type {Description} from "@devbox/product-shell/api";
import {componentCall,nativeCall} from "./native";

type Target="project"|"local";
interface Preview {
  previewId:string;target:Target;before:Record<string,unknown>;after:Record<string,unknown>;
  effectiveDiff:{added:string[];changed:string[];removed:string[];expectedPortsChanged:boolean};
}
export interface EditableDefinitions {project:Record<string,unknown>;local:Record<string,unknown>;editRevision:string}
interface Props {description:Description;view:EditableDefinitions;disabled:boolean;onDirtyChange:(dirty:boolean)=>void;onSaved:()=>void}
const label=(target:Target)=>target==="project"?"공유 프로젝트 정의":"이 컴퓨터의 설정";
export default function DefinitionEditor({description,view,disabled,onDirtyChange,onSaved}:Props) {
  const [target,setTarget]=useState<Target>("project");
  const [draft,setDraft]=useState<string|null>(null);
  const [base,setBase]=useState("");
  const [preview,setPreview]=useState<Preview|null>(null);
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState("");
  const [status,setStatus]=useState("");
  const sequence=useRef(0),pending=useRef<string|null>(null),working=useRef(false);
  const call=<T,>(method:string,args:Record<string,unknown>)=>componentCall<T>(description,"workspace.definitions",method,args,"overview");
  useEffect(()=>{onDirtyChange(draft!==null||busy);},[draft,busy,onDirtyChange]);
  useEffect(()=>()=>{
    sequence.current++;onDirtyChange(false);
    if(pending.current)void nativeCall("workspace.definitions","cancel",{previewId:pending.current}).catch(()=>{});
  },[onDirtyChange]);
  async function act(action:(request:number)=>Promise<void>) {
    if(working.current)return;working.current=true;setBusy(true);setError("");setStatus("");
    const request=++sequence.current;
    try{await action(request);}catch(cause){if(sequence.current===request)setError(cause instanceof Error?cause.message:"설정을 저장하지 못했습니다.");}
    finally{if(sequence.current===request){working.current=false;setBusy(false);}}
  }
  function begin(next:Target){setTarget(next);setBase(view.editRevision);setDraft(JSON.stringify(view[next],null,2));setStatus("");setError("");}
  function exportProject(){
    const url=URL.createObjectURL(new Blob([JSON.stringify(view.project,null,2)+"\n"],{type:"application/json"}));
    const link=document.createElement("a");link.href=url;link.download="project.json";link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);
  }
  return <section className="workspace-definition-editor" aria-label="프로젝트 설정 편집" aria-busy={busy}>
    {error&&<p role="alert">{error}</p>}{status&&<p role="status">{status}</p>}
    {draft===null?<>
      <button type="button" disabled={disabled||busy} onClick={()=>begin("project")}>공유 정의 편집</button>
      <button type="button" disabled={disabled||busy} onClick={()=>begin("local")}>이 컴퓨터 설정 편집</button>
      <button type="button" disabled={disabled||busy} onClick={exportProject}>공유 정의 내보내기</button>
    </>:<>
      <h3>{label(target)} 편집</h3>
      <p>{target==="project"?"저장 위치: .devbox/project.json. 공유할 정의만 입력하세요.":"설정은 이 컴퓨터에만 저장됩니다. 비밀 및 API 환경 연결은 기존 참조를 유지하세요."}</p>
      {!preview?<>
        <label>설정 JSON<textarea spellCheck={false} disabled={disabled||busy} value={draft} onChange={event=>setDraft(event.target.value)}/></label>
        <label>JSON 파일 가져오기<input type="file" accept=".json,application/json" disabled={disabled||busy} onChange={event=>{
          const file=event.target.files?.[0];event.target.value="";if(!file)return;
          void act(async request=>{
            if(file.size>256*1024)throw new Error("설정 파일은 256 KiB 이하여야 합니다.");
            const content=await file.text();if(sequence.current===request)setDraft(content);
          });
        }}/></label>
        <button type="button" disabled={disabled||busy} onClick={()=>void act(async request=>{
          const next=await call<Preview>("preview_edit",{target,content:draft,editRevision:base});
          if(sequence.current!==request){void nativeCall("workspace.definitions","cancel",{previewId:next.previewId}).catch(()=>{});return;}
          pending.current=next.previewId;setPreview(next);
        })}>저장 변경 검토</button>
        <button type="button" disabled={disabled||busy} onClick={()=>{setDraft(null);setError("");}}>편집 취소</button>
      </>:<section aria-label="설정 저장 확인">
        <h4>{label(preview.target)} 저장 전후</h4>
        <div className="workspace-definition-comparison"><div><h5>저장 전</h5><pre>{JSON.stringify(preview.before,null,2)}</pre></div><div><h5>저장 후</h5><pre>{JSON.stringify(preview.after,null,2)}</pre></div></div>
        <p>적용 설정의 추가: {preview.effectiveDiff.added.join(", ")||"없음"} · 변경: {preview.effectiveDiff.changed.join(", ")||"없음"} · 삭제: {preview.effectiveDiff.removed.join(", ")||"없음"}{preview.effectiveDiff.expectedPortsChanged?" · 예상 포트 변경":""}</p>
        <p>저장하면 기존 실행 승인이 철회됩니다. 저장 후 실행 정의를 다시 검토하세요.</p>
        <button type="button" disabled={disabled||busy} onClick={()=>void act(async request=>{
          const token=preview.previewId;pending.current=null;setPreview(null);
          try{
            const saved=await call<{saved:boolean;warning:string|null}>("apply_edit",{previewId:token});
            if(sequence.current!==request)return;
            setDraft(null);setStatus(saved.warning?"저장했습니다. 디스크 반영 상태를 다시 확인해 주세요.":"설정을 저장했습니다.");
          }finally{if(sequence.current===request)onSaved();}
        })}>검토한 설정 저장</button>
        <button type="button" disabled={disabled||busy} onClick={()=>void act(async request=>{
          await call("cancel",{previewId:preview.previewId});if(sequence.current!==request)return;pending.current=null;setPreview(null);
        })}>저장 검토 취소</button>
      </section>}
    </>}
  </section>;
}
