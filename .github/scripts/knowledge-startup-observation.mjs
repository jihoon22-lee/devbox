import { foundationMode } from "./product-foundation-observation.mjs";
import { observeWindowsCdpHost } from "./windows-cdp-host.mjs";
import { observeWindowsCdpWaits } from "./windows-cdp-waits.mjs";

export function knowledgeLifecycleMode(args, env) {
  if (args.length === 0) return foundationMode([], env);
  if (args.length !== 1 || args[0] !== "--retained-diagnostic")
    throw new Error("Unsupported Knowledge lifecycle arguments");
  return foundationMode(["--knowledge-diagnostic"], env);
}

// Failure-only observation: never attach, navigate, focus, or replay a product request.
export async function observeKnowledgeStartupTargets(port, request = fetch) {
  const response = await request(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(1000) });
  if (!response.ok) return { state: "http_error", status: response.status };
  const body = await response.text();
  if (body.length > 1_048_576) return { state: "invalid_response" };
  const targets = JSON.parse(body);
  if (!Array.isArray(targets) || targets.length > 32) return { state: "invalid_response" };
  const pages = targets
    .filter((target) => target?.type === "page")
    .map((target) => {
      let url = target.url === "about:blank" ? "about:blank" : "other";
      try {
        const parsed = new URL(target.url);
        if (
          ((parsed.protocol === "http:" || parsed.protocol === "https:") && parsed.hostname === "tauri.localhost") ||
          (parsed.protocol === "tauri:" && parsed.hostname === "localhost")
        ) {
          if (["/", "/index.html"].includes(parsed.pathname))
            url = `${parsed.protocol}//${parsed.hostname}${parsed.pathname}`;
        }
      } catch {
        /* Keep only a fixed category for unknown locations. */
      }
      return { title: ["about:blank", "Devbox Knowledge", ""].includes(target.title) ? target.title : "other", url };
    });
  return { state: "observed", status: response.status, pages };
}

export async function observeKnowledgeStartupFailure(
  error,
  item,
  port,
  evidence,
  { host = observeWindowsCdpHost, waits = observeWindowsCdpWaits, targets = observeKnowledgeStartupTargets } = {},
) {
  const observation = {
    identityCaptured: Boolean(item.identity),
    childAlive: item.child?.exitCode === null && item.child?.signalCode === null,
  };
  evidence.startupFailure = observation;
  for (const [field, inspect] of [
    ["cdpTargets", () => targets(port)],
    ["nativeObserver", () => host(item.identity, port)],
    ["waitObserver", () => waits(item.identity, port)],
  ]) {
    try {
      observation[field] = await inspect();
    } catch {
      observation[field] = { state: "probe_failed" };
    }
  }
  throw error;
}
