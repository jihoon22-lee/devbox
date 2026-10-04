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
  const diagnostic = args.length === 1 && args[0] === "--knowledge-diagnostic";
  if (!diagnostic && args.some((arg) => arg !== "--smoke-only")) throw new Error("Unsupported foundation arguments");
  if (diagnostic && !/^[a-f0-9]{40}$/.test(env.DEVBOX_SUITE_ARTIFACT_SOURCE ?? ""))
    throw new Error("Exact diagnostic payload source required");
  return {
    diagnostic,
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
