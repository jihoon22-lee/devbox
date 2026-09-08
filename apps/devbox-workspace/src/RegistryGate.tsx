import { useEffect, useRef, useState } from "react";
import type { ProjectContext } from "@devbox/product-shell/api";
import { nativeCall, issueMessage } from "./native";

type Status = {phase: "loading" | "setup" | "selected" | "failed"; issue?: string};
interface Worktree {id: string; projectId: string; revision: number; binding: {root: string; target: ProjectContext["target"]}; trustedDigest: string | null}
interface Registry {revision: number; projects: {id: string; name: string}[]; worktrees: Worktree[]}
interface Preview {previewId: string; binding: Worktree["binding"]; discovery: {kind: "known" | "newProject" | "linkedWorktree" | "aliasOrMove" | "replacedRoot"}}
const registryCall = <T,>(method: string, args: Record<string, unknown> = {}) => nativeCall<T>("workspace.registry", method, args);
const discoveryLabels = {known: "이미 등록한 폴더입니다.", newProject: "새 프로젝트로 등록합니다.", linkedWorktree: "기존 프로젝트의 연결된 작업 폴더입니다.", aliasOrMove: "기존 프로젝트의 경로가 변경되었습니다.", replacedRoot: "등록된 경로의 폴더가 교체되었습니다."};

export default function RegistryGate({context = null, onContextChanged = async () => {}}: {context?: ProjectContext | null; onContextChanged?: () => Promise<void>}) {
  const [status, setStatus] = useState<Status>({phase:"loading"});
  const [registry, setRegistry] = useState<Registry | null>(null);
  const [root, setRoot] = useState("");
  const [name, setName] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [rename, setRename] = useState<{id: string; name: string} | null>(null);
  const [remove, setRemove] = useState<Worktree | null>(null);
  const alive = useRef(true);
  const currentPreview = useRef<string | null>(null);
  const loadId = useRef(0);
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
      {context && <p>현재 선택한 작업 폴더: {registry?.worktrees.find(tree => tree.id === context.worktreeId)?.binding.root ?? "목록 확인 중"} <button disabled={busy} onClick={() => void act(async () => {await registryCall("clear_project"); await onContextChanged();})}>프로젝트 선택 해제</button></p>}
      <button disabled={busy} onClick={() => void act(refresh)}>목록 새로 고침</button>
      <form onSubmit={event => {event.preventDefault(); void act(async () => {
        if (preview) await cancelPreview();
        const next = await registryCall<Preview>("preview_windows", {root});
        if (!alive.current) {await registryCall("cancel_registration", {previewId:next.previewId}); return;}
        currentPreview.current = next.previewId; setPreview(next);
      });}}>
        <label htmlFor="workspace-project-path">Windows 프로젝트 폴더</label>
        <input id="workspace-project-path" value={root} maxLength={32768} disabled={busy || !!preview} onChange={event => setRoot(event.target.value)} required />
        <button disabled={busy || !root.trim() || !!preview}>폴더 확인</button>
      </form>
      {preview && <section aria-label="프로젝트 등록 확인">
        <h2>등록 확인</h2><p>{discoveryLabels[preview.discovery.kind]}</p><p>{preview.binding.root}</p>
        <p>명령 실행에 대한 신뢰는 별도로 확인합니다.</p>
        <label htmlFor="workspace-project-name">프로젝트 이름</label>
        <input id="workspace-project-name" value={name} maxLength={120} disabled={busy} onChange={event => setName(event.target.value)} />
        <button disabled={busy || !name.trim()} onClick={() => void act(async () => {
          await registryCall("apply_registration", {previewId:preview.previewId, name, action:["aliasOrMove","replacedRoot"].includes(preview.discovery.kind) ? "rebind" : "register"});
          currentPreview.current = null; setPreview(null); setRoot(""); setName(""); await refresh();
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
          <p>{worktree.binding.root}</p><p>{worktree.trustedDigest ? "실행 정의를 신뢰한 프로젝트" : "실행 신뢰 확인 전"}</p>
          <button disabled={busy || (context?.worktreeId === worktree.id && context.revision === worktree.revision)} onClick={() => void act(async () => {
            const next: ProjectContext = {projectId:worktree.projectId,worktreeId:worktree.id,revision:worktree.revision,target:worktree.binding.target};
            await registryCall("select_project", {context:next}); await onContextChanged();
          })}>프로젝트 선택</button>
          <button disabled={busy} onClick={() => setRemove(worktree)}>등록 해제</button>
          {remove?.id === worktree.id && <section aria-label="등록 해제 확인"><p>이 작업 폴더의 등록을 해제합니다. 실제 폴더와 Git 파일은 보존됩니다.</p>
            <button disabled={busy} onClick={() => void act(async () => {
              const context: ProjectContext = {projectId:worktree.projectId,worktreeId:worktree.id,revision:worktree.revision,target:worktree.binding.target};
              await registryCall("remove", {revision:registry.revision,context}); setRemove(null); await refresh();
            })}>해제 확인</button><button disabled={busy} onClick={() => setRemove(null)}>취소</button></section>}
        </div>)}
      </section>)}
    </>}
  </section>;
}
