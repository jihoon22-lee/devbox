import { componentInvoke } from "@devbox/knowledge-features/transport";
import { issueError } from "./issues";
export const vaultInvoke = componentInvoke("knowledge.migration");
export interface VaultSchedule { id: string; target: string; previousRoot: string }
export interface VaultPreview { previewId: string; target: string; previousRoot: string; alreadyApplied: boolean; expiresInSeconds: number }
export async function vaultJob<T>(method: "prepare_vault_change" | "apply_vault_change", args: Record<string, unknown>, signal: AbortSignal, onCommitted: () => void): Promise<T> {
  const { jobId } = await vaultInvoke<{ jobId: string }>(method, args);
  let committed = false;
  const failure = (issue?: string) => committed ? new Error("폴더 연결은 저장되었습니다. 앱을 다시 시작해 마무리해 주세요.") : issueError(issue);
  for (let attempt = 0; attempt < 400 && !signal.aborted; attempt++) {
    const result = await vaultInvoke<{ state: string; value?: T; issue?: string; committed?: boolean }>("vault_change_job", { id: jobId });
    if (signal.aborted) break;
    if (result.committed) { committed = true; onCommitted(); }
    if (result.state === "succeeded") return result.value as T;
    if (result.state === "failed") throw failure(result.issue);
    await new Promise(resolve => setTimeout(resolve, 150));
  }
  throw failure(signal.aborted ? "cancelled" : "vault_change_timeout");
}
