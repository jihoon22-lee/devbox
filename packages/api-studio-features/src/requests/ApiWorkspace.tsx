import { useEffect, useRef, useState } from "react";
import { componentInvoke } from "../transport";
import { isTauri } from "./lib/isTauri";
import "./apiWorkspace.css";
const invoke = componentInvoke("api-studio.api");
export interface WorkspaceLinks { collectionIds: string[]; environmentIds: string[]; openApiDefinitionIds: string[]; mockProfileIds: string[] }
export interface ApiWorkspace { id: string; name: string; projectId: string | null; links: WorkspaceLinks }
interface Document { schemaVersion: 1; revision: number; selectedId: string | null; workspaces: ApiWorkspace[] }
interface Item { id: string; name: string }
interface State { document: Document; currentProjectId: string | null; mockProfiles: { id: string; label: string }[] }
interface Edit { id: string | null; name: string; association: "keep" | "standalone" | "current-project"; links: WorkspaceLinks; expectedRevision: number }
const emptyLinks = (): WorkspaceLinks => ({ collectionIds: [], environmentIds: [], openApiDefinitionIds: [], mockProfileIds: [] });
const message = "Workspace 정보를 저장하지 못했습니다. 초안은 유지됩니다. 연결 목록을 새로고침한 뒤 다시 편집하세요.";
function record(value: unknown): value is Record<string, unknown> { return !!value && typeof value === "object" && !Array.isArray(value); }
export function parseWorkspaceDocument(value: unknown): Document {
  if (!record(value) || value.schemaVersion !== 1 || !Number.isSafeInteger(value.revision) || (value.revision as number) < 0
    || (value.selectedId !== null && typeof value.selectedId !== "string") || !Array.isArray(value.workspaces) || value.workspaces.length > 64) throw new Error(message);
  const ids = new Set<string>();
  for (const workspace of value.workspaces) {
    if (!record(workspace) || typeof workspace.id !== "string" || !/^[a-f0-9-]{36}$/u.test(workspace.id) || ids.has(workspace.id)
      || typeof workspace.name !== "string" || !workspace.name.trim() || workspace.name.length > 240
      || (workspace.projectId !== null && typeof workspace.projectId !== "string") || !record(workspace.links)) throw new Error(message);
    ids.add(workspace.id);
    for (const key of ["collectionIds", "environmentIds", "openApiDefinitionIds", "mockProfileIds"]) {
      const list = workspace.links[key];
      if (!Array.isArray(list) || list.length > 10_000 || list.some(id => typeof id !== "string" || !id || id.length > 4096) || new Set(list).size !== list.length) throw new Error(message);
    }
  }
  if (value.selectedId !== null && !ids.has(value.selectedId as string)) throw new Error(message);
  return value as unknown as Document;
}
export function ApiWorkspacePanel({ collections, environments, definitions = [], onChange }: {
  collections: Item[]; environments: Item[]; definitions?: Item[]; onChange: (workspace: ApiWorkspace | null) => void;
}) {
  const [state, setState] = useState<State | null>(null); const [busy, setBusy] = useState(false); const [error, setError] = useState("");
  const [edit, setEdit] = useState<Edit | null>(null); const [deleteId, setDeleteId] = useState<string | null>(null);
  const running = useRef(false); const mounted = useRef(false); const version = useRef(0); const change = useRef(onChange); change.current = onChange;
  const selected = state?.document.workspaces.find(v => v.id === state.document.selectedId) ?? null;
  function applyDocument(document: Document) {
    setState(previous => previous ? { ...previous, document } : previous);
    change.current(document.workspaces.find(v => v.id === document.selectedId) ?? null);
  }
  async function refresh() {
    if (!isTauri() || running.current) return;
    running.current = true; setBusy(true); setError(""); const current = ++version.current;
    try {
      const result = await invoke<State>("api_workspace_state"); const document = parseWorkspaceDocument(result?.document);
      if ((result.currentProjectId !== null && typeof result.currentProjectId !== "string") || !Array.isArray(result.mockProfiles)
        || result.mockProfiles.length > 64 || result.mockProfiles.some(v => typeof v.id !== "string" || typeof v.label !== "string")) throw new Error(message);
      if (mounted.current && current === version.current) {
        setState({ ...result, document }); change.current(document.workspaces.find(v => v.id === document.selectedId) ?? null);
      }
    } catch { if (mounted.current && current === version.current) setError(message); }
    finally { if (current === version.current) { running.current = false; if (mounted.current) setBusy(false); } }
  }
  useEffect(() => {
    mounted.current = true; running.current = false; void refresh();
    return () => { mounted.current = false; version.current += 1; };
  }, []);
  async function mutate(method: string, args: Record<string, unknown>) {
    if (running.current || !state) return;
    running.current = true; setBusy(true); setError(""); const current = ++version.current;
    try {
      const document = parseWorkspaceDocument(await invoke<unknown>(method, args));
      if (mounted.current && current === version.current) { applyDocument(document); setEdit(null); setDeleteId(null); }
    } catch { if (mounted.current && current === version.current) setError(message); }
    finally { if (current === version.current) { running.current = false; if (mounted.current) setBusy(false); } }
  }
  function editWorkspace(workspace: ApiWorkspace | null) {
    if (!state) return;
    setDeleteId(null); setError("");
    setEdit({ id: workspace?.id ?? null, name: workspace?.name ?? "", association: workspace ? "keep" : "standalone",
      links: structuredClone(workspace?.links ?? emptyLinks()), expectedRevision: state.document.revision });
  }
  function choices(key: keyof WorkspaceLinks, label: string, items: Item[]) {
    if (!edit) return null;
    const selectedIds = edit.links[key]; const known = new Set(items.map(item => item.id));
    const all = [...items, ...selectedIds.filter(id => !known.has(id)).map(id => ({ id, name: `사용할 수 없는 연결 · ${id}` }))];
    return <fieldset><legend>{label}</legend>{all.length === 0 ? <p className="dim">연결할 항목이 없습니다.</p> : <div className="api-workspace-choices">{all.map(item => <label key={item.id}>
      <input type="checkbox" checked={selectedIds.includes(item.id)} disabled={busy} onChange={event => {
        const checked = event.currentTarget.checked;
        setEdit(previous => previous ? { ...previous, links: { ...previous.links, [key]: checked ? [...previous.links[key], item.id] : previous.links[key].filter(id => id !== item.id) } } : previous);
      }} />{item.name}</label>)}</div>}</fieldset>;
  }
  return <section className="api-workspace" aria-label="API Workspace" aria-busy={busy}>
    <div className="api-workspace-toolbar"><label>API Workspace <select aria-label="API Workspace 선택" value={state?.document.selectedId ?? ""} disabled={!state || busy || !!edit}
      onChange={event => void mutate("select_api_workspace", { expectedRevision: state!.document.revision, id: event.currentTarget.value || null })}>
      <option value="">모든 자료</option>{state?.document.workspaces.map(workspace => <option key={workspace.id} value={workspace.id}>{workspace.name}</option>)}
    </select></label>
      <button className="btn mini" disabled={!state || busy || !!edit} onClick={() => editWorkspace(null)}>새 Workspace</button>
      <button className="btn mini" disabled={!selected || busy || !!edit} onClick={() => editWorkspace(selected)}>연결 편집</button>
      <button className="btn mini" disabled={!isTauri() || busy || !!edit} onClick={() => void refresh()}>연결 목록 새로고침</button>
    </div>
    {!isTauri() ? <p className="dim">Workspace 저장은 데스크톱 앱에서 사용할 수 있습니다.</p> : <p className="dim">{selected ? `${selected.projectId ? `Project · ${selected.projectId}` : "독립 Workspace"} · 컬렉션 ${selected.links.collectionIds.length} · 환경 ${selected.links.environmentIds.length} · OpenAPI ${selected.links.openApiDefinitionIds.length} · Mock ${selected.links.mockProfileIds.length}` : "전체 컬렉션과 환경을 표시합니다."} 목록을 바꿔도 요청 초안·현재 환경·연결은 유지됩니다.</p>}
    {error && <p role="alert">{error}</p>}
    {edit && <form className="api-workspace-editor" onSubmit={event => { event.preventDefault(); void mutate("save_api_workspace", edit as unknown as Record<string, unknown>); }}>
      <h2>{edit.id ? "Workspace 연결 편집" : "새 Workspace"}</h2>
      <label>이름 <input aria-label="Workspace 이름" value={edit.name} maxLength={240} disabled={busy} onChange={event => setEdit({ ...edit, name: event.currentTarget.value })} /></label>
      <label>Project 연결 <select value={edit.association} disabled={busy} onChange={event => setEdit({ ...edit, association: event.currentTarget.value as Edit["association"] })}>
        {edit.id && <option value="keep">현재 연결 유지</option>}<option value="standalone">독립 Workspace</option><option value="current-project" disabled={!state?.currentProjectId}>현재 Project{state?.currentProjectId ? ` · ${state.currentProjectId}` : " (연결 없음)"}</option>
      </select></label>
      {choices("collectionIds", "컬렉션", collections)}{choices("environmentIds", "환경", environments)}
      {choices("openApiDefinitionIds", "저장한 OpenAPI 작업", definitions)}{choices("mockProfileIds", "저장한 Mock 프로필", state?.mockProfiles.map(v => ({ id: v.id, name: v.label })) ?? [])}
      <div className="api-workspace-toolbar"><button type="button" className="btn" disabled={busy} onClick={() => { setEdit(null); setError(""); }}>취소</button><button className="btn" disabled={busy || !edit.name.trim()}>연결 저장</button></div>
    </form>}
    {selected && !edit && <div className="api-workspace-delete">{deleteId === selected.id ? <><span>이 Workspace의 연결 목록을 삭제합니다. 연결한 자료는 유지됩니다.</span><button className="btn mini" disabled={busy} onClick={() => setDeleteId(null)}>취소</button><button className="btn mini" disabled={busy} onClick={() => void mutate("delete_api_workspace", { expectedRevision: state!.document.revision, id: selected.id })}>Workspace 삭제 확인</button></> : <button className="btn mini" disabled={busy} onClick={() => setDeleteId(selected.id)}>Workspace 삭제…</button>}</div>}
  </section>;
}
