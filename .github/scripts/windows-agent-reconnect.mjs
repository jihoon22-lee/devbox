// One native reconnect, with read-only bounded diagnostics after failure.
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
    report({ ...base, stage: "failed", rendererResponsive, nativeStatus });
    throw error;
  }
}
