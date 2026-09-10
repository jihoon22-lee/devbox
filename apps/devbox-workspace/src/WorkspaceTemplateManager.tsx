import {useEffect, useRef, useState} from "react";
import {emptyProfileTemplateDraft, templateDraftFromTemplate, validateProfileTemplateDraft, type ProfileTemplateDraft} from "@devbox/workspace-features/overview-template-editor";
import {nativeCall} from "./native";
import {TemplateMetadata, type ImportedTemplate} from "./LegacyTemplateImport";
import type {Registry} from "./RegistryGate";

type Edit = {id:string|null; revision:number; draft:ProfileTemplateDraft};
const fields: [keyof Omit<ProfileTemplateDraft,"id">,string][] = [
  ["name","템플릿 이름"], ["windowsPath","기본 Windows 폴더"],
  ["wslDistro","기본 WSL 배포판"], ["wslPath","기본 WSL 폴더"],
  ["gitRoot","기본 Git 폴더"], ["expectedPortsText","기본 포트 (쉼표로 구분)"],
  ["serviceIdsText","서비스 참조 (쉼표로 구분)"],
];
export default function WorkspaceTemplateManager({registry,disabled,onSaved,onBusyChange}:{registry:Registry;disabled:boolean;onSaved:()=>Promise<void>;onBusyChange:(busy:boolean)=>void}) {
  const [edit,setEdit]=useState<Edit|null>(null);
  const [archive,setArchive]=useState<{entry:ImportedTemplate;revision:number}|null>(null);
  const [busy,setBusy]=useState(false),[error,setError]=useState("");
  const alive=useRef(false),running=useRef(false);
  useEffect(()=>{alive.current=true;return()=>{alive.current=false;onBusyChange(false);};},[onBusyChange]);
  useEffect(()=>{onBusyChange(busy||!!edit||!!archive);},[busy,edit,archive,onBusyChange]);
  async function act(action:()=>Promise<void>) {
    if(running.current||disabled)return;
    running.current=true;setBusy(true);setError("");
    try {await action();}catch(cause){if(alive.current)setError(cause instanceof Error?cause.message:"템플릿을 저장하지 못했습니다.");}
    finally {running.current=false;if(alive.current)setBusy(false);}
  }
  return <section aria-label="템플릿 관리" aria-busy={busy}>
    <h2>템플릿 관리</h2>
    <p>새 프로젝트에 사용할 기본값입니다. 수정하거나 보관해도 이미 만든 프로젝트 설정은 유지됩니다.</p>
    {error&&<p role="alert">{error}</p>}
    <button disabled={disabled||busy||!!edit||!!archive} onClick={()=>{setError("");setEdit({id:null,revision:registry.revision,draft:emptyProfileTemplateDraft()});}}>새 템플릿</button>
    {(registry.importedTemplates??[]).map(entry=><details key={entry.id}>
      <summary>{entry.template.name}{entry.archived?" · 보관됨":""}</summary>
      <TemplateMetadata template={entry.template}/>
      <button disabled={disabled||busy||!!edit||!!archive} onClick={()=>{setError("");setEdit({id:entry.id,revision:registry.revision,draft:templateDraftFromTemplate(entry.template)});}}>{entry.archived?"템플릿 복원 검토":"템플릿 수정"}</button>
      {!entry.archived&&<button disabled={disabled||busy||!!edit||!!archive} onClick={()=>{setError("");setArchive({entry,revision:registry.revision});}}>템플릿 보관</button>}
    </details>)}
    {edit&&<form aria-label="템플릿 편집" onSubmit={event=>{event.preventDefault();const result=validateProfileTemplateDraft(edit.draft);
      if(!result.template){setError(Object.values(result.errors).filter(Boolean).join(" "));return;}
      void act(async()=>{
        await nativeCall("workspace.registry","save_template",{revision:edit.revision,id:edit.id,template:result.template});
        if(alive.current)setEdit(null);
        await onSaved();
      });
    }}>
      {fields.map(([field,label])=><label key={field}>{label}<input value={edit.draft[field]} disabled={disabled||busy} maxLength={field==="name"?120:32768} required={field==="name"} onChange={event=>setEdit({...edit,draft:{...edit.draft,[field]:event.target.value}})}/></label>)}
      <p>폴더는 기본값으로 보관합니다. 프로젝트 생성 시 실제 폴더를 별도로 확인합니다.</p>
      <button disabled={disabled||busy}>템플릿 저장</button>
      <button type="button" disabled={busy} onClick={()=>{setEdit(null);setError("");}}>템플릿 편집 취소</button>
    </form>}
    {archive&&<section aria-label="템플릿 보관 확인">
      <p>{archive.entry.template.name}을 새 프로젝트 선택 목록에서 숨깁니다. 기존 프로젝트와 템플릿 내용은 유지하며 나중에 복원할 수 있습니다.</p>
      <button disabled={disabled||busy} onClick={()=>void act(async()=>{
        await nativeCall("workspace.registry","archive_template",{revision:archive.revision,id:archive.entry.id});
        if(alive.current)setArchive(null);
        await onSaved();
      })}>템플릿 보관 확인</button>
      <button disabled={busy} onClick={()=>{setArchive(null);setError("");}}>템플릿 보관 취소</button>
    </section>}
  </section>;
}
