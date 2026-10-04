// Self-contained: serialized into the owned renderer without arguments or results.
export function knowledgeStepObserver(marker) {
  return async (step, operation) => {
    const entry = { step, startedMs: Date.now(), state: "started", elapsedMs: 0 };
    marker.steps.push(entry);
    try {
      const value = await operation();
      entry.state = "completed";
      return value;
    } catch (error) {
      entry.state = "rejected";
      throw error;
    } finally {
      entry.endedMs = Date.now();
      entry.elapsedMs = entry.endedMs - entry.startedMs;
    }
  };
}
export function foundationMode(args, env) {
  const knowledgeOnly = args.length === 1 && args[0] === "--knowledge-diagnostic";
  const diagnostic = knowledgeOnly || (args.length === 1 && args[0] === "--product-sequence-diagnostic");
  if (!diagnostic && args.some((arg) => arg !== "--smoke-only")) throw new Error("Unsupported foundation arguments");
  if (diagnostic && !/^[a-f0-9]{40}$/.test(env.DEVBOX_SUITE_ARTIFACT_SOURCE ?? ""))
    throw new Error("Exact diagnostic payload source required");
  return {
    diagnostic,
    knowledgeOnly,
    smokeOnly: args.includes("--smoke-only"),
    evidence: diagnostic
      ? {
          source: env.DEVBOX_SUITE_ARTIFACT_SOURCE,
          payloadSource: env.DEVBOX_SUITE_ARTIFACT_SOURCE,
          fixtureSource: env.GITHUB_SHA,
          diagnosticOnly: true,
          promotionEvidence: false,
        }
      : { source: env.GITHUB_SHA },
  };
}

export function cleanupObservation(result, probeRoot) {
  let rootPidExists = null;
  try {
    probeRoot();
    rootPidExists = true;
  } catch (error) {
    if (error?.code === "ESRCH") rootPidExists = false;
    else if (error?.code === "EPERM") rootPidExists = true;
  }
  const code = (value) => (typeof value === "string" && /^[A-Z0-9_]{1,32}$/.test(value) ? value : null);
  return {
    taskkillAttempted: result !== undefined,
    status: Number.isInteger(result?.status) ? result.status : null,
    signal: code(result?.signal),
    error: result?.error ? (code(result.error.code) ?? "UNKNOWN") : null,
    rootPidExists,
  };
}
