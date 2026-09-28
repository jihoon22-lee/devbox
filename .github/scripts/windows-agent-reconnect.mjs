// One native reconnect, with read-only bounded diagnostics after failure.
export async function observeReconnectBaseline(item, enabled = false) {
  if (typeof enabled !== "boolean") throw new Error("invalid baseline selection");
  return enabled ? item.cdp.probeNewSession() : { state: "disabled" };
}
export async function reconnectAgent(item, phase, report) {
  if (
    !["workspace", "api-studio", "knowledge", "control-center"].includes(item.product) ||
    !["before-stop", "after-stop"].includes(phase)
  )
    throw new Error("invalid reconnect probe");
  const base = { product: item.product, phase };
  report({ ...base, stage: "submitted" });
  try {
    const result = await item.cdp.evaluate(
      "window.__TAURI_INTERNALS__.invoke('plugin:product-shell|agent_reconnect')",
      { timeoutMs: 35000 },
    );
    if (result !== "connected") throw new Error("agent reconnect did not connect");
    report({ ...base, stage: "passed" });
  } catch (error) {
    const { rendererResponsive, nativeStatus } = await readConnection(item);
    const connection = item.cdp.connectionState?.();
    const freshObserver = await item.cdp.probeNewSession?.().catch(() => ({ state: "observer_failed" }));
    const nativeObserver = await item.inspectCdpHost?.().catch(() => ({ state: "probe_failed" }));
    const failed = {
      ...base,
      stage: "failed",
      rendererResponsive,
      nativeStatus,
      ...(connection ? { connection } : {}),
      ...(freshObserver ? { freshObserver } : {}),
      ...(nativeObserver ? { nativeObserver } : {}),
      ...(item.cdpBaseline ? { freshObserverBaseline: item.cdpBaseline } : {}),
      ...(item.child ? { productExited: item.child.exitCode !== null || item.child.signalCode !== null } : {}),
    };
    report(failed);
    if (item.inspectCdpWaits) {
      failed.waitObserver = await item.inspectCdpWaits().catch(() => ({ state: "probe_failed" }));
      report({ ...failed });
    }
    if (item.inspectCdpStacks) {
      failed.stackObserver = await item.inspectCdpStacks().catch(() => ({ state: "probe_failed" }));
      report({ ...failed });
    }
    if (item.focusCdpHost) {
      const activation = await item.focusCdpHost().catch(() => ({ state: "probe_failed" }));
      const foregroundControl = { activation };
      if (activation.state === "focused") {
        foregroundControl.original = await readConnection(item);
        foregroundControl.fresh = await item.cdp.probeNewSession?.().catch(() => ({ state: "observer_failed" }));
      }
      report({ ...failed, foregroundControl });
    }
    throw error;
  }
}

async function readConnection(item) {
  const rendererResponsive = await item.cdp.evaluate("1", { timeoutMs: 1000 }).then(
    (value) => value === 1,
    () => false,
  );
  const nativeStatus = await item.cdp
    .evaluate("window.__TAURI_INTERNALS__.invoke('plugin:product-shell|agent_status')", { timeoutMs: 1000 })
    .then(
      (value) =>
        ["connected", "starting", "restarting", "unavailable", "unsupported"].includes(value) ? value : "unexpected",
      () => "probe_failed",
    );
  return { rendererResponsive, nativeStatus };
}
