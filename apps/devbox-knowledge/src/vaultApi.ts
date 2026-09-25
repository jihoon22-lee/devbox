import { setupCall } from "@devbox/knowledge-features/setup/api";
import type { VaultPreview } from "@devbox/knowledge-features/generated/VaultPreview";
import type { VaultActivated } from "@devbox/knowledge-features/generated/VaultActivated";
import { issueError } from "./issues";
export const vaultInvoke = setupCall;
export type VaultSchedule = import("@devbox/knowledge-features/generated/Schedule").Schedule;
export type { VaultPreview };
export function vaultJob(
  method: "prepare_vault_change",
  args: Record<string, never>,
  signal: AbortSignal,
  onCommitted: () => void,
): Promise<VaultPreview>;
export function vaultJob(
  method: "apply_vault_change",
  args: { id: string },
  signal: AbortSignal,
  onCommitted: () => void,
): Promise<VaultActivated>;
export async function vaultJob(
  method: "prepare_vault_change" | "apply_vault_change",
  args: { id?: string },
  signal: AbortSignal,
  onCommitted: () => void,
): Promise<VaultPreview | VaultActivated> {
  const { jobId } =
    method === "apply_vault_change" ? await setupCall(method, { id: args.id ?? "" }) : await setupCall(method, {});
  let committed = false;
  const failure = (issue?: string) =>
    committed
      ? new Error("폴더 연결은 저장되었습니다. 앱을 다시 시작해 마무리해 주세요.")
      : issueError(issue, "knowledge.setup");
  for (let attempt = 0; attempt < 400 && !signal.aborted; attempt++) {
    const result = await setupCall("vault_change_job", { id: jobId });
    if (signal.aborted) break;
    if ("committed" in result && result.committed) {
      committed = true;
      onCommitted();
    }
    if (result.state === "succeeded") {
      if (method === "prepare_vault_change" ? "previewId" in result.value : "active" in result.value)
        return result.value;
      throw failure();
    }
    if (result.state === "failed") throw failure(result.issue);
    await new Promise((resolve) => setTimeout(resolve, 150));
  }
  throw failure(signal.aborted ? "cancelled" : "vault_change_timeout");
}
