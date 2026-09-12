/** Preserve the same native receipt after renderer loss or an ambiguous reply. */
export async function runDockerControl(
  call: <T>(method: string, args: Record<string, unknown>) => Promise<T>,
  distro: string, containerId: string, action: string,
): Promise<void> {
  const key = `terminal-docker-control:${JSON.stringify([distro, containerId, action])}`;
  const operationId = sessionStorage.getItem(key) ?? crypto.randomUUID();
  sessionStorage.setItem(key, operationId);
  try {
    await call("docker_action", { operationId, distro, containerId, action });
    sessionStorage.removeItem(key);
  } catch (error) {
    try {
      const receipt = await call<{ state: string }>("wsl_control_status", { operationId });
      if (receipt.state === "completed") { sessionStorage.removeItem(key); return; }
      if (receipt.state === "failed") sessionStorage.removeItem(key);
    } catch { /* An unknown result must retain its original operation identity. */ }
    throw error;
  }
}
