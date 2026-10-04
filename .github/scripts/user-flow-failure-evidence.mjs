import { mkdir, open } from "node:fs/promises";
import path from "node:path";

// Assertion dumps can contain response bodies or credentials. Keep the first
// diagnostic line and source frames only, never actual/expected/cause objects.
export function boundedFailure(error) {
  const clean = (value) =>
    String(value)
      .replace(/Bearer\s+\S+/gi, "Bearer [redacted]")
      .replace(/(?:token|secret|password|authorization|api[_-]?key)\s*[:=]\s*\S+/gi, "credential=[redacted]")
      .replace(/(["'`])[^\n]*?\1/g, "[redacted]")
      .replace(/\b[a-zA-Z0-9_-]{40,}\b/g, "[redacted]");
  return {
    name: /^[A-Za-z][A-Za-z0-9]{0,63}$/.test(error?.name ?? "") ? error.name : "Error",
    message: clean(String(error?.message ?? error).split("\n")[0]).slice(0, 1000),
    stack: String(error?.stack ?? "")
      .split("\n")
      .filter((line) => /^\s+at /.test(line))
      .map(clean)
      .slice(0, 12)
      .join("\n")
      .slice(0, 4000),
  };
}
export async function preserveUserFlowFailure(
  product,
  error,
  { ui, identity = {}, evidenceRoot = "product-foundation-evidence" } = {},
) {
  if (!["workspace", "api-studio", "knowledge"].includes(product)) throw new Error("Invalid failure product");
  await mkdir(evidenceRoot, { recursive: true });
  const output = await open(path.join(evidenceRoot, `${product}-user-flow-failure.json`), "wx");
  try {
    const observation = { screenshotPath: null, screenshotUnavailable: false };
    try {
      if (ui) observation.screenshotPath = await ui.screenshot(`${product}-first-failure`);
      else observation.screenshotUnavailable = true;
    } catch {
      observation.screenshotUnavailable = true;
    }
    const {
      sourceSha,
      fixtureSha,
      artifactDigests,
      diagnosticOnly,
      promotionEvidence,
      payloadSourceSha,
      payloadRunId,
    } = identity;
    await output.writeFile(
      JSON.stringify(
        {
          schemaVersion: 1,
          product,
          status: "FAIL",
          sourceSha,
          fixtureSha,
          artifactDigests,
          diagnosticOnly,
          promotionEvidence,
          payloadSourceSha,
          payloadRunId,
          runnerSourceSha: process.env.GITHUB_SHA,
          runnerRunId: process.env.GITHUB_RUN_ID,
          error: boundedFailure(error),
          ...observation,
        },
        null,
        2,
      ),
    );
  } finally {
    await output.close();
  }
}
