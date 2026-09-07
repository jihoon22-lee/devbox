import { useEffect, useRef, useState } from "react";
import { componentInvoke } from "../transport";
import { isTauri } from "./lib/isTauri";
import { parseStore } from "./lib/collections";
import { toRequestTemplate } from "./lib/persistence";
import { MockDraftAction } from "../webhooks/MockDraftAction";
import type { RequestTemplate } from "./types";
const invoke = componentInvoke("api-studio.api");
export interface DefinitionSummary { id: string; name: string; operationCount: number }
interface Operation { label: string; method: string; requestTarget: string; mockStatus: number | null; request: RequestTemplate }
interface Definition { schemaVersion: 1; id: string; name: string; openApiVersion: "3.0" | "3.1"; operations: Operation[] }
const errorText = "저장한 OpenAPI 작업을 확인하지 못했습니다. 보관한 자료와 현재 요청은 유지됩니다.";
function record(value: unknown): value is Record<string, unknown> { return !!value && typeof value === "object" && !Array.isArray(value); }
function uuid(value: unknown): value is string { return typeof value === "string" && /^[a-f0-9-]{36}$/u.test(value); }
export function parseDefinition(value: unknown): Definition {
  if (!record(value) || value.schemaVersion !== 1 || !uuid(value.id) || typeof value.name !== "string" || !["3.0", "3.1"].includes(value.openApiVersion as string)
    || !Array.isArray(value.operations) || !value.operations.length || value.operations.length > 1000) throw new Error(errorText);
  const operations = value.operations.map((op, index) => {
    if (!record(op) || typeof op.label !== "string" || typeof op.method !== "string" || typeof op.requestTarget !== "string" || !op.requestTarget.startsWith("/")
      || (op.mockStatus !== null && (!Number.isInteger(op.mockStatus) || (op.mockStatus as number) < 100 || (op.mockStatus as number) > 599))) throw new Error(errorText);
    const parsed = parseStore(JSON.stringify({ version: 2, collections: [{ id: `projection-${index}`, name: op.label, folder: "OpenAPI", saved_at: 1, requiresSecretReview: true, request: op.request }] }));
    const request = parsed?.collections[0]?.request;
    if (!request || request.method !== op.method || request.requiresSecretReview !== true) throw new Error(errorText);
    return { label: op.label, method: op.method, requestTarget: op.requestTarget, mockStatus: op.mockStatus as number | null, request: toRequestTemplate(request) };
  });
  return { schemaVersion: 1, id: value.id, name: value.name, openApiVersion: value.openApiVersion as Definition["openApiVersion"], operations };
}
export function OpenApiDefinitions({ revision, linkedIds, onSummaries, onApply, disabled }: {
  revision: number; linkedIds: string[] | null; onSummaries: (items: DefinitionSummary[]) => void; onApply: (request: RequestTemplate) => void; disabled: boolean;
}) {
  const [items, setItems] = useState<DefinitionSummary[]>([]); const [definition, setDefinition] = useState<Definition | null>(null);
  const [error, setError] = useState(""); const [busy, setBusy] = useState(false); const [selected, setSelected] = useState(0); const [confirmDelete, setConfirmDelete] = useState(false);
  const running = useRef(false); const mounted = useRef(false); const generation = useRef(0); const summaries = useRef(onSummaries); summaries.current = onSummaries;
  async function list() {
    const result = await invoke<unknown>("list_openapi_definitions");
    if (!Array.isArray(result) || result.length > 32 || result.some(item => !record(item) || !uuid(item.id) || typeof item.name !== "string" || !Number.isSafeInteger(item.operationCount) || (item.operationCount as number) < 1)) throw new Error(errorText);
    return result as DefinitionSummary[];
  }
  useEffect(() => {
    mounted.current = true; const version = ++generation.current; let alive = true;
    if (isTauri()) void list().then(result => { if (alive && generation.current === version) { setItems(result); summaries.current(result); setError(""); } }).catch(() => { if (alive) setError(errorText); });
    return () => { alive = false; mounted.current = false; generation.current += 1; };
  }, [revision]);
  async function open(id: string) {
    if (running.current) return;
    running.current = true; setBusy(true); setError(""); const version = ++generation.current;
    try {
      const result = parseDefinition(await invoke<unknown>("get_openapi_definition", { id }));
      if (result.id !== id) throw new Error(errorText);
      if (mounted.current && generation.current === version) { setDefinition(result); setSelected(0); setConfirmDelete(false); }
    } catch { if (mounted.current && generation.current === version) setError(errorText); }
    finally { running.current = false; if (mounted.current) setBusy(false); }
  }
  async function remove() {
    if (!definition || running.current) return;
    running.current = true; setBusy(true); setError(""); const version = ++generation.current;
    try {
      await invoke("delete_openapi_definition", { id: definition.id });
      if (mounted.current && generation.current === version) {
        const remaining = items.filter(item => item.id !== definition.id); setItems(remaining); summaries.current(remaining); setDefinition(null); setConfirmDelete(false);
      }
      const result = await list();
      if (mounted.current && generation.current === version) { setItems(result); summaries.current(result); setDefinition(null); setConfirmDelete(false); }
    } catch { if (mounted.current && generation.current === version) setError("삭제 결과 또는 목록 갱신을 확인하지 못했습니다. 다시 열어 보관 상태를 확인하세요."); }
    finally { running.current = false; if (mounted.current) setBusy(false); }
  }
  const operation = definition?.operations[selected];
  return <details className="api-workspace" aria-label="보관한 OpenAPI 작업"><summary>보관한 OpenAPI 작업 ({items.filter(item => !linkedIds || linkedIds.includes(item.id)).length})</summary>
    <p className="dim">지원하는 요청과 Mock 초안을 보관합니다. 원문 전체와 지원하지 않는 스키마는 원래 문서에서 확인하세요.</p>
    {error && <p role="alert">{error}</p>}
    <div className="api-workspace-toolbar">{items.filter(item => !linkedIds || linkedIds.includes(item.id)).map(item => <button className="btn mini" key={item.id} disabled={busy} onClick={() => void open(item.id)}>{item.name} · {item.operationCount}개</button>)}</div>
    {definition && operation && <div className="api-workspace-editor"><h2>{definition.name} · OpenAPI {definition.openApiVersion}</h2>
      <label>보관한 작업 <select value={selected} disabled={busy} onChange={event => setSelected(Number(event.currentTarget.value))}>{definition.operations.map((op, index) => <option key={index} value={index}>{op.label}</option>)}</select></label>
      <p>마스킹된 요청입니다. 현재 요청에 적용한 뒤 필요한 환경·인증을 확인하고 직접 전송하세요.</p>
      <p>{operation.method} {operation.requestTarget}</p><pre>{operation.request.url}</pre>
      <div className="api-workspace-toolbar"><button className="btn" disabled={busy || disabled} onClick={() => onApply(structuredClone(operation.request))}>현재 요청 대신 적용</button>
        {operation.mockStatus !== null && <MockDraftAction owner="api-studio.api" value="" status={operation.mockStatus} requestTarget={operation.requestTarget} requestMethod={operation.method} label="Mock 초안으로 전달" disabled={busy} />}
        <button className="btn" disabled={busy} onClick={() => setDefinition(null)}>미리보기 닫기</button>
      </div>
      {confirmDelete ? <div className="api-workspace-toolbar"><span>보관한 작업을 삭제합니다. Workspace의 연결 ID는 남으며 원본 문서는 유지됩니다.</span><button className="btn" disabled={busy} onClick={() => setConfirmDelete(false)}>취소</button><button className="btn" disabled={busy} onClick={() => void remove()}>보관한 작업 삭제 확인</button></div> : <button className="btn" disabled={busy} onClick={() => setConfirmDelete(true)}>보관한 작업 삭제…</button>}
    </div>}
  </details>;
}
