import { lazy, Suspense, useEffect, useRef, useState } from "react";
import type { ProjectContext } from "@devbox/product-shell/api";
import { nativeCall, issueMessage } from "./native";
const WorkspaceTemplateManager=lazy(()=>import("./WorkspaceTemplateManager"));
import LegacyImports from "./LegacyImports";
import {TemplateMetadata,type ImportedTemplate} from "./LegacyTemplateImport";
import {ProfileMetadata,type ImportedProfile,type ProfileBinding} from "./LegacyProfileImport";

type Status = {phase: "loading" | "setup" | "selected" | "failed"; issue?: string};
export interface Worktree {id: string; projectId: string; revision: number; binding: {root: string; target: ProjectContext["target"]}; trustedDigest: string | null}
export interface Registry {revision: number; projects: {id: string; name: string}[]; worktrees: Worktree[];importedProfiles?:ImportedProfile[];importedTemplates?:ImportedTemplate[];importedProfileBindings?:ProfileBinding[]}
interface Preview {previewId: string; binding: Worktree["binding"]; importedProfileId?:string|null;templateProfile?:ImportedProfile|null;discovery: {kind: "known" | "newProject" | "linkedWorktree" | "aliasOrMove" | "replacedRoot"}}
const registryCall = <T,>(method: string, args: Record<string, unknown> = {}) => nativeCall<T>("workspace.registry", method, args);
const discoveryLabels = {known: "이미 등록한 폴더입니다.", newProject: "새 프로젝트로 등록합니다.", linkedWorktree: "기존 프로젝트의 연결된 작업 폴더입니다.", aliasOrMove: "기존 프로젝트의 경로가 변경되었습니다.", replacedRoot: "등록된 경로의 폴더가 교체되었습니다."};

export default function RegistryGate({context = null, onContextChanged = async () => {}, onReady, editing = false, refreshSignal=0, onSnapshot, suggestedRoot}: {context?: ProjectContext | null; onContextChanged?: () => Promise<void>; onReady?: () => void; editing?: boolean; refreshSignal?:number; onSnapshot?: (registry: Registry) => void; suggestedRoot?: {id:string;path:string;name:string}|null}) {
  const [status, setStatus] = useState<Status>({phase:"loading"});
  const [registry, setRegistry] = useState<Registry | null>(null);
  const [templateId,setTemplateId]=useState("");
  const [root, setRoot] = useState("");
  const [name, setName] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [operationBusy, setBusy] = useState(false);
  const [templateBusy,setTemplateBusy]=useState(false);
  const [templatesOpen,setTemplatesOpen]=useState(false);
  const busy=operationBusy||templateBusy;
  const [error, setError] = useState("");
  const [rename, setRename] = useState<{id: string; name: string} | null>(null);
  const [remove, setRemove] = useState<Worktree | null>(null);
  const [unlinkProfile,setUnlinkProfile]=useState<ProfileBinding|null>(null);
  const alive = useRef(true);
  const currentPreview = useRef<string | null>(null);
  const loadId = useRef(0);
  const handledSuggestion=useRef<string|null>(null);
  useEffect(() => {if (registry) onSnapshot?.(registry);}, [registry, onSnapshot]);
  useEffect(() => {if (status.phase === "selected") onReady?.();}, [status.phase, onReady]);
  async function refresh() {
    const requestId = ++loadId.current;
    const next = await nativeCall<Status>("workspace.migration", "status");
    if (!alive.current || loadId.current !== requestId) return;
    setStatus(next);
    if (next.phase === "selected") {
      const snapshot = await registryCall<Registry>("snapshot");
      if (alive.current && loadId.current === requestId) setRegistry(snapshot);
    }
  }
  useEffect(() => {if (refreshSignal) void refresh().catch(cause => setError(cause instanceof Error ? cause.message : "목록을 확인하지 못했습니다."));}, [refreshSignal]);
  useEffect(() => {
    alive.current = true;
    let timer: ReturnType<typeof setTimeout>;
    let disposed = false;
    async function load() {
      const requestId = ++loadId.current;
      try {
        const next = await nativeCall<Status>("workspace.migration", "status");
        if (disposed || !alive.current || loadId.current !== requestId) return;
        setStatus(next);
        if (next.phase === "loading") timer = setTimeout(() => { void load(); }, 300);
        if (next.phase === "selected") {
          const snapshot = await registryCall<Registry>("snapshot");
          if (!disposed && alive.current && loadId.current === requestId) setRegistry(snapshot);
        }
      } catch (cause) {if (!disposed && alive.current && loadId.current === requestId) setError(cause instanceof Error ? cause.message : "정보를 불러오지 못했습니다.");}
    }
    void load();
    return () => {
      disposed = true; alive.current = false; loadId.current += 1; clearTimeout(timer);
      if (currentPreview.current) void registryCall("cancel_registration", {previewId:currentPreview.current}).catch(() => {});
    };
  }, []);
  async function act(action: () => Promise<void>) {
    if (busy) return;
    setBusy(true); setError("");
    try {await action();} catch (cause) {if (alive.current) setError(cause instanceof Error ? cause.message : "작업을 완료하지 못했습니다.");}
    finally {if (alive.current) setBusy(false);}
  }
  useEffect(()=>{
    if(!suggestedRoot || handledSuggestion.current===suggestedRoot.id || busy || status.phase!=="selected")return;
    handledSuggestion.current=suggestedRoot.id;
    void act(async()=>{
      if(preview)await cancelPreview();
      setRoot(suggestedRoot.path);setName(suggestedRoot.name);setTemplateId("");
      const next=await registryCall<Preview>("preview_windows",{root:suggestedRoot.path});
      if(!alive.current){await registryCall("cancel_registration",{previewId:next.previewId});return;}
      currentPreview.current=next.previewId;setPreview(next);
      document.getElementById("workspace-project-path")?.scrollIntoView?.({block:"nearest"});
    });
    // A user clicked a Source proposal. Native Registry still validates its path.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  },[suggestedRoot,busy,status.phase]);
  useEffect(()=>{if(templateId&&!registry?.importedTemplates?.some(entry=>entry.id===templateId&&!entry.archived))setTemplateId("");},[registry,templateId]);
  async function cancelPreview() {
    if (!preview) return;
    await registryCall("cancel_registration", {previewId:preview.previewId});
    currentPreview.current = null; setPreview(null);
  }
  return <section className="workspace-registry" aria-label="프로젝트 관리" aria-busy={busy}>
    <h1>프로젝트</h1>
    {error && <p role="alert">{error}</p>}
    {status.phase === "loading" && <p role="status">저장된 정보를 불러오고 있습니다…</p>}
    {status.phase === "failed" && <p role="alert">{issueMessage(status.issue ?? "operation_failed")}</p>}
    {status.phase === "setup" && <>
      <p>Workspace에서 사용할 프로젝트 목록을 시작합니다.</p>
      <button disabled={busy} onClick={() => void act(async () => {await nativeCall("workspace.migration", "start_empty"); await refresh();})}>빈 Workspace 시작</button>
    </>}
    {status.phase === "selected" && <>
      {editing && <p role="status">편집 내용을 저장하거나 진행 중인 작업을 마친 뒤 프로젝트를 변경할 수 있습니다.</p>}
      {context && <p>현재 선택한 작업 폴더: {registry?.worktrees.find(tree => tree.id === context.worktreeId)?.binding.root ?? "목록 확인 중"} <button disabled={busy || editing} onClick={() => void act(async () => {await registryCall("clear_project"); await onContextChanged();})}>프로젝트 선택 해제</button></p>}
      <button disabled={busy} onClick={() => void act(refresh)}>목록 새로 고침</button>
      <form onSubmit={event => {event.preventDefault(); void act(async () => {
        if (preview) await cancelPreview();
        const next = templateId?await registryCall<Preview>("preview_template_profile_windows",{templateId,root,name:name||registry?.importedTemplates?.find(template=>template.id===templateId)?.template.name||"새 프로젝트"}):await registryCall<Preview>("preview_windows", {root});
        if (!alive.current) {await registryCall("cancel_registration", {previewId:next.previewId}); return;}
        if(next.templateProfile)setName(next.templateProfile.profile.name);
        currentPreview.current = next.previewId; setPreview(next);
      });}}>
        {!!registry?.importedTemplates?.length&&<label>프로젝트 템플릿 <select aria-label="프로젝트 템플릿" value={templateId} disabled={busy||!!preview} onChange={event=>setTemplateId(event.target.value)}><option value="">사용하지 않음</option>{registry.importedTemplates.filter(entry=>!entry.archived).map((imported,index)=><option key={imported.id} value={imported.id}>{imported.template.name}{registry.importedTemplates!.filter(entry=>entry.template.name===imported.template.name).length>1?` · 보관 항목 ${index+1}`:""}</option>)}</select></label>}
        {registry?.importedTemplates?.find(imported=>imported.id===templateId)&&<TemplateMetadata template={registry.importedTemplates.find(imported=>imported.id===templateId)!.template}/>}
        <label htmlFor="workspace-project-path">Windows 프로젝트 폴더</label>
        <input id="workspace-project-path" value={root} maxLength={32768} disabled={busy || !!preview} onChange={event => setRoot(event.target.value)} required />
        <button disabled={busy || !root.trim() || !!preview}>폴더 확인</button>
      </form>
      {preview && <section aria-label="프로젝트 등록 확인">
        <h2>등록 확인</h2><p>{discoveryLabels[preview.discovery.kind]}</p><p>{preview.binding.root}</p>
        <p>명령 실행에 대한 신뢰는 별도로 확인합니다.</p>
        {preview.templateProfile&&<><p>선택한 템플릿으로 아래 프로필을 만들고 이 폴더에 연결합니다.</p><ProfileMetadata profile={preview.templateProfile.profile}/></>}
        {preview.importedProfileId&&<p>가져온 프로필을 이 폴더에 연결합니다. 프로필의 포트 설정은 프로젝트·로컬 설정이 없는 경우 기본값으로 사용합니다.</p>}
        <label htmlFor="workspace-project-name">프로젝트 이름</label>
        <input id="workspace-project-name" value={name} maxLength={120} disabled={busy} onChange={event => setName(event.target.value)} />
        <button disabled={busy || !name.trim() || (editing && ["aliasOrMove","replacedRoot"].includes(preview.discovery.kind))} onClick={() => void act(async () => {
          await registryCall("apply_registration", {previewId:preview.previewId, name, action:["aliasOrMove","replacedRoot"].includes(preview.discovery.kind) ? "rebind" : "register"});
          currentPreview.current = null; setPreview(null); setRoot(""); setName(""); setTemplateId(""); await refresh();
        })}>{["aliasOrMove","replacedRoot"].includes(preview.discovery.kind) ? "경로 다시 연결" : "등록"}</button>
        <button disabled={busy} onClick={() => void act(cancelPreview)}>취소</button>
      </section>}
      {registry?.projects.length === 0 && <p>등록한 프로젝트가 없습니다.</p>}
      {registry?.projects.map(project => <section key={project.id} aria-label={project.name}>
        <h2>{project.name}</h2>
        <button disabled={busy} onClick={() => setRename({id:project.id,name:project.name})}>이름 변경</button>
        {rename?.id === project.id && <form onSubmit={event => {event.preventDefault(); void act(async () => {
          await registryCall("rename", {revision:registry.revision,projectId:project.id,name:rename.name}); setRename(null); await refresh();
        });}}><label htmlFor="workspace-rename">새 이름</label><input id="workspace-rename" required maxLength={120} value={rename.name} disabled={busy} onChange={event => setRename({...rename,name:event.target.value})}/><button disabled={busy}>저장</button><button type="button" disabled={busy} onClick={() => setRename(null)}>취소</button></form>}
        {registry.worktrees.filter(worktree => worktree.projectId === project.id).map(worktree => <div key={worktree.id}>
          <p>{worktree.binding.root}</p><p>{worktree.trustedDigest ? "실행 정의 검토 기록이 있습니다. 현재 상태는 프로젝트 설정에서 확인하세요." : "실행 신뢰 확인 전"}</p>
          <button disabled={busy || editing || (context?.worktreeId === worktree.id && context.revision === worktree.revision)} onClick={() => void act(async () => {
            const next: ProjectContext = {projectId:worktree.projectId,worktreeId:worktree.id,revision:worktree.revision,target:worktree.binding.target};
            await registryCall("select_project", {context:next}); await onContextChanged();
          })}>프로젝트 선택</button>
          <button disabled={busy || editing} onClick={() => setRemove(worktree)}>등록 해제</button>
          {remove?.id === worktree.id && <section aria-label="등록 해제 확인"><p>이 작업 폴더의 등록을 해제합니다. 실제 폴더와 Git 파일은 보존됩니다.</p>
            <button disabled={busy || editing} onClick={() => void act(async () => {
              const context: ProjectContext = {projectId:worktree.projectId,worktreeId:worktree.id,revision:worktree.revision,target:worktree.binding.target};
              await registryCall("remove", {revision:registry.revision,context}); setRemove(null); await refresh();
            })}>해제 확인</button><button disabled={busy} onClick={() => setRemove(null)}>취소</button></section>}
        </div>)}
      </section>)}
      {registry&&<>
        <button aria-expanded={templatesOpen} disabled={busy||editing||!!preview} onClick={()=>setTemplatesOpen(value=>!value)}>{templatesOpen?"템플릿 관리 닫기":"템플릿 관리 열기"}</button>
        {templatesOpen&&<Suspense fallback={<p role="status">템플릿 관리 화면을 불러오고 있습니다…</p>}><WorkspaceTemplateManager registry={registry} disabled={operationBusy||editing||!!preview||!!rename||!!remove||!!unlinkProfile} onSaved={refresh} onBusyChange={setTemplateBusy}/></Suspense>}
      </>}
      {(registry?.importedProfiles??[]).map(imported=><section key={imported.id} aria-label={`${imported.local?"프로필":"가져온 프로필"} ${imported.profile.name}`}>
        <h2>{imported.local?"프로필":"가져온 프로필"}: {imported.profile.name}</h2>
        <ProfileMetadata profile={imported.profile}/>
        <p>환경 설정과 서비스 참조는 보관되었습니다. 실행 연결은 해당 기능에서 확인해야 합니다.</p>
        {!(registry?.importedProfileBindings??[]).some(binding=>binding.importedId===imported.id)&&<p>아직 프로젝트 폴더에 연결하지 않았습니다.</p>}
        <button disabled={busy||editing||!imported.profile.windowsPath} onClick={()=>void act(async()=>{
          if(preview)await cancelPreview();
          const next=await registryCall<Preview>("preview_imported_profile_windows",{importedId:imported.id});
          if(!alive.current){await registryCall("cancel_registration",{previewId:next.previewId});return;}
          currentPreview.current=next.previewId;setPreview(next);setRoot(next.binding.root);setName(imported.profile.name);setTemplateId("");
          document.getElementById("workspace-project-path")?.scrollIntoView?.({block:"nearest"});
        })}>Windows 폴더 연결 검토</button>
        {imported.profile.wsl&&<p>WSL 폴더는 보관되어 있으며 연결 기능을 준비 중입니다.</p>}
        {(registry?.importedProfileBindings??[]).filter(binding=>binding.importedId===imported.id).map(binding=><div key={binding.target}>
          <p>연결한 폴더: {registry?.worktrees.find(tree=>tree.id===binding.worktreeId)?.binding.root}</p>
          <button disabled={busy||editing} onClick={()=>setUnlinkProfile(binding)}>프로필 연결 해제</button>
          {unlinkProfile===binding&&<section aria-label="프로필 연결 해제 확인"><p>보관한 프로필과 프로젝트 등록을 유지하고 둘 사이의 연결을 해제합니다.</p>
            <button disabled={busy||editing} onClick={()=>void act(async()=>{await registryCall("unbind_imported_profile",{revision:registry!.revision,importedId:imported.id,target:binding.target});setUnlinkProfile(null);await refresh();})}>연결 해제 확인</button>
            <button disabled={busy} onClick={()=>setUnlinkProfile(null)}>취소</button>
          </section>}
        </div>)}
      </section>)}
    </>}
    {(status.phase==="setup"||status.phase==="selected")&&<LegacyImports selected={status.phase==="selected"} existingProfiles={registry?.importedProfiles??[]} existingTemplates={registry?.importedTemplates??[]} onImported={refresh} disabled={busy||editing} onWorkspaceReview={jobId=>void act(async()=>{
      if(editing)return;
      if(preview)await cancelPreview();
      const next=await registryCall<Preview>("preview_legacy_workspace_windows",{jobId});
      if(!alive.current){await registryCall("cancel_registration",{previewId:next.previewId});return;}
      currentPreview.current=next.previewId;setPreview(next);setRoot(next.binding.root);setName("Code Pad 작업 폴더");setTemplateId("");
      document.getElementById("workspace-project-path")?.scrollIntoView?.({block:"nearest"});
    })}/>}
  </section>;
}
