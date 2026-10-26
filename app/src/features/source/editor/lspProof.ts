import type { WorkspaceBaseline, WorkspaceIdentity, WorkspaceProof, WorkspaceValidation } from "../../../rpc/gen/rpc";

export type { WorkspaceBaseline, WorkspaceIdentity, WorkspaceProof, WorkspaceValidation };
export async function textHash(text: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}
export function proofIdentity(proof: WorkspaceProof): WorkspaceIdentity {
  return { token: proof.token, requestId: proof.requestId, rootUri: proof.rootUri, generation: proof.generation };
}
