import { componentInvoke, type Component } from "../transport";
import type { OutputSource } from "../transforms/tools/outputPolicy";

export type DraftOwner = Extract<Component, "api-studio.api" | "api-studio.transforms">;
export interface DraftSummary {
  artifact: { id: string; kind: "knowledge-draft/v1"; provenance: {
    product: "api-studio"; component: DraftOwner; requestId: string; revision: number;
  } };
  createdAtMs: number;
  title: string;
  redacted: boolean;
}
export interface KnowledgeDraft extends DraftSummary { body: string }
const ERROR = "보관한 Knowledge 초안을 확인하지 못했습니다.";
const record = (value: unknown): value is Record<string, unknown> => !!value && typeof value === "object" && !Array.isArray(value);
export function parseSummary(value: unknown, owner: DraftOwner): DraftSummary {
  if (!record(value) || !record(value.artifact) || !record(value.artifact.provenance)) throw new Error(ERROR);
  const { artifact } = value; const provenance = artifact.provenance as Record<string, unknown>;
  if (artifact.kind !== "knowledge-draft/v1" || typeof artifact.id !== "string"
    || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/u.test(artifact.id)
    || provenance.product !== "api-studio" || provenance.component !== owner
    || typeof provenance.requestId !== "string" || !/^[a-zA-Z0-9-]{1,64}$/u.test(provenance.requestId)
    || !Number.isSafeInteger(provenance.revision) || (provenance.revision as number) < 1
    || !Number.isSafeInteger(value.createdAtMs) || (value.createdAtMs as number) < 1
    || typeof value.redacted !== "boolean"
    || value.title !== `API Studio · ${owner === "api-studio.api" ? "Requests" : "Transforms"} 결과`) throw new Error(ERROR);
  return { artifact: artifact as unknown as DraftSummary["artifact"], createdAtMs: value.createdAtMs as number,
    title: value.title, redacted: value.redacted };
}
export function parseDraft(value: unknown, owner: DraftOwner): KnowledgeDraft {
  const summary = parseSummary(value, owner);
  const body = (value as Record<string, unknown>).body;
  if (typeof body !== "string" || !body.trim() || body.includes("\0") || body.length > 512 * 1024
    || new TextEncoder().encode(body).length > 512 * 1024) throw new Error(ERROR);
  return { ...summary, body };
}
export async function saveDraft(owner: DraftOwner, output: string, source?: OutputSource): Promise<KnowledgeDraft> {
  const result = await componentInvoke(owner)<unknown>("save_knowledge_draft", { output, ...(source ? { source } : {}) });
  if (!record(result) || result.delivery !== "unavailable") throw new Error(ERROR);
  return parseDraft(result.draft, owner);
}
export async function listDrafts(owner: DraftOwner): Promise<DraftSummary[]> {
  const result = await componentInvoke(owner)<unknown>("list_knowledge_drafts");
  if (!Array.isArray(result) || result.length > 50) throw new Error(ERROR);
  return result.map(value => parseSummary(value, owner));
}
export async function getDraft(owner: DraftOwner, id: string): Promise<KnowledgeDraft> {
  return parseDraft(await componentInvoke(owner)<unknown>("get_knowledge_draft", { id }), owner);
}
export async function deleteDraft(owner: DraftOwner, id: string): Promise<void> {
  await componentInvoke(owner)("delete_knowledge_draft", { id });
}
export function draftError(error: unknown): string {
  return error instanceof Error && error.message === "knowledge_storage_full"
    ? "보관함이 가득 찼습니다. 기존 초안을 내보낸 뒤 직접 삭제하고 다시 시도하세요."
    : "초안 작업을 완료하지 못했습니다. 기존 보관함은 유지됩니다.";
}
export function exportDraft(draft: KnowledgeDraft): void {
  const url = URL.createObjectURL(new Blob([draft.body], { type: "text/plain;charset=utf-8" }));
  try {
    const link = document.createElement("a"); link.href = url;
    link.download = `api-studio-knowledge-${draft.artifact.id}.txt`; link.click();
  } finally { URL.revokeObjectURL(url); }
}
